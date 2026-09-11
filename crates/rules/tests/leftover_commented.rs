mod common;

use common::{fixture, run_on, run_on_langs, run_unfiltered};
use locrin_core::config::{Config, Languages};
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

/// PHP writes a line comment three ways. `//` and `#` are both stripped and
/// both make a run; a `/* */` block is read whole, as it is in TypeScript.
#[test]
fn flags_php_line_runs_hash_runs_and_block_comments() {
    let config = Config { languages: Languages { php: true, python: false }, ..Config::default() };
    let out = run_on_langs(Box::new(LeftoverCommented), &fixture("leftover_commented", "php/flag"), &config);
    let lines: Vec<u32> = out.iter().map(|f| f.span.start_line).collect();
    assert_eq!(lines, vec![4, 13, 18], "{out:?}");
    let evidence: Vec<&str> = out.iter().map(|f| f.evidence.as_str()).collect();
    assert_eq!(evidence, vec!["$old = compute($rows);", "$legacy = 1;", "$dead = 1;"]);
}

/// Prose, a licence header and a docblock are not commented-out PHP.
#[test]
fn ignores_php_prose_docblocks_and_license_headers() {
    let config = Config { languages: Languages { php: true, python: false }, ..Config::default() };
    let out = run_on_langs(Box::new(LeftoverCommented), &fixture("leftover_commented", "php/clean"), &config);
    assert!(out.is_empty(), "got {out:?}");
}

/// Python's vocabulary is its own: a run of `#` lines opening with `for` and
/// `print(` is commented-out code even though none of them ends in a
/// semicolon. Run directly, because the pair ships off (next test) and
/// `run_rules` would drop what the vocabulary finds.
#[test]
fn flags_python_hash_runs() {
    let config = Config { languages: Languages { php: false, python: true }, ..Config::default() };
    let out = run_unfiltered(Box::new(LeftoverCommented), &fixture("leftover_commented", "py/flag"), &config);
    let lines: Vec<u32> = out.iter().map(|f| f.span.start_line).collect();
    assert_eq!(lines, vec![2], "{out:?}");
    assert_eq!(out[0].evidence, "for row in rows:");
}

/// The Python pair ships off: round two of the precision gate scored 0 true of
/// 14 on Poetry, so the same fixture produces nothing through `run_rules`.
#[test]
fn python_is_off_by_default_after_the_precision_gate() {
    let config = Config { languages: Languages { php: false, python: true }, ..Config::default() };
    let out = run_on_langs(Box::new(LeftoverCommented), &fixture("leftover_commented", "py/flag"), &config);
    assert!(out.is_empty(), "got {out:?}");
}

/// A docstring is a string inside an expression statement, never a comment
/// node, so it is not scanned at all. The fixture's docstring holds the exact
/// lines the flag fixture is reported for, and none of them is a finding here.
#[test]
fn python_docstrings_prose_and_license_headers_are_not_scanned() {
    let config = Config { languages: Languages { php: false, python: true }, ..Config::default() };
    let out = run_on_langs(Box::new(LeftoverCommented), &fixture("leftover_commented", "py/clean"), &config);
    assert!(out.is_empty(), "got {out:?}");
}
