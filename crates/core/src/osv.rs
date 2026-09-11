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
//! A repository with several lockfiles is several checks with several keys: the
//! ecosystem is in the key, so an npm answer is never served for a PyPI list.

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
    /// The version to move to, or the reason there is not one to name.
    pub fix: Fix,
    pub aliases: Vec<String>,
}

/// What the advisory's own ranges said about the installed version.
///
/// Four findings can all carry no version to upgrade to and mean four different
/// things, and a reader acts on the difference: there is no release to move to,
/// or the advisory does not cover this version at all, or the engine never read
/// the ranges that would have named one. Collapsing them into one absent
/// `Option` made the rule assert the first of those whatever had happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fix {
    /// The affected range holding the installed version names the release that
    /// closes it.
    Named(String),
    /// A range holds the installed version and names no release after it, which
    /// is the advisory saying no fix has shipped for the branch the repository
    /// is on. Also the answer when the detail document was unavailable: nothing
    /// was read, so nothing was found to name.
    NonePublished,
    /// The detail document was read and no range of the entry for this package
    /// holds the installed version, which is the batch endpoint and the
    /// document disagreeing about what is affected.
    OutsideAllSemver,
    /// No range that holds the installed version was read, and at least one of
    /// the entry's ranges is in a form this engine does not order: a `GIT`
    /// range, an `ECOSYSTEM` range for a registry with no comparator here, or a
    /// range one of whose boundaries did not parse. The finding itself is not in
    /// doubt: the batch endpoint matched this exact version server side. Only
    /// the version to move to is missing, and saying that no fix applies would
    /// be the engine reporting a comparison it never made.
    UnreadableRanges,
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
        for id in one_per_family(&ids, &details) {
            let advisory = match details.get(&id) {
                Some(detail) => advisory_from(&id, detail, &package.name, &package.version, package.ecosystem),
                None => Advisory {
                    id: id.clone(),
                    summary: String::new(),
                    severity: "UNKNOWN".to_string(),
                    fix: Fix::NonePublished,
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
            .map(|p| json!({ "package": { "name": p.name, "ecosystem": p.ecosystem }, "version": p.version }))
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

/// One id per advisory family, out of the ids the batch endpoint returned for
/// one package.
///
/// OSV publishes the same advisory under several ids: a GHSA record, and a
/// `PYSEC-` (or `CVE-`) record that names it in its `aliases`, and the batch
/// endpoint returns both. Reported as they come, a package with one flaw is two
/// findings, and the copy is the poorer one: PyPI's PYSEC records rate
/// nothing and often carry no title. So ids that alias each other, in either
/// direction, are one family, and the family reports once under its GHSA id,
/// which is the record that carries the rating and the summary. A family with
/// no GHSA id reports under its first id in sort order. An alias that the
/// batch did not return for this package joins nothing: it is a name for the
/// same flaw, not a second finding, and there is nothing to merge it with.
fn one_per_family(ids: &[String], details: &BTreeMap<String, Value>) -> Vec<String> {
    let index: BTreeMap<&str, usize> = ids.iter().enumerate().map(|(i, id)| (id.as_str(), i)).collect();
    let mut parent: Vec<usize> = (0..ids.len()).collect();
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    for (i, id) in ids.iter().enumerate() {
        let aliases = details.get(id).and_then(|d| d.get("aliases")).and_then(Value::as_array);
        for alias in aliases.into_iter().flatten().filter_map(Value::as_str) {
            if let Some(&j) = index.get(alias) {
                let (a, b) = (root(&mut parent, i), root(&mut parent, j));
                parent[a] = b;
            }
        }
    }
    let mut families: BTreeMap<usize, Vec<&String>> = BTreeMap::new();
    for (i, id) in ids.iter().enumerate() {
        families.entry(root(&mut parent, i)).or_default().push(id);
    }
    let mut out: Vec<String> = families
        .into_values()
        .map(|mut family| {
            family.sort();
            family.iter().find(|id| id.starts_with("GHSA-")).unwrap_or(&family[0]).to_string()
        })
        .collect();
    out.sort();
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

fn advisory_from(id: &str, detail: &Value, package: &str, version: &str, ecosystem: &str) -> Advisory {
    let fix = fixed_for(detail, package, version, ecosystem);
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
        fix,
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
/// with each other, and `ecosystem` is the registry the package was actually
/// installed from. An entry that does not say which ecosystem it is for matches
/// none of them; a finding then carries no fix, which is a weaker sentence, not
/// a wrong version.
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
/// is the range that contains it and it names no fix: [`Fix::NonePublished`].
///
/// The two answers that name no version and are not that one are the point of
/// the enum. [`Fix::OutsideAllSemver`] is a document that was read and holds no
/// range covering this version, which is the batch endpoint and the detail
/// document disagreeing. [`Fix::UnreadableRanges`] is an entry with at least one
/// range this comparison does not order and none it does that holds the
/// version: nothing disagrees there, and nothing was read either.
fn fixed_for(detail: &Value, package: &str, version: &str, ecosystem: &str) -> Fix {
    let mut unordered = false;
    for affected in detail.get("affected").and_then(Value::as_array).into_iter().flatten() {
        let named = affected.get("package");
        let name = named.and_then(|p| p.get("name")).and_then(Value::as_str);
        let named_ecosystem = named.and_then(|p| p.get("ecosystem")).and_then(Value::as_str);
        if named_ecosystem != Some(ecosystem) || !name.is_some_and(|n| same_package(ecosystem, package, n)) {
            continue;
        }
        for range in affected.get("ranges").and_then(Value::as_array).into_iter().flatten() {
            match containing_range(range, version, ecosystem) {
                // The range holding the installed version names the upgrade.
                RangeSays::Holds(Some(fixed)) => return Fix::Named(fixed),
                // It holds the version and names no upgrade. There is no
                // second opinion to look for: this is the branch the
                // repository is on.
                RangeSays::Holds(None) => return Fix::NonePublished,
                RangeSays::Outside => continue,
                // Counted, not read. Taking its `fixed` event would be naming a
                // version chosen by its position in a list this engine cannot
                // order, which is the wrong-upgrade bug the range check exists
                // to prevent.
                RangeSays::Unreadable => {
                    unordered = true;
                    continue;
                }
            }
        }
    }
    if unordered {
        Fix::UnreadableRanges
    } else {
        Fix::OutsideAllSemver
    }
}

/// Whether an affected entry's package name is the installed package.
///
/// PyPI spells one project several ways and does not care which: `zope.interface`,
/// `Zope_Interface` and `zope-interface` are one distribution, and the lockfile
/// and the advisory are written by different people. PEP 503 is the registry's
/// own answer to that (lowercase, runs of `-`, `_` and `.` collapsed to one
/// `-`), so both sides are read through it and neither has to have been
/// normalised already. Every other ecosystem is compared literally: npm and
/// Packagist names are case sensitive and `.` is an ordinary character in them,
/// so collapsing it would match two different packages.
fn same_package(ecosystem: &str, installed: &str, named: &str) -> bool {
    if ecosystem == crate::lockfile::PYPI {
        pep503(installed) == pep503(named)
    } else {
        installed == named
    }
}

/// A PyPI project name in the form PEP 503 compares by.
fn pep503(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut previous_was_separator = false;
    for c in name.chars() {
        let separator = matches!(c, '-' | '_' | '.');
        if separator {
            if !previous_was_separator {
                out.push('-');
            }
        } else {
            out.extend(c.to_lowercase());
        }
        previous_was_separator = separator;
    }
    out
}

/// What one range of an advisory said about the installed version.
///
/// Three answers, because "the range does not hold this version" and "this
/// range was never compared against anything" are different facts and
/// [`fixed_for`] reports them as different findings.
#[derive(Debug, PartialEq, Eq)]
enum RangeSays {
    /// The range holds the version, and names this release as the fix, or names
    /// none at all.
    Holds(Option<String>),
    /// The range was read and does not hold the version.
    Outside,
    /// The range is in a form this engine does not order, or one of its
    /// boundaries is not a version the ecosystem's comparator reads.
    Unreadable,
}

/// How two versions of one ecosystem are ordered, or `None` when either of them
/// is not a version that comparator reads.
type Order = fn(&str, &str) -> Option<Ordering>;

/// The SEMVER comparator, in the shape the range reader takes. [`version_cmp`]
/// is total by construction (an unreadable boundary falls back to a string
/// comparison), so it never gives up.
fn semver_order(a: &str, b: &str) -> Option<Ordering> {
    Some(version_cmp(a, b))
}

/// The comparator for one range, or `None` when this engine has none for it.
///
/// A `SEMVER` range is ordered by semver whatever registry it came from, which
/// is what the type means. An `ECOSYSTEM` range carries the registry's own
/// version strings, so it is ordered by the registry's own rules and only for
/// the registries whose rules are implemented here: PyPI's PEP 440 and
/// Packagist's Composer ordering. npm keeps none, because npm advisories
/// publish `SEMVER` ranges and inventing a second reading of the same versions
/// could only make the two disagree. A `GIT` range names commits and orders
/// nothing.
fn range_order(range: &Value, ecosystem: &str) -> Option<Order> {
    match range.get("type").and_then(Value::as_str) {
        Some("SEMVER") => Some(semver_order),
        Some("ECOSYSTEM") => match ecosystem {
            crate::lockfile::PYPI => Some(pep440_cmp),
            crate::lockfile::PACKAGIST => Some(composer_cmp),
            _ => None,
        },
        _ => None,
    }
}

/// The upgrade half of [`fixed_for`]. The range tests read it, because the
/// version a range names is the whole of what they pin.
#[cfg(test)]
fn first_fixed(detail: &Value, package: &str, version: &str) -> Option<String> {
    match fixed_for(detail, package, version, "npm") {
        Fix::Named(fixed) => Some(fixed),
        _ => None,
    }
}

/// Whether a range contains `version`, and the `fixed` it names if it does.
///
/// The range is read with the comparator [`range_order`] picks for its type and
/// the package's ecosystem. A range with no comparator is
/// [`RangeSays::Unreadable`], and so is one whose boundaries that comparator
/// cannot read: a range is read whole or not at all, because a boundary nobody
/// can order is a comparison nobody can make, and answering from the other half
/// of the interval would be a guess. [`fixed_for`] turns that into
/// [`Fix::UnreadableRanges`] rather than into a version no range covers: such a
/// finding names the advisory and no upgrade, which is a thinner sentence
/// rather than a wrong one.
fn containing_range(range: &Value, version: &str, ecosystem: &str) -> RangeSays {
    let Some(order) = range_order(range, ecosystem) else {
        return RangeSays::Unreadable;
    };
    // Every boundary is compared against the one installed version, so an
    // unreadable one is either boundary or the version itself, and the answer
    // is the same in all three cases.
    let at_least = |boundary: &str| order(version, boundary).map(|o| o != Ordering::Less);
    let mut introduced: Option<&str> = None;
    for event in range.get("events").and_then(Value::as_array).into_iter().flatten() {
        if let Some(at) = event.get("introduced").and_then(Value::as_str) {
            introduced = Some(at);
            continue;
        }
        // A half-open interval closed by its fix: `introduced <= v < fixed`.
        if let Some(fixed) = event.get("fixed").and_then(Value::as_str) {
            let opened = introduced.take();
            let Some(after_start) = opened.map(at_least).unwrap_or(Some(false)) else {
                return RangeSays::Unreadable;
            };
            let Some(order_to_fix) = order(version, fixed) else {
                return RangeSays::Unreadable;
            };
            if after_start && order_to_fix == Ordering::Less {
                return RangeSays::Holds(Some(fixed.to_string()));
            }
            continue;
        }
        // An interval closed by its last affected release instead, which is
        // what an advisory writes when no fix has shipped: `introduced <= v <=
        // last_affected`, and no version to upgrade to.
        if let Some(last) = event.get("last_affected").and_then(Value::as_str) {
            let opened = introduced.take();
            let Some(after_start) = opened.map(at_least).unwrap_or(Some(false)) else {
                return RangeSays::Unreadable;
            };
            let Some(order_to_last) = order(version, last) else {
                return RangeSays::Unreadable;
            };
            if after_start && order_to_last != Ordering::Greater {
                return RangeSays::Holds(None);
            }
        }
    }
    // An `introduced` with nothing closing it runs to the end of time.
    match introduced.map(at_least) {
        Some(None) => RangeSays::Unreadable,
        Some(Some(true)) => RangeSays::Holds(None),
        _ => RangeSays::Outside,
    }
}

/// A PyPI version in the form PEP 440 compares by: the epoch, the release
/// segments with their trailing zeros dropped (`1.0` and `1.0.0` are one
/// version), and then the three suffixes and the local label, each in a key
/// whose derived ordering is the specification's own.
///
/// The fields are declared in comparison order, which is what `Ord` derives
/// from, so the whole comparison is `#[derive(Ord)]` over the parse.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Pep440 {
    epoch: u64,
    release: Vec<u64>,
    pre: Pep440Pre,
    /// A post release sorts after the version it follows, and `None` sorts
    /// before `Some` already.
    post: Option<u64>,
    dev: Pep440Dev,
    /// A local version sorts after the public version it labels, and `None`
    /// sorts before `Some` already.
    local: Option<Vec<LocalSegment>>,
}

/// Where a version sits against the pre-releases of the same release.
///
/// `dev < pre < final < post` is the specification's ordering, and the two ends
/// of it are not pre-release tags at all, so they are variants here: a bare dev
/// release (no pre, no post) precedes every pre-release of its version, and a
/// version with no pre-release at all follows every one of them.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Pep440Pre {
    DevOnly,
    Tag(PreTag, u64),
    Final,
}

/// `a < b < rc`, after the spellings PEP 440 normalises away (`alpha`, `beta`,
/// `c`, `pre`, `preview`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum PreTag {
    A,
    B,
    Rc,
}

/// A dev release precedes the version it is a dev release of, so the absent
/// case has to sort last and cannot be an `Option`.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Pep440Dev {
    At(u64),
    None,
}

/// A segment of a local version label. Numeric segments sort after
/// alphabetic ones, which is what the declaration order says.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
enum LocalSegment {
    Text(String),
    Number(u64),
}

/// Orders two PyPI versions by PEP 440, or `None` when either is not a version
/// that specification describes.
fn pep440_cmp(a: &str, b: &str) -> Option<Ordering> {
    Some(parse_pep440(a)?.cmp(&parse_pep440(b)?))
}

/// `[N!]N(.N)*[{a|b|rc}N][.postN][.devN][+local]`, with the separators and
/// spellings the specification permits and normalises.
fn parse_pep440(version: &str) -> Option<Pep440> {
    let lowered = version.trim().to_ascii_lowercase();
    let public = lowered.strip_prefix('v').unwrap_or(&lowered);
    let (public, local) = match public.split_once('+') {
        Some((public, label)) => (public, Some(parse_local(label)?)),
        None => (public, None),
    };
    let (epoch, rest) = match public.split_once('!') {
        Some((epoch, rest)) => (epoch.parse().ok()?, rest),
        None => (0, public),
    };
    let (release, rest) = parse_release(rest)?;
    let (pre, rest) = parse_pep440_pre(rest);
    let (post, rest) = parse_pep440_post(rest);
    let (dev, rest) = parse_pep440_dev(rest);
    // Anything left over is a version this comparator does not read, and a
    // version it does not read is never guessed at.
    if !rest.is_empty() {
        return None;
    }
    Some(Pep440 {
        epoch,
        release,
        pre: match (pre, post, &dev) {
            (Some((tag, n)), _, _) => Pep440Pre::Tag(tag, n),
            (None, None, Pep440Dev::At(_)) => Pep440Pre::DevOnly,
            (None, _, _) => Pep440Pre::Final,
        },
        post,
        dev,
        local,
    })
}

/// The dotted numbers a version opens with, and whatever follows them. Trailing
/// zeros are dropped, because `1.0` and `1.0.0` are one version.
fn parse_release(s: &str) -> Option<(Vec<u64>, &str)> {
    let mut release = Vec::new();
    let mut rest = s;
    loop {
        let (number, after) = take_number(rest);
        release.push(number?);
        rest = after;
        match rest.strip_prefix('.') {
            Some(next) if next.starts_with(|c: char| c.is_ascii_digit()) => rest = next,
            _ => break,
        }
    }
    while release.len() > 1 && release.last() == Some(&0) {
        release.pop();
    }
    Some((release, rest))
}

/// The leading digits of `s` as a number, and the rest of it. `None` when there
/// are no leading digits, or too many of them to hold.
fn take_number(s: &str) -> (Option<u64>, &str) {
    let end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    (s[..end].parse().ok(), &s[end..])
}

/// One optional `-`, `_` or `.` between a version and a suffix.
fn strip_separator(s: &str) -> &str {
    s.strip_prefix(['-', '_', '.']).unwrap_or(s)
}

/// The pre-release suffix, longest spelling first so `alpha` is not read as `a`
/// and `preview` is not read as `pre`. An absent number is a zero (`1.0a` is
/// `1.0a0`), and no suffix at all leaves `s` exactly as it was for the next
/// parser to try.
fn parse_pep440_pre(s: &str) -> (Option<(PreTag, u64)>, &str) {
    const TAGS: &[(&str, PreTag)] = &[
        ("alpha", PreTag::A),
        ("beta", PreTag::B),
        ("preview", PreTag::Rc),
        ("pre", PreTag::Rc),
        ("rc", PreTag::Rc),
        ("a", PreTag::A),
        ("b", PreTag::B),
        ("c", PreTag::Rc),
    ];
    let body = strip_separator(s);
    for (label, tag) in TAGS {
        if let Some(after) = body.strip_prefix(label) {
            let (number, rest) = take_number(strip_separator(after));
            return (Some((*tag, number.unwrap_or(0))), rest);
        }
    }
    (None, s)
}

/// The post-release suffix, in both the spelled forms (`post`, `rev`, `r`) and
/// the bare one PEP 440 keeps for compatibility: `1.0-1` is `1.0.post1`.
fn parse_pep440_post(s: &str) -> (Option<u64>, &str) {
    if let Some(after) = s.strip_prefix('-') {
        if let (Some(number), rest) = take_number(after) {
            return (Some(number), rest);
        }
    }
    let body = strip_separator(s);
    for label in ["post", "rev", "r"] {
        if let Some(after) = body.strip_prefix(label) {
            let (number, rest) = take_number(strip_separator(after));
            return (Some(number.unwrap_or(0)), rest);
        }
    }
    (None, s)
}

fn parse_pep440_dev(s: &str) -> (Pep440Dev, &str) {
    if let Some(after) = strip_separator(s).strip_prefix("dev") {
        let (number, rest) = take_number(strip_separator(after));
        return (Pep440Dev::At(number.unwrap_or(0)), rest);
    }
    (Pep440Dev::None, s)
}

/// A local version label: alphanumeric segments separated by `-`, `_` or `.`.
fn parse_local(label: &str) -> Option<Vec<LocalSegment>> {
    if label.is_empty() {
        return None;
    }
    label
        .split(['-', '_', '.'])
        .map(|segment| {
            if segment.is_empty() || !segment.chars().all(|c| c.is_ascii_alphanumeric()) {
                return None;
            }
            Some(match segment.parse() {
                Ok(number) => LocalSegment::Number(number),
                Err(_) => LocalSegment::Text(segment.to_string()),
            })
        })
        .collect()
}

/// A Packagist version in the form Composer's normaliser compares by: the
/// numeric part with its trailing zeros dropped, then the stability suffix and
/// its number.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct ComposerVersion {
    release: Vec<u64>,
    stability: Stability,
    number: u64,
}

/// Composer's stability ladder. `stable` is where a version with no suffix
/// sits, and a patch release (`-p1`, `-pl1`) sits above it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Stability {
    Dev,
    Alpha,
    Beta,
    Rc,
    Stable,
    Patch,
}

/// Orders two Packagist versions the way Composer does, or `None` when either
/// is not a release: a `dev-` branch name is a branch, and anything this
/// normaliser does not recognise is not guessed at.
fn composer_cmp(a: &str, b: &str) -> Option<Ordering> {
    Some(parse_composer(a)?.cmp(&parse_composer(b)?))
}

fn parse_composer(version: &str) -> Option<ComposerVersion> {
    let lowered = version.trim().to_ascii_lowercase();
    // `dev-main` names a branch rather than a release, and no ordering of it
    // against a release means anything.
    if lowered.starts_with("dev-") {
        return None;
    }
    let body = lowered.strip_prefix('v').unwrap_or(&lowered);
    let (numbers, suffix) = match body.split_once('-') {
        Some((numbers, suffix)) => (numbers, Some(suffix)),
        None => (body, None),
    };
    let mut release = Vec::new();
    for part in numbers.split('.') {
        if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        release.push(part.parse().ok()?);
    }
    // `1.0` and `1.0.0.0` are one version: Composer pads the numeric part to
    // four segments, which is the same comparison as dropping trailing zeros.
    while release.len() > 1 && release.last() == Some(&0) {
        release.pop();
    }
    let (stability, number) = match suffix {
        Some(suffix) => parse_stability(suffix)?,
        None => (Stability::Stable, 0),
    };
    Some(ComposerVersion { release, stability, number })
}

/// A stability suffix and its number, with Composer's own aliases (`a`, `b`,
/// `pl`, `p`) and the longest spelling tried first.
fn parse_stability(suffix: &str) -> Option<(Stability, u64)> {
    const LABELS: &[(&str, Stability)] = &[
        ("alpha", Stability::Alpha),
        ("beta", Stability::Beta),
        ("stable", Stability::Stable),
        ("patch", Stability::Patch),
        ("dev", Stability::Dev),
        ("rc", Stability::Rc),
        ("pl", Stability::Patch),
        ("a", Stability::Alpha),
        ("b", Stability::Beta),
        ("p", Stability::Patch),
    ];
    for (label, stability) in LABELS {
        let Some(after) = suffix.strip_prefix(label) else {
            continue;
        };
        let rest = strip_separator(after);
        if rest.is_empty() {
            return Some((*stability, 0));
        }
        return Some((*stability, rest.parse().ok()?));
    }
    None
}

/// Orders two version strings.
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
            assert!(body.contains(r#""ecosystem":"npm""#), "the fixture is an npm lockfile: {body}");
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
        lockfile::read(&dir).unwrap().into_iter().next().expect("the fixture directory has a lockfile")
    }

    fn package(name: &str, version: &str, ecosystem: &'static str) -> Package {
        Package { name: name.to_string(), version: version.to_string(), line: 1, ecosystem }
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
        assert_eq!(hit.advisory.fix, Fix::Named("4.17.20".to_string()), "the fix comes from the lodash entry");
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

        assert_eq!(advisory_from("GHSA-x", &bare, "lodash", "4.17.19", "npm").severity, "UNKNOWN");
        assert_eq!(advisory_from("GHSA-x", &bare, "lodash", "4.17.19", "npm").fix, Fix::OutsideAllSemver);
        assert_eq!(advisory_from(VULN_ID, &detail, "left-pad", "1.3.0", "npm").fix, Fix::Named("9.9.9".to_string()));
        assert_eq!(advisory_from(VULN_ID, &detail, "not-in-the-advisory", "1.0.0", "npm").fix, Fix::OutsideAllSemver);
    }

    /// An advisory can carry the same name in several ecosystems, and only the
    /// npm entry's versions mean anything to a package read out of an npm
    /// lockfile.
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
        assert_eq!(advisory_from("GHSA-y", &detail, "lodash", "4.17.19", "npm").fix, Fix::Named("4.17.21".to_string()));

        let unstated = json!({
          "id": "GHSA-z",
          "affected": [{
            "package": {"name": "lodash"},
            "ranges": [{"type": "SEMVER", "events": [{"fixed": "9.9.9"}]}]
          }]
        });
        assert_eq!(
            advisory_from("GHSA-z", &unstated, "lodash", "4.17.19", "npm").fix,
            Fix::OutsideAllSemver,
            "an entry that does not say it is npm is not read as one"
        );
    }

    /// `requests` is a PyPI package and an npm package, `monolog/monolog` is a
    /// Packagist one, and their version numbers have nothing to do with each
    /// other. The entry an advisory answers with is the one for the ecosystem
    /// the package was actually installed from, so a PyPI package never reads
    /// an npm range as its own.
    #[test]
    fn a_fix_comes_only_from_the_entry_for_the_packages_own_ecosystem() {
        let detail = json!({
          "id": "GHSA-eco",
          "affected": [
            {
              "package": {"name": "requests", "ecosystem": "npm"},
              "ranges": [{"type": "SEMVER", "events": [{"introduced": "0"}, {"fixed": "9.9.9"}]}]
            },
            {
              "package": {"name": "requests", "ecosystem": "PyPI"},
              "ranges": [{"type": "SEMVER", "events": [{"introduced": "0"}, {"fixed": "2.20.0"}]}]
            }
          ]
        });
        assert_eq!(
            advisory_from("GHSA-eco", &detail, "requests", "2.19.0", "PyPI").fix,
            Fix::Named("2.20.0".to_string())
        );
        assert_eq!(
            advisory_from("GHSA-eco", &detail, "requests", "2.19.0", "npm").fix,
            Fix::Named("9.9.9".to_string())
        );
        let elsewhere = advisory_from("GHSA-eco", &detail, "requests", "2.19.0", "Packagist");
        assert_eq!(elsewhere.fix, Fix::OutsideAllSemver, "no Packagist entry, so no version to name");
    }

    /// A range in a form nothing here orders was never compared against the
    /// installed version. Reporting that as "no range covers this version"
    /// would be the engine claiming a comparison it never made, so it is its
    /// own answer. A `GIT` range names commits; an `ECOSYSTEM` range is read
    /// only for the registries whose ordering this engine implements, which
    /// npm is not.
    #[test]
    fn an_entry_whose_ranges_are_not_ordered_says_the_fix_was_never_read() {
        let git_only = json!({
          "id": "GHSA-git",
          "affected": [{
            "package": {"name": "monolog/monolog", "ecosystem": "Packagist"},
            "ranges": [{"type": "GIT", "repo": "https://example.test/m", "events": [{"introduced": "0"}]}]
          }]
        });
        assert_eq!(
            advisory_from("GHSA-git", &git_only, "monolog/monolog", "2.0.0", "Packagist").fix,
            Fix::UnreadableRanges
        );

        let npm_ecosystem = json!({
          "id": "GHSA-eco-only",
          "affected": [{
            "package": {"name": "lodash", "ecosystem": "npm"},
            "ranges": [{"type": "ECOSYSTEM", "events": [{"introduced": "0"}, {"fixed": "4.17.21"}]}]
          }]
        });
        let advisory = advisory_from("GHSA-eco-only", &npm_ecosystem, "lodash", "4.17.19", "npm");
        assert_eq!(advisory.fix, Fix::UnreadableRanges, "the ECOSYSTEM fixed event is not taken");

        // A SEMVER range beside them still answers, and still wins.
        let both = json!({
          "id": "GHSA-both",
          "affected": [{
            "package": {"name": "lodash", "ecosystem": "npm"},
            "ranges": [
              {"type": "ECOSYSTEM", "events": [{"introduced": "0"}, {"fixed": "9.9.9"}]},
              {"type": "SEMVER", "events": [{"introduced": "0"}, {"fixed": "4.17.21"}]}
            ]
          }]
        });
        assert_eq!(
            advisory_from("GHSA-both", &both, "lodash", "4.17.19", "npm").fix,
            Fix::Named("4.17.21".to_string())
        );

        // And a document whose SEMVER ranges simply do not hold the version is
        // still the two endpoints disagreeing, which is a different sentence.
        let uri = two_branches("fast-uri", "2.4.5", "3.1.6");
        assert_eq!(advisory_from("GHSA-test", &uri, "fast-uri", "3.2.0", "npm").fix, Fix::OutsideAllSemver);
    }

    /// PyPI spells one project several ways: `zope.interface`, `Zope_Interface`
    /// and `zope-interface` are one package, and the lockfile and the advisory
    /// need not agree on which spelling. PEP 503 is what the registry itself
    /// uses to decide, so both sides are read through it.
    #[test]
    fn a_pypi_entry_matches_whatever_spelling_of_the_name_the_advisory_uses() {
        let detail = json!({
          "id": "GHSA-pep",
          "affected": [{
            "package": {"name": "zope-interface", "ecosystem": "PyPI"},
            "ranges": [{"type": "SEMVER", "events": [{"introduced": "0"}, {"fixed": "5.4.0"}]}]
          }]
        });
        for spelling in ["zope-interface", "zope.interface", "Zope_Interface", "ZOPE...INTERFACE"] {
            assert_eq!(
                advisory_from("GHSA-pep", &detail, spelling, "5.0.0", "PyPI").fix,
                Fix::Named("5.4.0".to_string()),
                "{spelling}"
            );
        }

        // Only PyPI collapses those characters. An npm name is a name.
        let npm = json!({
          "id": "GHSA-npm",
          "affected": [{
            "package": {"name": "foo-bar", "ecosystem": "npm"},
            "ranges": [{"type": "SEMVER", "events": [{"introduced": "0"}, {"fixed": "2.0.0"}]}]
          }]
        });
        assert_eq!(advisory_from("GHSA-npm", &npm, "foo.bar", "1.0.0", "npm").fix, Fix::OutsideAllSemver);
        assert_eq!(advisory_from("GHSA-npm", &npm, "foo-bar", "1.0.0", "npm").fix, Fix::Named("2.0.0".to_string()));
    }

    /// Every package is queried under the ecosystem it was installed from, so
    /// one run over a repository with a `composer.lock` and a `requirements.txt`
    /// asks osv.dev two different questions.
    #[test]
    fn the_batch_query_names_each_packages_own_ecosystem() {
        let ix = Index::open_in_memory().unwrap();
        let lock = Lockfile {
            rel: "requirements.txt".into(),
            hash: "pypi-list".into(),
            packages: vec![package("urllib3", "1.26.4", "PyPI"), package("monolog/monolog", "2.0.0", "Packagist")],
        };
        let body = std::cell::RefCell::new(String::new());
        let capture = |url: &str, sent: Option<&str>| -> anyhow::Result<String> {
            assert_eq!(url, BATCH_URL);
            *body.borrow_mut() = sent.expect("the batch endpoint is a POST").to_string();
            Ok(r#"{"results":[{},{}]}"#.to_string())
        };

        check(&ix, &lock, false, &capture).unwrap();

        let sent = body.borrow();
        assert!(sent.contains(r#"{"ecosystem":"PyPI","name":"urllib3"}"#), "{sent}");
        assert!(sent.contains(r#"{"ecosystem":"Packagist","name":"monolog/monolog"}"#), "{sent}");
        assert!(!sent.contains(r#""ecosystem":"npm""#), "nothing here came out of an npm lockfile: {sent}");
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

    /// Findings that all name no fixed version are not all the same claim, and
    /// the rule says which one it is. See [`Fix`].
    #[test]
    fn a_version_no_range_holds_is_told_apart_from_one_with_no_published_fix() {
        let uri = two_branches("fast-uri", "2.4.5", "3.1.6");
        assert_eq!(
            advisory_from("GHSA-test", &uri, "fast-uri", "3.2.0", "npm").fix,
            Fix::OutsideAllSemver,
            "past the last fix"
        );
        assert_eq!(
            advisory_from("GHSA-test", &uri, "other", "1.0.0", "npm").fix,
            Fix::OutsideAllSemver,
            "another package entirely"
        );
        assert_eq!(
            advisory_from("GHSA-test", &uri, "fast-uri", "3.1.5", "npm").fix,
            Fix::Named("3.1.6".to_string()),
            "the 3.x range holds it"
        );

        // An `introduced` with nothing closing it holds the version and names
        // no fix, which is the advisory saying no fix has shipped.
        let open: Value = serde_json::from_str(
            r#"{"affected": [{"package": {"name": "p", "ecosystem": "npm"},
                 "ranges": [{"type": "SEMVER", "events": [{"introduced": "2.0.0"}]}]}]}"#,
        )
        .unwrap();
        let advisory = advisory_from("GHSA-test", &open, "p", "2.5.0", "npm");
        assert_eq!(advisory.fix, Fix::NonePublished);
    }

    /// The PyPI shape: the batch names the GHSA record and the PYSEC record
    /// that aliases it, and the package has one flaw, so it is one finding
    /// under the GHSA id, which is the record that rates it.
    #[test]
    fn two_aliased_advisories_are_one_finding_under_the_ghsa_id() {
        const PYSEC: &str = "PYSEC-2021-1";
        let both = r#"{"results":[{},{},{},{"vulns":[{"id":"PYSEC-2021-1"},{"id":"GHSA-p6mc-m468-83gg"}]}]}"#;
        let fetch = |url: &str, body: Option<&str>| -> anyhow::Result<String> {
            if url == BATCH_URL {
                return Ok(both.to_string());
            }
            if url == format!("{VULN_URL}{PYSEC}") {
                return Ok(json!({"id": PYSEC, "aliases": [VULN_ID], "affected": []}).to_string());
            }
            canned(url, body)
        };
        let ix = Index::open_in_memory().unwrap();

        let outcome = check(&ix, &fixture_lock(), false, &fetch).unwrap();

        let hit = only_hit(&outcome);
        assert_eq!(hit.advisory.id, VULN_ID);
        assert_eq!(hit.advisory.severity, "HIGH", "the family reports with the GHSA record's rating");
        assert_eq!(outcome.warnings, Vec::<String>::new());
    }

    /// The alias can be published on either record, a family can hold three
    /// ids, an id whose detail is unavailable still joins through the other
    /// side, and two advisories that name each other nowhere stay two.
    #[test]
    fn a_family_is_read_from_either_side_and_unrelated_advisories_stay_apart() {
        let ids: Vec<String> =
            ["PYSEC-2026-1", "GHSA-aaaa-bbbb-cccc", "CVE-2026-1", "GHSA-dddd-eeee-ffff", "PYSEC-2026-2"]
                .into_iter()
                .map(String::from)
                .collect();
        let mut details = BTreeMap::new();
        // The GHSA names the PYSEC; the PYSEC's own detail was never read.
        details.insert("GHSA-aaaa-bbbb-cccc".to_string(), json!({"aliases": ["PYSEC-2026-1", "CVE-2026-1"]}));
        details.insert("GHSA-dddd-eeee-ffff".to_string(), json!({"aliases": ["CVE-2026-9"]}));
        details.insert("PYSEC-2026-2".to_string(), json!({}));

        assert_eq!(one_per_family(&ids, &details), vec!["GHSA-aaaa-bbbb-cccc", "GHSA-dddd-eeee-ffff", "PYSEC-2026-2"]);
    }

    /// A family with no GHSA record reports under its first id in sort order,
    /// so the choice is stable across runs and the anchor does not move.
    #[test]
    fn a_family_without_a_ghsa_id_reports_under_its_first_id() {
        let ids: Vec<String> = ["PYSEC-2026-7", "CVE-2026-7"].into_iter().map(String::from).collect();
        let mut details = BTreeMap::new();
        details.insert("PYSEC-2026-7".to_string(), json!({"aliases": ["CVE-2026-7"]}));

        assert_eq!(one_per_family(&ids, &details), vec!["CVE-2026-7"]);
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

    /// PEP 440 is what PyPI orders by, and an `ECOSYSTEM` range from a PyPI
    /// advisory carries versions written in it. Every row here is a rule of
    /// that specification the range check leans on.
    #[test]
    fn pypi_versions_are_ordered_by_pep_440() {
        let cases: &[(&str, &str, Ordering)] = &[
            ("1.0", "1.0.0", Ordering::Equal),
            ("1.0", "1.0.0.0", Ordering::Equal),
            ("1.0a1", "1.0b1", Ordering::Less),
            ("1.0b1", "1.0rc1", Ordering::Less),
            ("1.0rc1", "1.0", Ordering::Less),
            ("1.0", "1.0.post1", Ordering::Less),
            ("1.0.dev1", "1.0a1", Ordering::Less),
            ("1.0.dev1", "1.0.dev2", Ordering::Less),
            ("1.0.post1.dev1", "1.0.post1", Ordering::Less),
            ("2!1.0", "1.9", Ordering::Greater),
            ("1.0+local", "1.0", Ordering::Greater),
            ("1.0-1", "1.0", Ordering::Greater),
            ("1.0alpha1", "1.0a1", Ordering::Equal),
            ("1.0-beta.2", "1.0b2", Ordering::Equal),
            ("1.0c1", "1.0rc1", Ordering::Equal),
            ("1.0a", "1.0a0", Ordering::Equal),
            ("v1.0", "1.0", Ordering::Equal),
            ("1.26.5", "1.26.4", Ordering::Greater),
            ("2.19.0", "0", Ordering::Greater),
            ("1.10", "1.9", Ordering::Greater),
        ];
        for (a, b, want) in cases {
            assert_eq!(pep440_cmp(a, b), Some(*want), "{a} vs {b}");
            assert_eq!(pep440_cmp(b, a), Some(want.reverse()), "{b} vs {a}");
        }
        // Never guessed at: a version this comparator does not read says so,
        // and the range holding it is reported as one that was not compared.
        for unreadable in ["1.0.x", "", "latest", "1.2.3+", "1!!2"] {
            assert_eq!(pep440_cmp(unreadable, "1.0"), None, "{unreadable}");
            assert_eq!(pep440_cmp("1.0", unreadable), None, "{unreadable}");
        }
    }

    /// Composer's own normaliser is what Packagist orders by: a `v` prefix
    /// means nothing, a missing numeric segment is a zero, and the stability
    /// suffix runs dev, alpha, beta, RC, stable, patch.
    #[test]
    fn composer_versions_are_ordered_by_their_stability_suffixes() {
        let cases: &[(&str, &str, Ordering)] = &[
            ("v1.2.3", "1.2.3", Ordering::Equal),
            ("1.0", "1.0.0.0", Ordering::Equal),
            ("1.0.0-alpha1", "1.0.0-beta1", Ordering::Less),
            ("1.0.0-beta1", "1.0.0-RC1", Ordering::Less),
            ("1.0.0-RC1", "1.0.0", Ordering::Less),
            ("1.0.0", "1.0.0-p1", Ordering::Less),
            ("1.0.0-dev", "1.0.0-alpha1", Ordering::Less),
            ("1.0.0-a1", "1.0.0-alpha1", Ordering::Equal),
            ("1.0.0-b2", "1.0.0-beta.2", Ordering::Equal),
            ("1.0.0-pl1", "1.0.0-p1", Ordering::Equal),
            ("2.0.0", "1.9.9", Ordering::Greater),
            ("1.10.0", "1.9.0", Ordering::Greater),
            ("2.0.0", "0", Ordering::Greater),
        ];
        for (a, b, want) in cases {
            assert_eq!(composer_cmp(a, b), Some(*want), "{a} vs {b}");
            assert_eq!(composer_cmp(b, a), Some(want.reverse()), "{b} vs {a}");
        }
        // A branch name is not a release, and nothing else here is a version.
        for unreadable in ["dev-main", "dev-feature/x", "1.0.x-dev", "", "1.0.0-frog", "v"] {
            assert_eq!(composer_cmp(unreadable, "1.0.0"), None, "{unreadable}");
            assert_eq!(composer_cmp("1.0.0", unreadable), None, "{unreadable}");
        }
    }

    /// The limit this lifts: PyPI publishes `ECOSYSTEM` ranges, which the
    /// engine used to count rather than read, so every PyPI finding named the
    /// advisory and no version to move to.
    #[test]
    fn a_pypi_ecosystem_range_names_the_version_to_move_to() {
        let detail = json!({
          "id": "GHSA-pypi",
          "affected": [{
            "package": {"name": "requests", "ecosystem": "PyPI"},
            "ranges": [{"type": "ECOSYSTEM", "events": [{"introduced": "0"}, {"fixed": "2.31.0"}]}]
          }]
        });
        assert_eq!(
            advisory_from("GHSA-pypi", &detail, "requests", "2.19.0", "PyPI").fix,
            Fix::Named("2.31.0".to_string())
        );
        // The upper bound is exclusive, so the fixed release itself is outside
        // the range and the document and the batch endpoint disagree.
        assert_eq!(advisory_from("GHSA-pypi", &detail, "requests", "2.31.0", "PyPI").fix, Fix::OutsideAllSemver);
        // A boundary the comparator cannot read leaves the whole range
        // uncompared rather than guessed at.
        let unreadable = json!({
          "id": "GHSA-pypi-bad",
          "affected": [{
            "package": {"name": "requests", "ecosystem": "PyPI"},
            "ranges": [{"type": "ECOSYSTEM", "events": [{"introduced": "0"}, {"fixed": "2.31.x"}]}]
          }]
        });
        assert_eq!(
            advisory_from("GHSA-pypi-bad", &unreadable, "requests", "2.19.0", "PyPI").fix,
            Fix::UnreadableRanges
        );
        // And the ecosystem still decides: npm has no comparator of its own
        // here, so an npm `ECOSYSTEM` range is as unread as it ever was.
        let npm = json!({
          "id": "GHSA-npm-eco",
          "affected": [{
            "package": {"name": "lodash", "ecosystem": "npm"},
            "ranges": [{"type": "ECOSYSTEM", "events": [{"introduced": "0"}, {"fixed": "4.17.21"}]}]
          }]
        });
        assert_eq!(advisory_from("GHSA-npm-eco", &npm, "lodash", "4.17.19", "npm").fix, Fix::UnreadableRanges);
    }

    /// The same for Packagist, whose advisories publish `ECOSYSTEM` ranges too.
    #[test]
    fn a_packagist_ecosystem_range_names_the_version_to_move_to() {
        let detail = json!({
          "id": "GHSA-composer",
          "affected": [{
            "package": {"name": "monolog/monolog", "ecosystem": "Packagist"},
            "ranges": [{"type": "ECOSYSTEM", "events": [{"introduced": "0"}, {"fixed": "2.0.0"}]}]
          }]
        });
        assert_eq!(
            advisory_from("GHSA-composer", &detail, "monolog/monolog", "1.25.0", "Packagist").fix,
            Fix::Named("2.0.0".to_string())
        );
        assert_eq!(
            advisory_from("GHSA-composer", &detail, "monolog/monolog", "2.0.0", "Packagist").fix,
            Fix::OutsideAllSemver
        );
        // `last_affected` closes an interval without naming a fix, and it is
        // inclusive where `fixed` is exclusive.
        let last = json!({
          "id": "GHSA-composer-last",
          "affected": [{
            "package": {"name": "monolog/monolog", "ecosystem": "Packagist"},
            "ranges": [{"type": "ECOSYSTEM", "events": [{"introduced": "1.0.0"}, {"last_affected": "1.25.0"}]}]
          }]
        });
        assert_eq!(
            advisory_from("GHSA-composer-last", &last, "monolog/monolog", "1.25.0", "Packagist").fix,
            Fix::NonePublished
        );
        assert_eq!(
            advisory_from("GHSA-composer-last", &last, "monolog/monolog", "1.26.0", "Packagist").fix,
            Fix::OutsideAllSemver
        );
    }

    /// A range this engine cannot order beside one it can: the readable range
    /// holds the version, so the finding names a fix rather than reporting that
    /// nothing was read. [`Fix::UnreadableRanges`] is what is left when no
    /// readable range holds it.
    #[test]
    fn a_readable_range_beside_an_unreadable_one_still_names_the_fix() {
        let mixed = json!({
          "id": "GHSA-mixed",
          "affected": [{
            "package": {"name": "monolog/monolog", "ecosystem": "Packagist"},
            "ranges": [
              {"type": "GIT", "repo": "https://example.test/m", "events": [{"introduced": "0"}]},
              {"type": "ECOSYSTEM", "events": [{"introduced": "0"}, {"fixed": "2.0.0"}]}
            ]
          }]
        });
        assert_eq!(
            advisory_from("GHSA-mixed", &mixed, "monolog/monolog", "1.25.0", "Packagist").fix,
            Fix::Named("2.0.0".to_string())
        );
        // Outside the readable range, the unreadable one is all that is left,
        // and it is a range this run never compared anything against.
        assert_eq!(
            advisory_from("GHSA-mixed", &mixed, "monolog/monolog", "2.1.0", "Packagist").fix,
            Fix::UnreadableRanges
        );
    }
}
