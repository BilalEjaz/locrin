mod common;

use common::{fixture, hits, run_on};
use locrin_core::config::Config;
use locrin_core::finding::{make_id, Category, Confidence, Severity};
use locrin_rules::test_no_assert::TestNoAssert;

#[test]
fn flags_cases_that_run_code_without_asserting_anything() {
    let out = run_on(Box::new(TestNoAssert), &fixture("test_no_assert", "flag"), &Config::default());
    assert_eq!(hits(&out), vec![("a.test.ts".into(), 10), ("a.test.ts".into(), 15)], "{out:?}");
    assert_eq!(
        out.iter().map(|f| f.evidence.as_str()).collect::<Vec<_>>(),
        vec!["test \"mounts without throwing\" has no assertion", "test \"renders a row\" has no assertion"]
    );
    assert!(out.iter().all(|f| f.fix.starts_with("Assert on the outcome")), "{:?}", out[0].fix);
    assert!(
        out.iter().all(|f| f.severity == Severity::Low
            && f.confidence == Confidence::Medium
            && f.category == Category::Erosion),
        "{out:?}"
    );
    // The name is the anchor, so a case keeps its id when the cases around it
    // move rather than being reported afresh on every edit above it.
    assert_eq!(
        out.iter().map(|f| f.id.as_str()).collect::<Vec<_>>(),
        vec![
            make_id("test-no-assert", "a.test.ts", "case\x1fmounts without throwing"),
            make_id("test-no-assert", "a.test.ts", "case\x1frenders a row")
        ]
    );
}

#[test]
fn helper_assertions_every_assertion_dialect_skips_and_non_test_files_are_clean() {
    let out = run_on(Box::new(TestNoAssert), &fixture("test_no_assert", "clean"), &Config::default());
    assert!(out.is_empty(), "{out:?}");
}

#[test]
fn one_helper_deep_and_an_options_object_are_seen_but_two_helpers_deep_is_not() {
    let out = run_on(Box::new(TestNoAssert), &fixture("test_no_assert", "edge"), &Config::default());
    assert_eq!(hits(&out), vec![("__tests__/c.ts".into(), 16)], "{out:?}");
    assert_eq!(out[0].evidence, "test \"asserts through two helpers\" has no assertion");
}
