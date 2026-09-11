mod common;

use common::{fixture, run_on, run_on_langs, run_unfiltered};
use locrin_core::config::{Config, Languages};
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

/// A marker is a comment, and every language has comments, so this rule
/// declares every language rather than the JavaScript family. With `php = true`
/// the PHP fixture's `//` and `#` markers are findings. Run directly, because
/// the pair ships off (next test) and `run_rules` would drop what the rule
/// finds.
#[test]
fn flags_markers_in_php_when_the_language_is_enabled() {
    let config = Config { languages: Languages { php: true, python: false }, ..Config::default() };
    let out = run_unfiltered(Box::new(LeftoverMarker), &fixture("leftover_marker", "php/flag"), &config);
    let lines: Vec<u32> = out.iter().map(|f| f.span.start_line).collect();
    assert_eq!(lines, vec![2, 5], "got {out:?}");
}

/// The PHP pair ships off: round two of the precision gate scored 4 true of 5
/// on Monica, under the 85 percent it needs, so the same fixture produces
/// nothing through `run_rules`.
#[test]
fn php_is_off_by_default_after_the_precision_gate() {
    let config = Config { languages: Languages { php: true, python: false }, ..Config::default() };
    let out = run_on_langs(Box::new(LeftoverMarker), &fixture("leftover_marker", "php/flag"), &config);
    assert!(out.is_empty(), "got {out:?}");
}
