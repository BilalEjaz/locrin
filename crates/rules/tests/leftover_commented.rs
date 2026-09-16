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

/// The five blocks the public benchmark reported on Locrin 0.5.0, written
/// again in the same shapes: a `let` sentence, a keyword opening with no code
/// on its line, a banner, a usage line, a French block of commas and
/// parentheses, and a `$` sentence. Every one of them is prose, so the fixture
/// holds no findings at all. The PHP files need the language enabled or the
/// walk drops them, and dropping them would pass this test for the wrong
/// reason, so the run asserts it saw all five.
#[test]
fn prose_blocks_are_not_commented_out_code() {
    let config = Config { languages: Languages { php: true, python: false }, ..Config::default() };
    let root = fixture("leftover_commented", "prose");
    assert_eq!(common::parse_dir_langs(&root, config.languages).len(), 5);
    let out = run_on_langs(Box::new(LeftoverCommented), &root, &config);
    assert!(out.is_empty(), "got {out:?}");
}

/// The other side of the prose veto: a real commented-out function under one
/// sentence of explanation is still a finding, because most of the block still
/// looks like code. One prose line does not buy a block its way out.
#[test]
fn a_commented_out_block_under_one_sentence_is_still_reported() {
    let out = run_on(Box::new(LeftoverCommented), &fixture("leftover_commented", "mixed"), &Config::default());
    let lines: Vec<u32> = out.iter().map(|f| f.span.start_line).collect();
    assert_eq!(lines, vec![2], "{out:?}");
    assert_eq!(out[0].evidence, "The old path summed the rows twice, so it is parked here for now.");
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

/// A prose comment whose header line ends in a colon (`# Note:`, `# Returns:`,
/// `# Options Used:` over a table) is the round-two false positive on Poetry:
/// a colon is a block only behind a suite keyword, and on its own it is a
/// label. Run directly, so the vocabulary is what is being tested whatever the
/// pair's default is.
#[test]
fn python_prose_headers_ending_in_a_colon_are_not_code() {
    let config = Config { languages: Languages { php: false, python: true }, ..Config::default() };
    let out = run_unfiltered(Box::new(LeftoverCommented), &fixture("leftover_commented", "py/clean"), &config);
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
