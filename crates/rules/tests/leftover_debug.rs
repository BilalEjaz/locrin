mod common;

use common::{fixture, run_on};
use locrin_core::config::Config;
use locrin_core::finding::{Confidence, Severity};
use locrin_rules::leftover_debug::LeftoverDebug;

#[test]
fn flags_console_log_debug_and_debugger() {
    let out = run_on(Box::new(LeftoverDebug), &fixture("leftover_debug", "flag"), &Config::default());
    let lines: Vec<u32> = out.iter().map(|f| f.span.start_line).collect();
    assert_eq!(lines, vec![2, 3, 4]);
    assert!(out.iter().all(|f| f.severity == Severity::High && f.confidence == Confidence::High));
    assert_eq!(out[0].evidence, "console.log(\"loading\", id);");
    assert_eq!(out[0].rule, "leftover-debug");
}

#[test]
fn ignores_error_warn_and_allowed_paths() {
    let out = run_on(Box::new(LeftoverDebug), &fixture("leftover_debug", "clean"), &Config::default());
    assert!(out.is_empty(), "got {:?}", out);
}

#[test]
fn allow_comment_suppresses_and_shadowed_console_is_still_flagged() {
    let out = run_on(Box::new(LeftoverDebug), &fixture("leftover_debug", "edge"), &Config::default());
    let lines: Vec<u32> = out.iter().map(|f| f.span.start_line).collect();
    assert_eq!(lines, vec![4]);
}
