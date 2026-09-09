mod common;

use common::{fixture, hits, run_on};
use locrin_core::config::Config;
use locrin_core::finding::{Category, Confidence, Severity};
use locrin_rules::swallowed_error::SwallowedError;

#[test]
fn flags_empty_catch_log_only_catch_and_floating_promises() {
    let out = run_on(Box::new(SwallowedError), &fixture("swallowed_error", "flag"), &Config::default());
    assert_eq!(
        hits(&out),
        vec![("a.ts".into(), 6), ("a.ts".into(), 13), ("a.ts".into(), 28), ("a.ts".into(), 29)],
        "{out:?}"
    );
    assert_eq!(
        out.iter().map(|f| (f.span.start_line, f.confidence)).collect::<Vec<_>>(),
        vec![(6, Confidence::High), (13, Confidence::Medium), (28, Confidence::High), (29, Confidence::High)],
        "an empty catch and a floating promise are certain; a log-only catch is a judgement"
    );
    assert_eq!(
        out.iter().map(|f| f.evidence.as_str()).collect::<Vec<_>>(),
        vec![
            "} catch (e) {}",
            "catch in parseCount only logs; callers use its result",
            "refresh();",
            "loadConfig(\"y\");"
        ]
    );
    assert!(out[0].fix.starts_with("Handle the error, rethrow it, or log it"), "{:?}", out[0].fix);
    assert!(out[1].fix.starts_with("Return a failure value or rethrow"), "{:?}", out[1].fix);
    assert!(out[3].fix.starts_with("await it, or attach .catch"), "{:?}", out[3].fix);
    assert!(out.iter().all(|f| f.severity == Severity::Medium && f.category == Category::Erosion), "{out:?}");
}

#[test]
fn rethrows_returned_failures_unused_results_and_handled_promises_are_clean() {
    let out = run_on(Box::new(SwallowedError), &fixture("swallowed_error", "clean"), &Config::default());
    assert!(out.is_empty(), "{out:?}");
}

#[test]
fn comment_only_catch_and_async_arrow_call_with_the_allow_marker_honoured() {
    let out = run_on(Box::new(SwallowedError), &fixture("swallowed_error", "edge"), &Config::default());
    assert_eq!(hits(&out), vec![("c.ts".into(), 12), ("c.ts".into(), 32), ("c.ts".into(), 39)], "{out:?}");
}
