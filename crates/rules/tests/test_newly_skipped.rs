mod common;

use std::collections::{HashMap, HashSet};

use common::{fixture, hits, run_on_with};
use locrin_core::config::Config;
use locrin_core::finding::{make_id, Category, Confidence, Severity};
use locrin_core::previous::Previous;
use locrin_rules::test_newly_skipped::TestNewlySkipped;

/// A previous state that says `rel` skipped exactly `names`.
fn previous(rel: &str, names: &[&str]) -> Previous {
    let set: HashSet<String> = names.iter().map(|n| n.to_string()).collect();
    Previous { skipped_tests: HashMap::from([(rel.to_string(), set)]) }
}

#[test]
fn with_no_known_previous_version_every_skipped_case_is_reported() {
    let out = run_on_with(
        Box::new(TestNewlySkipped),
        &fixture("test_newly_skipped", "flag"),
        &Config::default(),
        &Previous::default(),
    );
    assert_eq!(hits(&out), vec![("a.test.ts".into(), 6), ("a.test.ts".into(), 14)], "{out:?}");
    // No previous version was known, so the evidence does not claim the skip is
    // new: it says only what is true, that the case is skipped.
    assert_eq!(
        out.iter().map(|f| f.evidence.as_str()).collect::<Vec<_>>(),
        vec!["test \"mounts without throwing\" is skipped", "test \"keeps the row count\" is skipped"]
    );
    assert!(out.iter().all(|f| f.fix.starts_with("Re-enable the test or delete it")), "{:?}", out[0].fix);
    assert!(
        out.iter().all(|f| f.severity == Severity::Medium
            && f.confidence == Confidence::High
            && f.category == Category::Erosion),
        "{out:?}"
    );
    // The name is the anchor, so a skip keeps its id when the cases around it
    // move rather than being reported afresh on every edit above it.
    assert_eq!(
        out.iter().map(|f| f.id.as_str()).collect::<Vec<_>>(),
        vec![
            make_id("test-newly-skipped", "a.test.ts", "skip\x1fmounts without throwing"),
            make_id("test-newly-skipped", "a.test.ts", "skip\x1fkeeps the row count")
        ]
    );
}

#[test]
fn a_skip_the_previous_version_already_had_is_not_reported() {
    let out = run_on_with(
        Box::new(TestNewlySkipped),
        &fixture("test_newly_skipped", "flag"),
        &Config::default(),
        &previous("a.test.ts", &["mounts without throwing"]),
    );
    assert_eq!(hits(&out), vec![("a.test.ts".into(), 14)], "{out:?}");
    // A previous version was known, so the evidence says the skip is new.
    assert_eq!(out[0].evidence, "test \"keeps the row count\" is skipped (newly)");
}

/// An entry that lists every skip in the file silences the rule completely, which
/// is the steady state of a repository nobody is adding skips to.
#[test]
fn a_file_whose_every_skip_is_known_reports_nothing() {
    let out = run_on_with(
        Box::new(TestNewlySkipped),
        &fixture("test_newly_skipped", "flag"),
        &Config::default(),
        &previous("a.test.ts", &["mounts without throwing", "keeps the row count"]),
    );
    assert!(out.is_empty(), "{out:?}");
}

#[test]
fn active_tests_and_non_test_files_are_clean() {
    let out = run_on_with(
        Box::new(TestNewlySkipped),
        &fixture("test_newly_skipped", "clean"),
        &Config::default(),
        &Previous::default(),
    );
    assert!(out.is_empty(), "{out:?}");
}
