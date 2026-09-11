mod common;

use common::{fixture, run_on, run_on_langs};
use locrin_core::config::{Config, Languages};
use locrin_core::finding::{Confidence, Severity};
use locrin_rules::leftover_marker::LeftoverMarker;

/// The last two lines of the fixture are the shapes the path filter used to
/// swallow. A comment's own `//` opener sits in the text the rule reads, so
/// `//TODO: no space` had a `/` before the marker word and read as a file name;
/// `// TODO/FIXME both` had one after it and read the same way. Neither is a
/// path: a `/` separates path segments only between names.
#[test]
fn flags_markers_without_issue_references() {
    let out = run_on(Box::new(LeftoverMarker), &fixture("leftover_marker", "flag"), &Config::default());
    let lines: Vec<u32> = out.iter().map(|f| f.span.start_line).collect();
    assert_eq!(lines, vec![1, 3, 5, 6, 7, 8], "got {out:?}");
    assert!(out.iter().all(|f| f.severity == Severity::Low && f.confidence == Confidence::Medium));
}

/// The clean fixture also carries every marker word glued to a path character:
/// `avatars/XXX.jpg`, `XXX-XXX-XXXX`, `fixtures/TODO_list.json`. A word inside
/// a path or a file name is a name, not a note; the flag fixture's bare
/// `XXX handle this` is the note.
#[test]
fn ignores_markers_with_references_urls_and_lowercase_prose() {
    let out = run_on(Box::new(LeftoverMarker), &fixture("leftover_marker", "clean"), &Config::default());
    assert!(out.is_empty(), "got {:?}", out);
}

/// A marker is a comment, and every language has comments, so this rule
/// declares every language rather than the JavaScript family. With `php = true`
/// the PHP fixture's `//` and `#` markers are findings, and they come through
/// `run_rules`: the pair went off after round two of the precision gate (4
/// true of 5 on Monica) and came back on in round three, once a marker word
/// inside a path stopped counting. The last two lines are PHP's copy of the
/// comment-opener case the TypeScript fixture pins: `//TODO:` and
/// `// TODO/FIXME` are notes, not file names.
#[test]
fn flags_markers_in_php_through_run_rules() {
    let config = Config { languages: Languages { php: true, python: false }, ..Config::default() };
    let out = run_on_langs(Box::new(LeftoverMarker), &fixture("leftover_marker", "php/flag"), &config);
    let lines: Vec<u32> = out.iter().map(|f| f.span.start_line).collect();
    assert_eq!(lines, vec![2, 5, 8, 9], "got {out:?}");
}

/// The round-two false positive, verbatim: `XXX` inside `avatars/XXX.jpg` in
/// a prose comment is a placeholder, and the `TODO` on the next line carries a
/// URL.
#[test]
fn a_placeholder_inside_a_php_path_is_not_a_marker() {
    let config = Config { languages: Languages { php: true, python: false }, ..Config::default() };
    let out = run_on_langs(Box::new(LeftoverMarker), &fixture("leftover_marker", "php/clean"), &config);
    assert!(out.is_empty(), "got {out:?}");
}
