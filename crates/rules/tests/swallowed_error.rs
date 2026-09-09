mod common;

use common::{fixture, hits, rule_on, run_on};
use locrin_core::finding::{Category, Confidence, Severity};
use locrin_rules::swallowed_error::SwallowedError;

#[test]
fn flags_empty_catch_log_only_catch_and_floating_promises() {
    let out = run_on(Box::new(SwallowedError), &fixture("swallowed_error", "flag"), &rule_on("swallowed-error"));
    assert_eq!(
        hits(&out),
        vec![("a.ts".into(), 6), ("a.ts".into(), 13), ("a.ts".into(), 28), ("a.ts".into(), 29), ("a.ts".into(), 38)],
        "{out:?}"
    );
    assert_eq!(
        out.iter().map(|f| (f.span.start_line, f.confidence)).collect::<Vec<_>>(),
        vec![
            (6, Confidence::High),
            (13, Confidence::Medium),
            (28, Confidence::High),
            (29, Confidence::High),
            (38, Confidence::Medium)
        ],
        "an empty catch and a floating promise are certain; a log-only catch is a judgement"
    );
    assert_eq!(
        out.iter().map(|f| f.evidence.as_str()).collect::<Vec<_>>(),
        vec![
            "} catch (e) {}",
            "catch in parseCount only logs; callers use its result",
            "refresh();",
            "loadConfig(\"y\");",
            "catch in size only logs; callers use its result"
        ]
    );
    assert!(out[0].fix.starts_with("Handle the error, rethrow it, or log it"), "{:?}", out[0].fix);
    assert!(out[1].fix.starts_with("Return a failure value or rethrow"), "{:?}", out[1].fix);
    assert!(out[3].fix.starts_with("await it, or attach .catch"), "{:?}", out[3].fix);
    assert!(out.iter().all(|f| f.severity == Severity::Medium && f.category == Category::Erosion), "{out:?}");
}

#[test]
fn rethrows_returned_failures_unused_results_and_handled_promises_are_clean() {
    let out = run_on(Box::new(SwallowedError), &fixture("swallowed_error", "clean"), &rule_on("swallowed-error"));
    assert!(out.is_empty(), "{out:?}");
}

/// The comment-only catch is the case this rule gets wrong most often on real
/// code: a maintainer who writes down why the failure is not worth handling has
/// made the decision the rule exists to ask for. Only a catch with nothing in it
/// at all, not even a comment, is a swallow.
#[test]
fn a_commented_catch_is_exempt_and_the_allow_marker_is_honoured() {
    let out = run_on(Box::new(SwallowedError), &fixture("swallowed_error", "edge"), &rule_on("swallowed-error"));
    assert_eq!(hits(&out), vec![("c.ts".into(), 32), ("c.ts".into(), 39)], "{out:?}");
}
