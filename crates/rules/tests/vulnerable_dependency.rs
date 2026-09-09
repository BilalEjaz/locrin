//! `vulnerable-dependency` over the three lockfile formats.
//!
//! No test here reaches the network. Every one that expects a finding seeds the
//! index's advisory snapshot first, through `osv::check` with a canned fetch,
//! which is what an online run would have written; the rule then runs offline
//! and answers from that snapshot. A test that seeds nothing is a repository
//! whose first ever run was offline, and it reports nothing.

mod common;

use std::path::Path;

use common::{fixture, run_on, run_on_seeded};
use locrin_core::finding::{Confidence, Finding, Severity};
use locrin_core::index::Index;
use locrin_core::previous::Previous;
use locrin_core::{lockfile, osv};
use locrin_rules::vulnerable_dependency::VulnerableDependency;

const VULN_ID: &str = "GHSA-p6mc-m468-83gg";

/// The advisory sits on the fourth package of every fixture's sorted list
/// (`@acme/util`, `b`, `left-pad`, `lodash`), so the batch answers four queries
/// and only the last one carries a vulnerability.
const BATCH: &str = r#"{"results":[{},{},{},{"vulns":[{"id":"GHSA-p6mc-m468-83gg"}]}]}"#;

fn detail(rating: &str, fixed: Option<&str>, summary: &str) -> String {
    let events = match fixed {
        Some(fixed) => format!(r#"[{{"introduced": "0"}}, {{"fixed": "{fixed}"}}]"#),
        None => r#"[{"introduced": "0"}]"#.to_string(),
    };
    format!(
        r#"{{
          "id": "{VULN_ID}",
          "summary": "{summary}",
          "database_specific": {{"severity": "{rating}"}},
          "affected": [{{
            "package": {{"name": "lodash", "ecosystem": "npm"}},
            "ranges": [{{"type": "SEMVER", "events": {events}}}]
          }}]
        }}"#
    )
}

/// Writes the snapshot an online run would have written, from canned documents.
fn seed<'a>(rating: &'a str, fixed: Option<&'a str>, summary: &'a str) -> impl FnOnce(&Index, &Path) + 'a {
    move |ix: &Index, root: &Path| {
        let detail = detail(rating, fixed, summary);
        let canned = |url: &str, body: Option<&str>| -> anyhow::Result<String> {
            if url == osv::BATCH_URL {
                assert!(body.is_some(), "the batch endpoint is a POST");
                return Ok(BATCH.to_string());
            }
            assert_eq!(url, format!("{}{VULN_ID}", osv::VULN_URL), "no other endpoint is asked");
            Ok(detail.clone())
        };
        let lock = lockfile::read(root).unwrap().expect("the fixture directory has a lockfile");
        let outcome = osv::check(ix, &lock, false, &canned).unwrap();
        assert_eq!(outcome.warnings, Vec::<String>::new(), "the seed itself must be clean");
        assert_eq!(outcome.hits.len(), 1, "one advisory to report");
    }
}

fn run(bucket: &str, rating: &str, fixed: Option<&str>, summary: &str) -> Vec<Finding> {
    let root = fixture("vulnerable_dependency", bucket);
    let config = locrin_core::config::Config::default();
    run_on_seeded(Box::new(VulnerableDependency), &root, &config, &Previous::default(), seed(rating, fixed, summary))
}

fn only(findings: &[Finding]) -> &Finding {
    assert_eq!(findings.len(), 1, "one advisory on one package: {findings:?}");
    &findings[0]
}

/// The same install described three ways is the same finding, at whichever line
/// that format happens to declare lodash on.
#[test]
fn every_lockfile_format_reports_the_advisory_at_its_own_line() {
    let cases = [("npm", "package-lock.json", 32), ("yarn", "yarn.lock", 17), ("pnpm", "pnpm-lock.yaml", 25)];
    for (bucket, rel, line) in cases {
        let findings = run(bucket, "moderate", Some("4.17.20"), "Prototype Pollution in lodash");
        let f = only(&findings);
        assert_eq!(f.file, rel, "{bucket}");
        assert_eq!((f.span.start_line, f.span.start_col, f.span.end_col), (line, 0, 0), "{bucket}");
        assert_eq!(f.rule, "vulnerable-dependency");
        assert_eq!(f.severity, Severity::Medium, "MODERATE is a Medium finding");
        assert_eq!(f.confidence, Confidence::Medium, "only a High advisory with a fix reads High");
        assert_eq!(
            f.evidence,
            format!("lodash 4.17.15: {VULN_ID} (MODERATE) Prototype Pollution in lodash"),
            "{bucket}"
        );
        assert_eq!(f.fix, "Upgrade lodash to 4.17.20");
        assert_eq!(f.owasp.as_deref(), Some("A06:2021"));
        assert_eq!(f.cwe.as_deref(), Some("CWE-1395"));
    }
}

/// The id is the package and the advisory, so it survives the lockfile being
/// regenerated and the entry moving to another line, and it is the same id
/// whichever package manager wrote the file.
#[test]
fn the_finding_id_follows_the_package_and_the_advisory_within_one_lockfile() {
    let npm = run("npm", "moderate", Some("4.17.20"), "Prototype Pollution in lodash");
    let same = run("npm", "moderate", Some("4.17.20"), "Prototype Pollution in lodash");
    assert_eq!(only(&npm).id, only(&same).id);
    assert_eq!(only(&npm).id.len(), 16);
}

#[test]
fn a_high_advisory_that_names_a_fix_is_high_severity_and_high_confidence() {
    let findings = run("npm", "high", Some("4.17.21"), "Command injection in lodash");
    let f = only(&findings);
    assert_eq!(f.severity, Severity::High);
    assert_eq!(f.confidence, Confidence::High, "a rated advisory with a version to move to is a full instruction");
    assert_eq!(f.evidence, format!("lodash 4.17.15: {VULN_ID} (HIGH) Command injection in lodash"));
    assert_eq!(f.fix, "Upgrade lodash to 4.17.21");
}

/// An advisory with no fixed version cannot be closed by upgrading, so the fix
/// says the two things that are left, and the confidence drops even though the
/// rating is High.
#[test]
fn an_advisory_without_a_fixed_version_says_to_pin_or_replace() {
    let findings = run("npm", "high", None, "Unpatched prototype pollution");
    let f = only(&findings);
    assert_eq!(f.severity, Severity::High);
    assert_eq!(f.confidence, Confidence::Medium);
    assert_eq!(f.fix, format!("No fixed version published; review {VULN_ID} and pin or replace the package"));
}

/// A long summary is a pointer to the advisory, not a copy of it.
#[test]
fn a_long_summary_is_cut_in_the_evidence() {
    let summary = "x".repeat(200);
    let findings = run("npm", "low", Some("4.17.20"), &summary);
    let f = only(&findings);
    assert_eq!(f.severity, Severity::Low, "LOW is a Low finding");
    let head = f.evidence.split_once("(LOW) ").expect("the evidence names the rating").1;
    assert_eq!(head.chars().count(), 120);
    assert!(head.ends_with("..."), "{}", f.evidence);
}

#[test]
fn a_repository_without_a_lockfile_has_nothing_to_report() {
    let root = fixture("vulnerable_dependency", "none");
    let findings = run_on(Box::new(VulnerableDependency), &root, &locrin_core::config::Config::default());
    assert!(findings.is_empty(), "no lockfile, no packages, no advisories: {findings:?}");
}

/// The first run of a repository that has never been online reports nothing and
/// says so on stderr, rather than failing the run or claiming the dependencies
/// are clean (spec 9).
#[test]
fn an_offline_run_without_a_snapshot_reports_nothing_and_warns() {
    let root = fixture("vulnerable_dependency", "npm");
    let config = locrin_core::config::Config::default();
    let findings = run_on_seeded(Box::new(VulnerableDependency), &root, &config, &Previous::default(), |_ix, _root| {});
    assert!(findings.is_empty(), "an unseeded offline run has nothing to answer from: {findings:?}");

    // The warning the rule printed, from the same call the rule makes.
    let ix = Index::open_in_memory().unwrap();
    let lock = lockfile::read(&root).unwrap().unwrap();
    let refuse = |_url: &str, _body: Option<&str>| -> anyhow::Result<String> { anyhow::bail!("offline") };
    let outcome = osv::check(&ix, &lock, true, &refuse).unwrap();
    assert_eq!(outcome.warnings, vec!["no cached advisory snapshot; vulnerable-dependency skipped"]);
}

/// The advisory decides the severity, and the config still gets the last word
/// when a repository sets one.
#[test]
fn a_configured_severity_overrides_every_advisorys_rating() {
    let root = fixture("vulnerable_dependency", "npm");
    let mut config = locrin_core::config::Config::default();
    config.rules.insert(
        "vulnerable-dependency".into(),
        locrin_core::config::RuleOverride { enabled: None, severity: Some(Severity::Low) },
    );
    let findings = run_on_seeded(
        Box::new(VulnerableDependency),
        &root,
        &config,
        &Previous::default(),
        seed("high", Some("4.17.21"), "Command injection in lodash"),
    );
    assert_eq!(only(&findings).severity, Severity::Low, "the config flattens the rating it disagrees with");
}
