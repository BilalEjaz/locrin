mod common;

use common::{fixture, run_on};
use locrin_core::config::Config;
use locrin_core::finding::{Confidence, Severity};
use locrin_rules::leftover_commented::LeftoverCommented;

#[test]
fn flags_line_runs_and_block_comments_that_look_like_code() {
    let out = run_on(Box::new(LeftoverCommented), &fixture("leftover_commented", "flag"), &Config::default());
    let lines: Vec<u32> = out.iter().map(|f| f.span.start_line).collect();
    assert_eq!(lines, vec![2, 8]);
    assert!(out.iter().all(|f| f.severity == Severity::Medium && f.confidence == Confidence::Medium));
    let evidence: Vec<&str> = out.iter().map(|f| f.evidence.as_str()).collect();
    assert_eq!(evidence, vec!["const old = compute();", "const legacy = 1;"]);
}

#[test]
fn ignores_prose_jsdoc_and_license_headers() {
    let out = run_on(Box::new(LeftoverCommented), &fixture("leftover_commented", "clean"), &Config::default());
    assert!(out.is_empty(), "got {:?}", out);
}

#[test]
fn two_lines_is_not_a_run() {
    let out = run_on(Box::new(LeftoverCommented), &fixture("leftover_commented", "edge"), &Config::default());
    assert!(out.is_empty());
}
