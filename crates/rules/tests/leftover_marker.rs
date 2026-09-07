mod common;

use common::{fixture, run_on};
use locrin_core::config::Config;
use locrin_core::finding::{Confidence, Severity};
use locrin_rules::leftover_marker::LeftoverMarker;

#[test]
fn flags_markers_without_issue_references() {
    let out = run_on(Box::new(LeftoverMarker), &fixture("leftover_marker", "flag"), &Config::default());
    let lines: Vec<u32> = out.iter().map(|f| f.span.start_line).collect();
    assert_eq!(lines, vec![1, 3, 5]);
    assert!(out.iter().all(|f| f.severity == Severity::Low && f.confidence == Confidence::Medium));
}

#[test]
fn ignores_markers_with_references_urls_and_lowercase_prose() {
    let out = run_on(Box::new(LeftoverMarker), &fixture("leftover_marker", "clean"), &Config::default());
    assert!(out.is_empty(), "got {:?}", out);
}
