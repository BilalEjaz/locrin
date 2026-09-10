mod common;

use common::{fixture, hits, run_on};
use locrin_core::config::Config;
use locrin_core::finding::{Confidence, Severity};
use locrin_rules::leftover_debug::LeftoverDebug;

#[test]
fn flags_console_log_debug_and_debugger() {
    let out = run_on(Box::new(LeftoverDebug::default()), &fixture("leftover_debug", "flag"), &Config::default());
    assert_eq!(
        hits(&out),
        vec![
            ("a.ts".to_string(), 2),
            ("a.ts".to_string(), 3),
            ("a.ts".to_string(), 4),
            ("twice.ts".to_string(), 2),
            ("twice.ts".to_string(), 3),
        ]
    );
    assert!(out.iter().all(|f| f.severity == Severity::High && f.confidence == Confidence::High));
    assert_eq!(out[0].evidence, "console.log(\"loading\", id);");
    assert_eq!(out[0].rule, "leftover-debug");
}

/// The same debug line twice in one function is two findings a reader has to
/// remove separately, so it is two ids: with one id between them, accepting
/// either into a baseline would silently accept the other.
#[test]
fn two_identical_debug_lines_in_one_function_get_two_ids() {
    let out = run_on(Box::new(LeftoverDebug::default()), &fixture("leftover_debug", "flag"), &Config::default());
    let twice: Vec<&str> = out.iter().filter(|f| f.file == "twice.ts").map(|f| f.id.as_str()).collect();
    assert_eq!(twice.len(), 2, "the fixture holds the line twice: {:?}", hits(&out));
    assert_ne!(twice[0], twice[1], "identical lines, separate findings");

    let mut ids: Vec<&str> = out.iter().map(|f| f.id.as_str()).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), out.len(), "no two findings of this rule share an id");
}

#[test]
fn ignores_error_warn_and_allowed_paths() {
    let out = run_on(Box::new(LeftoverDebug::default()), &fixture("leftover_debug", "clean"), &Config::default());
    assert!(out.is_empty(), "got {:?}", out);
}

#[test]
fn allow_comment_suppresses_and_shadowed_console_is_still_flagged() {
    let out = run_on(Box::new(LeftoverDebug::default()), &fixture("leftover_debug", "edge"), &Config::default());
    let lines: Vec<u32> = out.iter().map(|f| f.span.start_line).collect();
    assert_eq!(lines, vec![4]);
}

/// One rule instance answers for every file of a run, so it keeps the compiled
/// allow list rather than recompiling it once per file. What it kept belongs to
/// the config it was built under: asked again under a different one, it has to
/// compile that config's list instead of serving the first one's.
#[test]
fn a_reused_instance_recompiles_the_allow_list_when_the_config_changes() {
    use common::{ctx_for, index_dir, parse_dir};
    use locrin_rules::Rule;

    let root = fixture("leftover_debug", "clean");
    let files = parse_dir(&root);
    let rule = LeftoverDebug::default();

    let allowed = Config::default();
    let (ix, entries) = index_dir(&root, &files, &allowed);
    let ctx = ctx_for(&files, &allowed, &ix, &entries, &root);
    assert!(rule.run(&ctx).unwrap().is_empty(), "the default config allows **/scripts/**");

    let nothing_allowed = Config { debug_allowed: vec![], ..Config::default() };
    let ctx = ctx_for(&files, &nothing_allowed, &ix, &entries, &root);
    let out = rule.run(&ctx).unwrap();
    assert_eq!(out.len(), 1, "with no allow list the script's debug line is a finding: {out:?}");
    assert_eq!(out[0].file, "scripts/build.ts");
}
