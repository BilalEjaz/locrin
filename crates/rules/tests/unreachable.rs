mod common;

use common::{fixture, hits, run_on};
use locrin_core::config::Config;
use locrin_core::finding::{Confidence, Severity};
use locrin_rules::unreachable::Unreachable;

#[test]
fn flags_code_after_return_throw_break_and_continue() {
    let out = run_on(Box::new(Unreachable), &fixture("unreachable", "flag"), &Config::default());
    assert_eq!(
        hits(&out),
        vec![("a.ts".into(), 3), ("a.ts".into(), 7), ("a.ts".into(), 14), ("a.ts".into(), 17), ("a.ts".into(), 25)],
        "{out:?}"
    );
    let b = &out[1];
    assert_eq!((b.span.start_line, b.span.end_line), (7, 8), "one finding spans every dead statement in the block");
    assert_eq!(b.evidence, "cleanup();");
    assert_eq!(b.fix, "Delete the code after the `throw` on line 6, or move it before the `throw`");
    assert!(out.iter().all(|f| f.severity == Severity::Medium && f.confidence == Confidence::High));
}

#[test]
fn branches_hoisted_functions_type_declarations_and_case_blocks_are_clean() {
    let out = run_on(Box::new(Unreachable), &fixture("unreachable", "clean"), &Config::default());
    assert!(out.is_empty(), "{out:?}");
}

#[test]
fn comments_hoisted_vars_and_allow_marker() {
    let out = run_on(Box::new(Unreachable), &fixture("unreachable", "edge"), &Config::default());
    assert_eq!(hits(&out), vec![("c.ts".into(), 8)], "{out:?}");
}
