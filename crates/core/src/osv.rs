//! The OSV advisory client: the engine's only network call, and its on-disk
//! snapshot.
//!
//! Everything the network does here is optional. [`check`] asks osv.dev which of
//! a lockfile's packages have advisories, but it writes every answer into the
//! index first, so the next run can produce the same findings with the network
//! unplugged. Under `--offline`, or when a request fails for any reason at all,
//! the snapshot is used instead and the run says so in a warning. There is no
//! path through this module that turns a network problem into a failed run
//! (spec 9): the worst case is an empty result and one warning.
//!
//! One failure is enough. A batch request that does not answer marks the
//! network down for the rest of that run, so the advisory details behind it are
//! served from the snapshot rather than each paying the same [`TIMEOUT`] over
//! again: an unreachable registry costs one timeout, not one per advisory.
//!
//! The snapshot is keyed by the lockfile's package list (see
//! [`crate::lockfile::Lockfile::hash`]), because the response is stored as one
//! document and zipped positionally against that list: the key has to promise
//! that the list has not moved under it. Installing or upgrading a package
//! invalidates the snapshot, which is exactly when the answer can change, and a
//! snapshot from today is reused without a request even when the run is online.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Context;
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};

use crate::index::Index;
use crate::lockfile::{Lockfile, Package};

/// The batch endpoint: many packages in, advisory ids out.
pub const BATCH_URL: &str = "https://api.osv.dev/v1/querybatch";
/// The detail endpoint, an advisory id appended.
pub const VULN_URL: &str = "https://api.osv.dev/v1/vulns/";

/// osv.dev caps a batch query at 1000 entries.
const CHUNK: usize = 1000;
const DAY: u64 = 86_400;
/// How long a cached advisory detail is used without asking again. Summaries and
/// fixed versions are edited rarely, and a month-old one is still accurate
/// enough to act on.
const VULN_MAX_AGE_DAYS: u64 = 30;
/// The whole request, connect included, gives up after this. A quality gate that
/// blocks on a slow registry is worse than one that reports from its snapshot.
const TIMEOUT: Duration = Duration::from_secs(10);

/// What an advisory says about one package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Advisory {
    pub id: String,
    pub summary: String,
    /// `CRITICAL`, `HIGH`, `MODERATE`, `LOW`, or `UNKNOWN` when the advisory does
    /// not rate itself.
    pub severity: String,
    /// The version that fixes the installed one: the `fixed` event of the
    /// affected range the installed version falls in, when that range names one.
    pub fixed: Option<String>,
    /// Whether the detail document was read and held no affected range covering
    /// the installed version, which is the batch endpoint and the document
    /// disagreeing about what is affected.
    ///
    /// It separates two findings that both carry no `fixed` and cannot honestly
    /// say the same thing. A range that holds the version and names no upgrade
    /// is the advisory saying no fix has shipped for the branch the repository
    /// is on; no range at all is the advisory saying nothing about this version.
    /// False when the detail document was unavailable: nothing was read, so
    /// nothing disagrees.
    pub outside_every_range: bool,
    pub aliases: Vec<String>,
}

/// One installed package matched to one advisory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub package: Package,
    pub advisory: Advisory,
}

/// The result of a check: what was found, what the reader should know about how
/// it was found, and how old the data behind it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub hits: Vec<Hit>,
    pub warnings: Vec<String>,
    /// The age of the snapshot the hits came from, zero when it was just fetched
    /// and `None` when there was no snapshot at all.
    pub snapshot_age_days: Option<u64>,
}

/// The advisories affecting `lock`, from the network when it is allowed and
/// answers, and from the index's snapshot otherwise.
pub fn check(
    ix: &Index,
    lock: &Lockfile,
    offline: bool,
    fetch: &dyn Fn(&str, Option<&str>) -> anyhow::Result<String>,
) -> anyhow::Result<Outcome> {
    let mut warnings = Vec::new();
    if lock.packages.is_empty() {
        return Ok(Outcome { hits: Vec::new(), warnings, snapshot_age_days: None });
    }
    let now = now_secs();
    // A stored row whose JSON does not parse is a row this run cannot use: it
    // was truncated by a full disk, or written by a version of this engine that
    // stored something else. Reading it as an answer would report zero
    // advisories for a repository that has them, silently, so it is treated as
    // no snapshot at all. An online run then fetches over it; an offline one
    // says the rule was skipped, which is the honest answer.
    let cached = match cached_batch(ix, &lock.hash)? {
        Some((_, json)) if serde_json::from_str::<Value>(&json).is_err() => {
            warnings.push("the cached advisory snapshot is unreadable; ignoring it".to_string());
            None
        }
        row => row,
    };

    let mut batch = None;
    let mut snapshot_age_days = None;
    // A failed request says the network is unreachable for this run, not for
    // that one request: the registry is down, the machine is on a train, the
    // proxy refuses. Every advisory detail behind the batch is stale by the same
    // clock, so without this the run pays the whole [`TIMEOUT`] again once per
    // advisory found and then serves each of them from the snapshot anyway. The
    // rest of the run therefore behaves as an offline one: cached details are
    // served, and a detail with no cached copy reads UNKNOWN, exactly as today.
    let mut network_down = false;
    // A snapshot from today already answers for this exact package list, and one
    // day is the age at which the run would start warning about it, so it is
    // also the age at which an online run stops trusting it. Below that the run
    // asks the network nothing: a repository whose lockfile has not moved costs
    // no requests however often it is checked, which is what makes the rule
    // affordable in a pre-commit hook.
    let fresh = cached.as_ref().is_some_and(|(fetched_at, _)| age_in_days(now, *fetched_at) < 1);
    if !offline && !fresh {
        match fetch_batch(lock, fetch) {
            Ok(json) => {
                store_batch(ix, &lock.hash, now, &json)?;
                batch = Some(json);
                snapshot_age_days = Some(0);
            }
            Err(e) => {
                network_down = true;
                warnings.push(format!("advisory lookup failed ({e}); falling back to the cached snapshot"));
            }
        }
    }
    if batch.is_none() {
        match cached {
            Some((fetched_at, json)) => {
                let age = age_in_days(now, fetched_at);
                if age >= 1 {
                    warnings.push(format!("using cached advisory snapshot from {age} days ago"));
                }
                snapshot_age_days = Some(age);
                batch = Some(json);
            }
            None => {
                warnings.push("no cached advisory snapshot; vulnerable-dependency skipped".to_string());
                return Ok(Outcome { hits: Vec::new(), warnings, snapshot_age_days: None });
            }
        }
    }

    // The snapshot is keyed by the lockfile hash, so a cached response was
    // produced for this exact package list in this exact order and its results
    // line up with it positionally.
    let per_package = parse_batch(batch.as_deref().unwrap_or_default(), lock.packages.len());
    let ids: BTreeSet<String> = per_package.iter().flatten().cloned().collect();
    let mut details: BTreeMap<String, Value> = BTreeMap::new();
    let mut unavailable = 0usize;
    for id in ids {
        match vuln_detail(ix, &id, offline || network_down, fetch, now)? {
            Some(detail) => {
                details.insert(id, detail);
            }
            None => unavailable += 1,
        }
    }
    if unavailable > 0 {
        // The batch answer already establishes that the package is affected, so
        // the finding is still reported; only its wording is thinner.
        warnings.push(format!("advisory details unavailable for {unavailable} of the advisories found"));
    }

    let mut hits = Vec::new();
    for (package, ids) in lock.packages.iter().zip(per_package) {
        for id in ids {
            let advisory = match details.get(&id) {
                Some(detail) => advisory_from(&id, detail, &package.name, &package.version),
                None => Advisory {
                    id: id.clone(),
                    summary: String::new(),
                    severity: "UNKNOWN".to_string(),
                    fixed: None,
                    outside_every_range: false,
                    aliases: Vec::new(),
                },
            };
            hits.push(Hit { package: package.clone(), advisory });
        }
    }
    Ok(Outcome { hits, warnings, snapshot_age_days })
}

/// The one place in the engine that opens a socket.
///
/// `body` present means a JSON POST, absent means a GET.
///
/// One run makes one batch request and then one request per advisory found, so
/// the agent is built once and shared: a fresh agent per request would throw
/// away the connection pool and pay a TLS handshake for every advisory.
pub fn http_fetch(url: &str, body: Option<&str>) -> anyhow::Result<String> {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    let agent = AGENT.get_or_init(|| ureq::AgentBuilder::new().timeout(TIMEOUT).user_agent("locrin").build());
    let response = match body {
        Some(body) => agent.post(url).set("Content-Type", "application/json").send_string(body),
        None => agent.get(url).call(),
    }
    .with_context(|| format!("requesting {url}"))?;
    response.into_string().with_context(|| format!("reading the response from {url}"))
}

/// Queries every package in chunks and splices the chunks back into one response
/// so the snapshot is a single row shaped exactly like a one-chunk answer.
fn fetch_batch(
    lock: &Lockfile,
    fetch: &dyn Fn(&str, Option<&str>) -> anyhow::Result<String>,
) -> anyhow::Result<String> {
    let mut results: Vec<Value> = Vec::with_capacity(lock.packages.len());
    for chunk in lock.packages.chunks(CHUNK) {
        let queries: Vec<Value> = chunk
            .iter()
            .map(|p| json!({ "package": { "name": p.name, "ecosystem": "npm" }, "version": p.version }))
            .collect();
        let text = fetch(BATCH_URL, Some(&json!({ "queries": queries }).to_string()))?;
        let parsed: Value = serde_json::from_str(&text).context("parsing the OSV batch response")?;
        let answered = parsed.get("results").and_then(|r| r.as_array()).cloned().unwrap_or_default();
        // A response that does not answer every query cannot be lined up with the
        // package list, and guessing would attach an advisory to the wrong
        // package. Failing here falls back to the snapshot.
        anyhow::ensure!(
            answered.len() == chunk.len(),
            "the OSV batch response has {} results for {} queries",
            answered.len(),
            chunk.len()
        );
        results.extend(answered);
    }
    Ok(json!({ "results": results }).to_string())
}

/// The advisory ids for each package, in the package list's order.
fn parse_batch(json: &str, packages: usize) -> Vec<Vec<String>> {
    let mut out = vec![Vec::new(); packages];
    let Ok(parsed) = serde_json::from_str::<Value>(json) else { return out };
    let Some(results) = parsed.get("results").and_then(|r| r.as_array()) else { return out };
    for (slot, result) in out.iter_mut().zip(results) {
        let mut ids: Vec<String> = result
            .get("vulns")
            .and_then(|v| v.as_array())
            .map(|vulns| vulns.iter().filter_map(|v| v.get("id")).filter_map(Value::as_str).map(String::from).collect())
            .unwrap_or_default();
        ids.sort();
        ids.dedup();
        *slot = ids;
    }
    out
}

/// The advisory's detail document, refreshed when it is stale and the run may
/// use the network, and read from the snapshot otherwise.
fn vuln_detail(
    ix: &Index,
    id: &str,
    offline: bool,
    fetch: &dyn Fn(&str, Option<&str>) -> anyhow::Result<String>,
    now: u64,
) -> anyhow::Result<Option<Value>> {
    let cached = cached_vuln(ix, id)?;
    let stale = cached.as_ref().map(|(at, _)| age_in_days(now, *at) >= VULN_MAX_AGE_DAYS).unwrap_or(true);
    if !offline && stale && is_safe_id(id) {
        if let Ok(text) = fetch(&format!("{VULN_URL}{id}"), None) {
            if let Ok(detail) = serde_json::from_str::<Value>(&text) {
                store_vuln(ix, id, now, &text)?;
                return Ok(Some(detail));
            }
        }
    }
    Ok(cached.and_then(|(_, json)| serde_json::from_str::<Value>(&json).ok()))
}

/// Whether an id can be pasted into a URL path as it stands. Advisory ids are
/// `GHSA-...`, `CVE-...` and the like; anything else came from a response that
/// should not be steering a request.
///
/// At least one letter or digit is required, which is what makes `..` and `...`
/// fail: every character in them is on the permitted list, so the character
/// check alone let the one spelling of "the directory above" straight through.
fn is_safe_id(id: &str) -> bool {
    id.chars().any(|c| c.is_ascii_alphanumeric())
        && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

fn advisory_from(id: &str, detail: &Value, package: &str, version: &str) -> Advisory {
    let (fixed, holds) = fixed_for(detail, package, version);
    let severity = detail
        .get("database_specific")
        .and_then(|d| d.get("severity"))
        .and_then(Value::as_str)
        .map(|s| s.trim().to_ascii_uppercase())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "UNKNOWN".to_string());
    Advisory {
        id: id.to_string(),
        summary: detail.get("summary").and_then(Value::as_str).unwrap_or_default().to_string(),
        severity,
        fixed,
        outside_every_range: !holds,
        aliases: detail
            .get("aliases")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).map(String::from).collect())
            .unwrap_or_default(),
    }
}

/// The version the advisory says fixes the installed version of this package.
///
/// An advisory can carry ranges for several ecosystems and several packages, so
/// the entry has to be matched by name and by ecosystem: the same name lives in
/// npm and in Maven and in PyPI, with version numbers that have nothing to do
/// with each other, and everything this engine reads came out of an npm
/// lockfile. An entry that does not say which ecosystem it is for is not read as
/// npm; a finding then carries no fix, which is a weaker sentence, not a wrong
/// version.
///
/// Within the matching entry the range has to be the one the installed version
/// falls in, and that is the whole point of this function. A package that
/// maintains more than one branch gets one range per branch, each with its own
/// `fixed`, and the document lists them oldest first: `fast-uri` carries
/// `[{introduced: 0, fixed: 2.4.5}, {introduced: 3.0.0, fixed: 3.1.6}]`, so
/// reading the first `fixed` in the document told a repository on `3.1.5` to
/// install `2.4.5`. A security finding that names an older release is a wrong
/// instruction, whatever it is right about, so the range is chosen by the
/// version rather than by its position.
///
/// Every range is half open, `introduced <= v < fixed`, which is what OSV means
/// by the two events. A range whose `introduced` the version is at or past with
/// no `fixed` after it (or with a `last_affected` the version is at or before)
/// is the range that contains it and it names no fix: the answer is `None`, and
/// the advisory reads "No fixed version published". `None` is also the answer
/// when no range contains the version at all, which happens when the batch
/// endpoint and the detail document disagree about what is affected; the second
/// half of the return value separates the two, and a finding whose version no
/// range holds reads "No fixed version applies to this version" instead.
fn fixed_for(detail: &Value, package: &str, version: &str) -> (Option<String>, bool) {
    for affected in detail.get("affected").and_then(Value::as_array).into_iter().flatten() {
        let named = affected.get("package");
        let name = named.and_then(|p| p.get("name")).and_then(Value::as_str);
        let ecosystem = named.and_then(|p| p.get("ecosystem")).and_then(Value::as_str);
        if name != Some(package) || ecosystem != Some("npm") {
            continue;
        }
        for range in affected.get("ranges").and_then(Value::as_array).into_iter().flatten() {
            match containing_range(range, version) {
                // The range holding the installed version names the upgrade.
                Some(Some(fixed)) => return (Some(fixed), true),
                // It holds the version and names no upgrade. There is no
                // second opinion to look for: this is the branch the
                // repository is on.
                Some(None) => return (None, true),
                None => continue,
            }
        }
    }
    (None, false)
}

/// The upgrade half of [`fixed_for`]. The range tests read it, because the
/// version a range names is the whole of what they pin.
#[cfg(test)]
fn first_fixed(detail: &Value, package: &str, version: &str) -> Option<String> {
    fixed_for(detail, package, version).0
}

/// Whether a SEMVER range contains `version`, and the `fixed` it names if it
/// does: `Some(Some(fixed))` for a contained version with a fix,
/// `Some(None)` for a contained version with none, `None` for a range that does
/// not contain it.
///
/// Only `SEMVER` ranges are read. A `GIT` range names commits, and an
/// `ECOSYSTEM` range on npm carries the same version strings but is not
/// guaranteed to be ordered by them, so neither is a range this comparison can
/// answer.
fn containing_range(range: &Value, version: &str) -> Option<Option<String>> {
    if range.get("type").and_then(Value::as_str) != Some("SEMVER") {
        return None;
    }
    let mut introduced: Option<&str> = None;
    for event in range.get("events").and_then(Value::as_array).into_iter().flatten() {
        if let Some(at) = event.get("introduced").and_then(Value::as_str) {
            introduced = Some(at);
            continue;
        }
        // A half-open interval closed by its fix: `introduced <= v < fixed`.
        if let Some(fixed) = event.get("fixed").and_then(Value::as_str) {
            let opened = introduced.take();
            if opened.is_some_and(|at| version_cmp(version, at) != Ordering::Less)
                && version_cmp(version, fixed) == Ordering::Less
            {
                return Some(Some(fixed.to_string()));
            }
            continue;
        }
        // An interval closed by its last affected release instead, which is
        // what an advisory writes when no fix has shipped: `introduced <= v <=
        // last_affected`, and no version to upgrade to.
        if let Some(last) = event.get("last_affected").and_then(Value::as_str) {
            let opened = introduced.take();
            if opened.is_some_and(|at| version_cmp(version, at) != Ordering::Less)
                && version_cmp(version, last) != Ordering::Greater
            {
                return Some(None);
            }
        }
    }
    // An `introduced` with nothing closing it runs to the end of time.
    match introduced {
        Some(at) if version_cmp(version, at) != Ordering::Less => Some(None),
        _ => None,
    }
}

/// Orders two npm version strings.
///
/// This is the smallest comparison the range check needs and not a semver
/// implementation: `major.minor.patch` compared as numbers, a missing part read
/// as zero (`introduced: "0"` is how OSV spells the beginning of time), build
/// metadata dropped, and a prerelease of a version sorted before that version so
/// `3.1.6-rc.1` is inside a range fixed at `3.1.6`. Prereleases of the same
/// version are not ordered against each other, because no range boundary this
/// engine reads has ever needed it. Anything that does not parse falls back to a
/// string comparison, which is arbitrary but total: a range with an unreadable
/// boundary is not allowed to panic a run.
fn version_cmp(a: &str, b: &str) -> Ordering {
    match (parse_version(a), parse_version(b)) {
        (Some((a_parts, a_pre)), Some((b_parts, b_pre))) => a_parts.cmp(&b_parts).then_with(|| match (a_pre, b_pre) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            _ => Ordering::Equal,
        }),
        _ => a.cmp(b),
    }
}

/// A version as its three numbers and whether it carries a prerelease tag, or
/// `None` when it is not three numbers at all.
fn parse_version(v: &str) -> Option<([u64; 3], bool)> {
    let v = v.trim();
    let v = v.strip_prefix('v').unwrap_or(v);
    // Build metadata never orders anything.
    let v = v.split('+').next().unwrap_or(v);
    let (core, pre) = match v.split_once('-') {
        Some((core, rest)) => (core, !rest.is_empty()),
        None => (v, false),
    };
    let mut parts = core.split('.');
    let mut numbers = [0u64; 3];
    for slot in numbers.iter_mut() {
        match parts.next() {
            Some(part) => *slot = part.parse().ok()?,
            // A boundary written `3` or `3.1` means `3.0.0` and `3.1.0`.
            None => break,
        }
    }
    // A fourth part is not a version this comparison understands.
    if parts.next().is_some() {
        return None;
    }
    Some((numbers, pre))
}

fn store_batch(ix: &Index, lock_hash: &str, now: u64, json: &str) -> anyhow::Result<()> {
    ix.conn()
        .execute(
            "INSERT OR REPLACE INTO osv_batch(lock_hash, fetched_at, json) VALUES (?1, ?2, ?3)",
            params![lock_hash, now as i64, json],
        )
        .context("caching the OSV batch response")?;
    Ok(())
}

fn cached_batch(ix: &Index, lock_hash: &str) -> anyhow::Result<Option<(i64, String)>> {
    ix.conn()
        .query_row("SELECT fetched_at, json FROM osv_batch WHERE lock_hash = ?1", params![lock_hash], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .optional()
        .context("reading the cached OSV batch response")
}

fn store_vuln(ix: &Index, id: &str, now: u64, json: &str) -> anyhow::Result<()> {
    ix.conn()
        .execute(
            "INSERT OR REPLACE INTO osv_vulns(id, fetched_at, json) VALUES (?1, ?2, ?3)",
            params![id, now as i64, json],
        )
        .context("caching an OSV advisory")?;
    Ok(())
}

fn cached_vuln(ix: &Index, id: &str) -> anyhow::Result<Option<(i64, String)>> {
    ix.conn()
        .query_row("SELECT fetched_at, json FROM osv_vulns WHERE id = ?1", params![id], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .optional()
        .context("reading a cached OSV advisory")
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Whole days between a stored timestamp and now. A clock that has moved
/// backwards reads as age zero rather than as an enormous age.
fn age_in_days(now: u64, fetched_at: i64) -> u64 {
    now.saturating_sub(fetched_at.max(0) as u64) / DAY
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::lockfile;

    const VULN_ID: &str = "GHSA-p6mc-m468-83gg";

    const BATCH: &str =
        r#"{"results":[{},{},{},{"vulns":[{"id":"GHSA-p6mc-m468-83gg","modified":"2021-05-10T00:00:00Z"}]}]}"#;

    const DETAIL: &str = r#"{
      "id": "GHSA-p6mc-m468-83gg",
      "summary": "Prototype Pollution in lodash",
      "aliases": ["CVE-2020-8203"],
      "severity": [{"type": "CVSS_V3", "score": "CVSS:3.1/AV:N/AC:H/PR:N/UI:N/S:U/C:N/I:H/A:N"}],
      "database_specific": {"severity": "high"},
      "affected": [
        {
          "package": {"name": "left-pad", "ecosystem": "npm"},
          "ranges": [{"type": "SEMVER", "events": [{"introduced": "0"}, {"fixed": "9.9.9"}]}]
        },
        {
          "package": {"name": "lodash", "ecosystem": "npm"},
          "ranges": [{"type": "SEMVER", "events": [{"introduced": "0"}, {"fixed": "4.17.20"}]}]
        }
      ]
    }"#;

    /// Answers the two osv.dev endpoints from canned documents. No test in this
    /// module reaches the network.
    fn canned(url: &str, body: Option<&str>) -> anyhow::Result<String> {
        if url == BATCH_URL {
            let body = body.expect("the batch endpoint is a POST");
            assert!(body.contains(r#""name":"lodash""#), "every package is queried: {body}");
            assert!(body.contains(r#""ecosystem":"npm""#), "the ecosystem is npm: {body}");
            return Ok(BATCH.to_string());
        }
        if url == format!("{VULN_URL}{VULN_ID}") {
            assert!(body.is_none(), "the detail endpoint is a GET");
            return Ok(DETAIL.to_string());
        }
        anyhow::bail!("unexpected request to {url}")
    }

    fn refuse(_url: &str, _body: Option<&str>) -> anyhow::Result<String> {
        anyhow::bail!("connection refused")
    }

    fn fixture_lock() -> Lockfile {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/lockfiles");
        lockfile::read(&dir).unwrap().expect("the fixture directory has a lockfile")
    }

    fn backdate_batch(ix: &Index, days: u64) {
        ix.conn().execute("UPDATE osv_batch SET fetched_at = fetched_at - ?1", params![(days * DAY) as i64]).unwrap();
    }

    fn backdate_vuln(ix: &Index, days: u64) {
        ix.conn().execute("UPDATE osv_vulns SET fetched_at = fetched_at - ?1", params![(days * DAY) as i64]).unwrap();
    }

    fn only_hit(outcome: &Outcome) -> &Hit {
        assert_eq!(outcome.hits.len(), 1, "one advisory in the canned batch: {:?}", outcome.hits);
        &outcome.hits[0]
    }

    #[test]
    fn an_online_check_reports_the_advisory_and_writes_a_snapshot() {
        let ix = Index::open_in_memory().unwrap();
        let lock = fixture_lock();

        let outcome = check(&ix, &lock, false, &canned).unwrap();

        assert_eq!(outcome.warnings, Vec::<String>::new());
        assert_eq!(outcome.snapshot_age_days, Some(0));
        let hit = only_hit(&outcome);
        assert_eq!(hit.package.name, "lodash");
        assert_eq!(hit.package.version, "4.17.15");
        assert_eq!(hit.package.line, 32);
        assert_eq!(hit.advisory.id, VULN_ID);
        assert_eq!(hit.advisory.summary, "Prototype Pollution in lodash");
        assert_eq!(hit.advisory.severity, "HIGH");
        assert_eq!(hit.advisory.fixed.as_deref(), Some("4.17.20"), "the fix comes from the lodash entry");
        assert_eq!(hit.advisory.aliases, vec!["CVE-2020-8203"]);

        let batches: i64 = ix.conn().query_row("SELECT count(*) FROM osv_batch", [], |r| r.get(0)).unwrap();
        let vulns: i64 = ix.conn().query_row("SELECT count(*) FROM osv_vulns", [], |r| r.get(0)).unwrap();
        assert_eq!((batches, vulns), (1, 1), "both halves of the answer are cached");
    }

    #[test]
    fn an_offline_check_uses_the_snapshot_and_names_its_age() {
        let ix = Index::open_in_memory().unwrap();
        let lock = fixture_lock();
        check(&ix, &lock, false, &canned).unwrap();
        backdate_batch(&ix, 3);

        let outcome = check(&ix, &lock, true, &refuse).unwrap();

        assert_eq!(outcome.warnings, vec!["using cached advisory snapshot from 3 days ago"]);
        assert_eq!(outcome.snapshot_age_days, Some(3));
        assert_eq!(only_hit(&outcome).advisory.id, VULN_ID);
    }

    #[test]
    fn an_offline_check_without_a_snapshot_skips_the_rule() {
        let ix = Index::open_in_memory().unwrap();

        let outcome = check(&ix, &fixture_lock(), true, &refuse).unwrap();

        assert!(outcome.hits.is_empty());
        assert_eq!(outcome.warnings, vec!["no cached advisory snapshot; vulnerable-dependency skipped"]);
        assert_eq!(outcome.snapshot_age_days, None);
    }

    #[test]
    fn a_failed_request_falls_back_to_the_snapshot_rather_than_failing_the_run() {
        let ix = Index::open_in_memory().unwrap();
        let lock = fixture_lock();
        check(&ix, &lock, false, &canned).unwrap();
        // Old enough that the run wants to refresh it, which is what puts a
        // request in the way of the answer at all.
        backdate_batch(&ix, 2);

        let outcome = check(&ix, &lock, false, &refuse).unwrap();

        assert_eq!(outcome.warnings.len(), 2, "{:?}", outcome.warnings);
        assert!(outcome.warnings[0].starts_with("advisory lookup failed"), "{:?}", outcome.warnings);
        assert_eq!(outcome.warnings[1], "using cached advisory snapshot from 2 days ago");
        assert_eq!(outcome.snapshot_age_days, Some(2));
        assert_eq!(only_hit(&outcome).advisory.id, VULN_ID);
    }

    /// A failed batch request says the network is unreachable for this run, not
    /// for that one request. Every advisory detail behind it is stale by the
    /// same clock, so left alone the run paid the whole ten-second timeout once
    /// per advisory found and then served each of them from the snapshot
    /// anyway: a repository with a dozen advisories waited two minutes for the
    /// answer it already had.
    #[test]
    fn a_failed_batch_request_stops_the_run_asking_for_advisory_details() {
        let ix = Index::open_in_memory().unwrap();
        let lock = fixture_lock();
        check(&ix, &lock, false, &canned).unwrap();
        // Old enough that the run wants to refresh the batch, and old enough
        // that it would want to refresh the detail behind it too.
        backdate_batch(&ix, 2);
        backdate_vuln(&ix, VULN_MAX_AGE_DAYS + 1);

        let calls = std::cell::Cell::new(0usize);
        let counted = |url: &str, body: Option<&str>| -> anyhow::Result<String> {
            calls.set(calls.get() + 1);
            refuse(url, body)
        };

        let outcome = check(&ix, &lock, false, &counted).unwrap();

        assert_eq!(calls.get(), 1, "the batch request failed, so no detail was asked for");
        let hit = only_hit(&outcome);
        assert_eq!(hit.advisory.id, VULN_ID);
        assert_eq!(hit.advisory.summary, "Prototype Pollution in lodash", "the stale detail still answers");
        assert!(!outcome.warnings.iter().any(|w| w.contains("details unavailable")), "{:?}", outcome.warnings);
    }

    #[test]
    fn a_fresh_snapshot_is_used_without_an_age_warning() {
        let ix = Index::open_in_memory().unwrap();
        let lock = fixture_lock();
        check(&ix, &lock, false, &canned).unwrap();

        let outcome = check(&ix, &lock, true, &refuse).unwrap();

        assert_eq!(outcome.warnings, Vec::<String>::new());
        assert_eq!(outcome.snapshot_age_days, Some(0));
    }

    /// The warm run is the common one, and a snapshot from today answers it. An
    /// online check over a snapshot less than a day old asks the network
    /// nothing, so a repository whose lockfile has not moved costs no requests.
    #[test]
    fn an_online_check_over_a_snapshot_from_today_makes_no_request() {
        let ix = Index::open_in_memory().unwrap();
        let lock = fixture_lock();
        check(&ix, &lock, false, &canned).unwrap();
        let forbid = |url: &str, _body: Option<&str>| -> anyhow::Result<String> {
            panic!("a fresh snapshot must not be refreshed, but {url} was requested")
        };

        let outcome = check(&ix, &lock, false, &forbid).unwrap();

        assert_eq!(outcome.warnings, Vec::<String>::new());
        assert_eq!(outcome.snapshot_age_days, Some(0));
        assert_eq!(only_hit(&outcome).advisory.id, VULN_ID);
    }

    /// A day old is the age the run would start warning about, so it is also the
    /// age at which an online run refreshes rather than reuses.
    #[test]
    fn an_online_check_over_a_day_old_snapshot_refreshes_it() {
        let ix = Index::open_in_memory().unwrap();
        let lock = fixture_lock();
        check(&ix, &lock, false, &canned).unwrap();
        backdate_batch(&ix, 1);

        let outcome = check(&ix, &lock, false, &canned).unwrap();

        assert_eq!(outcome.warnings, Vec::<String>::new(), "the refreshed snapshot is today's");
        assert_eq!(outcome.snapshot_age_days, Some(0));
        let stored: i64 = ix.conn().query_row("SELECT fetched_at FROM osv_batch", [], |r| r.get(0)).unwrap();
        assert!(age_in_days(now_secs(), stored) == 0, "the row was rewritten with today's timestamp");
    }

    #[test]
    fn an_advisory_without_a_rating_is_unknown_and_a_foreign_package_has_no_fix() {
        let detail: Value = serde_json::from_str(DETAIL).unwrap();
        let bare = json!({"id": "GHSA-x", "affected": []});

        assert_eq!(advisory_from("GHSA-x", &bare, "lodash", "4.17.19").severity, "UNKNOWN");
        assert_eq!(advisory_from("GHSA-x", &bare, "lodash", "4.17.19").fixed, None);
        assert_eq!(advisory_from(VULN_ID, &detail, "left-pad", "1.3.0").fixed.as_deref(), Some("9.9.9"));
        assert_eq!(advisory_from(VULN_ID, &detail, "not-in-the-advisory", "1.0.0").fixed, None);
    }

    /// An advisory can carry the same name in several ecosystems, and only the
    /// npm entry's versions mean anything to a package this engine read out of
    /// an npm lockfile.
    #[test]
    fn a_fixed_version_comes_only_from_the_npm_entry() {
        let detail = json!({
          "id": "GHSA-y",
          "affected": [
            {
              "package": {"name": "lodash", "ecosystem": "Maven"},
              "ranges": [{"type": "ECOSYSTEM", "events": [{"introduced": "0"}, {"fixed": "1.0.0"}]}]
            },
            {
              "package": {"name": "lodash", "ecosystem": "npm"},
              "ranges": [{"type": "SEMVER", "events": [{"introduced": "0"}, {"fixed": "4.17.21"}]}]
            }
          ]
        });
        assert_eq!(advisory_from("GHSA-y", &detail, "lodash", "4.17.19").fixed.as_deref(), Some("4.17.21"));

        let unstated = json!({
          "id": "GHSA-z",
          "affected": [{
            "package": {"name": "lodash"},
            "ranges": [{"type": "SEMVER", "events": [{"fixed": "9.9.9"}]}]
          }]
        });
        assert_eq!(
            advisory_from("GHSA-z", &unstated, "lodash", "4.17.19").fixed,
            None,
            "an entry that does not say it is npm is not read as one"
        );
    }

    #[test]
    fn a_batch_response_that_does_not_answer_every_query_is_rejected() {
        let ix = Index::open_in_memory().unwrap();
        let lock = fixture_lock();
        let short = |url: &str, _body: Option<&str>| -> anyhow::Result<String> {
            assert_eq!(url, BATCH_URL);
            Ok(r#"{"results":[{}]}"#.to_string())
        };

        let outcome = check(&ix, &lock, false, &short).unwrap();

        assert!(outcome.hits.is_empty(), "a response that cannot be lined up is not guessed at");
        assert_eq!(outcome.warnings.len(), 2, "{:?}", outcome.warnings);
        assert!(outcome.warnings[0].contains("1 results for 4 queries"), "{:?}", outcome.warnings);
    }

    #[test]
    fn a_lockfile_with_no_packages_asks_nothing() {
        let ix = Index::open_in_memory().unwrap();
        let empty = Lockfile { rel: "yarn.lock".into(), hash: "abc".into(), packages: Vec::new() };

        let outcome = check(&ix, &empty, false, &refuse).unwrap();

        assert_eq!(outcome, Outcome { hits: Vec::new(), warnings: Vec::new(), snapshot_age_days: None });
    }

    #[test]
    fn an_id_that_is_not_url_safe_is_never_put_in_a_request() {
        assert!(is_safe_id("GHSA-p6mc-m468-83gg"));
        assert!(is_safe_id("CVE-2020-8203"));
        assert!(!is_safe_id(""));
        assert!(!is_safe_id("../../etc/passwd"));
        assert!(!is_safe_id("GHSA x"));
        // Every character here is on the permitted list, and the segment still
        // means "the directory above", so a letter or a digit is required.
        assert!(!is_safe_id(".."));
        assert!(!is_safe_id("..."));
        assert!(!is_safe_id("-_."));
    }

    /// A stored row that does not parse cannot answer, and reading it as an
    /// empty answer would report a repository with advisories as clean.
    #[test]
    fn an_unreadable_snapshot_row_is_treated_as_no_snapshot_at_all() {
        let ix = Index::open_in_memory().unwrap();
        let lock = fixture_lock();
        check(&ix, &lock, false, &canned).unwrap();
        ix.conn().execute("UPDATE osv_batch SET json = '{not json'", []).unwrap();

        let outcome = check(&ix, &lock, true, &refuse).unwrap();

        assert!(outcome.hits.is_empty(), "a row that cannot be read is not an answer");
        assert_eq!(
            outcome.warnings,
            vec![
                "the cached advisory snapshot is unreadable; ignoring it".to_string(),
                "no cached advisory snapshot; vulnerable-dependency skipped".to_string(),
            ]
        );
        assert_eq!(outcome.snapshot_age_days, None);

        // And an online run fetches over it rather than reusing it.
        let outcome = check(&ix, &lock, false, &canned).unwrap();
        assert_eq!(outcome.warnings, vec!["the cached advisory snapshot is unreadable; ignoring it".to_string()]);
        assert_eq!(only_hit(&outcome).advisory.id, VULN_ID);
    }

    /// Two findings that both name no fixed version are two different claims,
    /// and the rule says so. See [`Advisory::outside_every_range`].
    #[test]
    fn a_version_no_range_holds_is_told_apart_from_one_with_no_published_fix() {
        let uri = two_branches("fast-uri", "2.4.5", "3.1.6");
        assert!(advisory_from("GHSA-test", &uri, "fast-uri", "3.2.0").outside_every_range, "past the last fix");
        assert!(advisory_from("GHSA-test", &uri, "other", "1.0.0").outside_every_range, "another package entirely");
        assert!(!advisory_from("GHSA-test", &uri, "fast-uri", "3.1.5").outside_every_range, "the 3.x range holds it");

        // An `introduced` with nothing closing it holds the version and names
        // no fix, which is the advisory saying no fix has shipped.
        let open: Value = serde_json::from_str(
            r#"{"affected": [{"package": {"name": "p", "ecosystem": "npm"},
                 "ranges": [{"type": "SEMVER", "events": [{"introduced": "2.0.0"}]}]}]}"#,
        )
        .unwrap();
        let advisory = advisory_from("GHSA-test", &open, "p", "2.5.0");
        assert_eq!(advisory.fixed, None);
        assert!(!advisory.outside_every_range);
    }

    #[test]
    fn a_clock_that_moved_backwards_reads_as_no_age() {
        assert_eq!(age_in_days(1_000 * DAY, (1_002 * DAY) as i64), 0);
        assert_eq!(age_in_days(1_000 * DAY, (997 * DAY) as i64), 3);
        assert_eq!(age_in_days(1_000 * DAY, -5), 1_000);
    }

    /// An advisory for a package that maintains two branches, which is the
    /// shape the whole range check exists for.
    fn two_branches(package: &str, first: &str, second: &str) -> Value {
        serde_json::from_str(&format!(
            r#"{{
              "id": "GHSA-test",
              "affected": [
                {{
                  "package": {{"name": "{package}", "ecosystem": "npm"}},
                  "ranges": [
                    {{"type": "SEMVER", "events": [{{"introduced": "0"}}, {{"fixed": "{first}"}}]}},
                    {{"type": "SEMVER", "events": [{{"introduced": "3.0.0"}}, {{"fixed": "{second}"}}]}}
                  ]
                }}
              ]
            }}"#
        ))
        .unwrap()
    }

    /// The five downgrades Task 16 reproduced on `fasting-app`. Every one of
    /// them was the first `fixed` in the document rather than the fix for the
    /// branch the repository is on.
    #[test]
    fn the_fix_comes_from_the_range_the_installed_version_is_in() {
        let uri = two_branches("fast-uri", "2.4.5", "3.1.6");
        assert_eq!(first_fixed(&uri, "fast-uri", "3.1.5").as_deref(), Some("3.1.6"), "not the 2.x fix");
        assert_eq!(first_fixed(&uri, "fast-uri", "2.4.4").as_deref(), Some("2.4.5"), "the 2.x branch still answers");

        // `@xmldom/xmldom` carries three branches and the report saw it get all
        // three wrong in both directions.
        let xmldom: Value = serde_json::from_str(
            r#"{
              "affected": [
                {
                  "package": {"name": "@xmldom/xmldom", "ecosystem": "npm"},
                  "ranges": [
                    {"type": "SEMVER", "events": [{"introduced": "0"}, {"fixed": "0.7.14"}]},
                    {"type": "SEMVER", "events": [{"introduced": "0.8.0"}, {"fixed": "0.8.14"}]},
                    {"type": "SEMVER", "events": [{"introduced": "0.9.0"}, {"fixed": "0.9.12"}]}
                  ]
                }
              ]
            }"#,
        )
        .unwrap();
        assert_eq!(first_fixed(&xmldom, "@xmldom/xmldom", "0.9.10").as_deref(), Some("0.9.12"));
        assert_eq!(first_fixed(&xmldom, "@xmldom/xmldom", "0.8.13").as_deref(), Some("0.8.14"));
        assert_eq!(first_fixed(&xmldom, "@xmldom/xmldom", "0.7.13").as_deref(), Some("0.7.14"));

        let yaml = two_branches("js-yaml", "3.15.2", "4.3.2");
        assert_eq!(first_fixed(&yaml, "js-yaml", "3.15.1").as_deref(), Some("3.15.2"), "not the 4.x fix");
    }

    /// A version outside every range gets no fix rather than the nearest one,
    /// and so does one inside a range that names none.
    #[test]
    fn a_version_no_range_holds_names_no_fix() {
        let uri = two_branches("fast-uri", "2.4.5", "3.1.6");
        assert_eq!(first_fixed(&uri, "fast-uri", "3.2.0"), None, "past the last fix");
        assert_eq!(first_fixed(&uri, "fast-uri", "2.9.0"), None, "between the two branches");
        assert_eq!(first_fixed(&uri, "other", "1.0.0"), None, "another package entirely");

        // An `introduced` with nothing closing it: affected from here on, and
        // no release to move to.
        let open: Value = serde_json::from_str(
            r#"{"affected": [{"package": {"name": "p", "ecosystem": "npm"},
                 "ranges": [{"type": "SEMVER", "events": [{"introduced": "2.0.0"}]}]}]}"#,
        )
        .unwrap();
        assert_eq!(first_fixed(&open, "p", "2.5.0"), None);
        assert_eq!(first_fixed(&open, "p", "1.9.0"), None);

        // A `last_affected` closes the interval without naming a fix.
        let last: Value = serde_json::from_str(
            r#"{"affected": [{"package": {"name": "p", "ecosystem": "npm"},
                 "ranges": [{"type": "SEMVER", "events": [{"introduced": "1.0.0"}, {"last_affected": "1.4.0"}]},
                            {"type": "SEMVER", "events": [{"introduced": "2.0.0"}, {"fixed": "2.1.0"}]}]}]}"#,
        )
        .unwrap();
        assert_eq!(first_fixed(&last, "p", "1.3.0"), None);
        assert_eq!(first_fixed(&last, "p", "2.0.5").as_deref(), Some("2.1.0"));

        // The ecosystem gate is unchanged: a Maven entry is not read as npm.
        let maven: Value = serde_json::from_str(
            r#"{"affected": [{"package": {"name": "p", "ecosystem": "Maven"},
                 "ranges": [{"type": "SEMVER", "events": [{"introduced": "0"}, {"fixed": "9.9.9"}]}]}]}"#,
        )
        .unwrap();
        assert_eq!(first_fixed(&maven, "p", "1.0.0"), None);
    }

    /// The comparison the range check runs on, and the two places it gives up.
    #[test]
    fn versions_are_ordered_by_their_numbers_and_a_prerelease_sorts_first() {
        assert_eq!(version_cmp("3.1.5", "3.1.6"), Ordering::Less);
        assert_eq!(version_cmp("3.10.0", "3.9.0"), Ordering::Greater, "numbers, not text");
        assert_eq!(version_cmp("0.9.10", "0.9.9"), Ordering::Greater);
        assert_eq!(version_cmp("2.4.5", "2.4.5"), Ordering::Equal);
        assert_eq!(version_cmp("3.0.0", "0"), Ordering::Greater, "the beginning of time");
        assert_eq!(version_cmp("3.1", "3.1.0"), Ordering::Equal, "a missing part is a zero");
        assert_eq!(version_cmp("v3.1.6", "3.1.6"), Ordering::Equal);
        assert_eq!(version_cmp("3.1.6+build.7", "3.1.6"), Ordering::Equal, "build metadata orders nothing");
        assert_eq!(version_cmp("3.1.6-rc.1", "3.1.6"), Ordering::Less, "a prerelease comes before its release");
        assert_eq!(version_cmp("3.1.6", "3.1.6-rc.1"), Ordering::Greater);
        // A prerelease inside the range it precedes, which is what that rule is
        // for: `3.1.6-rc.1` is still affected by a bug fixed in `3.1.6`.
        let uri = two_branches("fast-uri", "2.4.5", "3.1.6");
        assert_eq!(first_fixed(&uri, "fast-uri", "3.1.6-rc.1").as_deref(), Some("3.1.6"));
        // The last resort, which is arbitrary but never panics.
        assert_eq!(version_cmp("not-a-version", "not-a-version"), Ordering::Equal);
        assert_eq!(version_cmp("1.2.3.4", "1.2.3"), "1.2.3.4".cmp("1.2.3"));
    }
}
