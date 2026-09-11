mod common;

use common::{fixture, hits, parse_dir, run_on, run_on_langs};
use locrin_core::config::{Config, Languages};
use locrin_core::finding::{Confidence, Severity};
use locrin_rules::leftover_debug::LeftoverDebug;
use locrin_rules::line_text;

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

/// tree-sitter's lexer treats byte 0 as end of input, so a file holding a NUL
/// as a composite-key separator inside a template literal used to parse as if
/// it ended at that byte: the tree carried errors and every rule skipped the
/// whole file, silently. The fixture holds a real NUL on line 2 and a debug
/// line after it, so a finding on line 3 is proof the rules saw the file.
#[test]
fn a_nul_in_a_template_literal_does_not_exclude_the_file() {
    // The bucket is `nul_byte` rather than `nul`: NUL is a reserved DOS device
    // name, and a directory called that cannot be walked on Windows.
    let root = fixture("leftover_debug", "nul_byte");

    let files = parse_dir(&root);
    let file = files.iter().find(|f| f.rel == "key.ts").expect("the fixture file is walked and read");
    // A lone NUL is a valid UTF-8 scalar, so `read_to_string` keeps it: what
    // used to break was the parse, not the read.
    assert!(file.source.contains('\0'), "the fixture must hold a real NUL byte, not the escape");
    assert!(!file.has_error, "no parse errors, so no `excluded from rules` warning");
    // Readers still see the file as written; only the parser got a copy.
    assert!(line_text(file, 2).contains('\0'), "line 2 keeps its NUL: {:?}", line_text(file, 2));

    let out = run_on(Box::new(LeftoverDebug::default()), &root, &Config::default());
    assert_eq!(hits(&out), vec![("key.ts".to_string(), 3)]);
    assert_eq!(out[0].evidence, "console.log(\"key\", k);");
}

/// The JSX-text scanner refuses a bare `&` (tree-sitter-javascript #366), so
/// `BODY & NUTRITION` in a heading used to carry a parse error and exempt the
/// whole screen from every rule. The refusal leaves the rest of the tree
/// intact, so the debug line above it is a finding.
#[test]
fn a_bare_ampersand_in_jsx_text_does_not_exclude_the_file() {
    let root = fixture("leftover_debug", "jsx_text_amp");

    let files = parse_dir(&root);
    let file = files.iter().find(|f| f.rel == "Label.tsx").expect("the fixture file is walked and read");
    assert!(file.source.contains(" & "), "the fixture must hold the bare ampersand");
    assert!(!file.has_error, "no blocking parse errors, so no `excluded from rules` warning");
    // The text is kept as written; only the verdict on the refusal changed.
    assert_eq!(line_text(file, 3), "return <Text>BODY & NUTRITION</Text>;");

    let out = run_on(Box::new(LeftoverDebug::default()), &root, &Config::default());
    assert_eq!(hits(&out), vec![("Label.tsx".to_string(), 2)]);
    assert_eq!(out[0].evidence, "console.log(\"rendering\", id);");
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
    use common::{ctx_for, index_dir};
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

/// PHP's debug sinks are ordinary function calls, so the rule matches them by
/// the called name rather than by a `console` member expression. Every sink the
/// rule knows sits on its own line in the fixture, and line 11 is the spelling a
/// file inside a namespace uses to reach the global function: `\var_dump($x)`.
/// Lines 12 and 13 are `print_r` and `var_export` whose second argument is not
/// the literal `true`, so they still print and are still sinks.
#[test]
fn flags_php_debug_sinks() {
    let config = Config { languages: Languages { php: true, python: false }, ..Config::default() };
    let out = run_on_langs(Box::new(LeftoverDebug::default()), &fixture("leftover_debug", "php/flag"), &config);
    assert_eq!(
        hits(&out),
        vec![
            ("a.php".to_string(), 4),
            ("a.php".to_string(), 5),
            ("a.php".to_string(), 6),
            ("a.php".to_string(), 7),
            ("a.php".to_string(), 8),
            ("a.php".to_string(), 9),
            ("a.php".to_string(), 10),
            ("a.php".to_string(), 11),
            ("a.php".to_string(), 12),
            ("a.php".to_string(), 13),
        ],
        "{out:?}"
    );
    assert!(out.iter().all(|f| f.severity == Severity::High && f.confidence == Confidence::High));
    assert_eq!(out[0].evidence, "var_dump($rows);");
}

/// `error_log`, `echo` and `printf` are how PHP writes output on purpose, and a
/// `->debug()` call is a logger, not a leftover. `Acme\dump()` is a function in
/// somebody's namespace that happens to share a name with the sink: one leading
/// backslash and no other is what makes a qualified name the global function.
/// `print_r($x, true)`, `var_export($x, true)` and the named
/// `print_r($x, return: true)` return a string instead of printing and are the
/// legitimate way to render a value into a message.
#[test]
fn ignores_php_logging_and_output() {
    let config = Config { languages: Languages { php: true, python: false }, ..Config::default() };
    let out = run_on_langs(Box::new(LeftoverDebug::default()), &fixture("leftover_debug", "php/clean"), &config);
    assert!(out.is_empty(), "got {out:?}");
}

/// Python's sinks are the debugger entry points and the imports that reach
/// them: an `import pdb` left at the top of a module is the same leftover as
/// the `set_trace()` it was added for. Line 4 is the aliased spelling,
/// `import pdb as p`, which names the same module.
#[test]
fn flags_python_breakpoints_and_debugger_imports() {
    let config = Config { languages: Languages { php: false, python: true }, ..Config::default() };
    let out = run_on_langs(Box::new(LeftoverDebug::default()), &fixture("leftover_debug", "py/flag"), &config);
    assert_eq!(
        hits(&out),
        vec![
            ("a.py".to_string(), 1),
            ("a.py".to_string(), 2),
            ("a.py".to_string(), 3),
            ("a.py".to_string(), 4),
            ("a.py".to_string(), 8),
            ("a.py".to_string(), 9),
            ("a.py".to_string(), 10),
            ("a.py".to_string(), 11),
        ],
        "{out:?}"
    );
    assert!(out.iter().all(|f| f.severity == Severity::High && f.confidence == Confidence::High));
    assert_eq!(out[0].evidence, "import pdb");
}

/// `print(` is not a sink: it is how a Python script speaks, and flagging it
/// would fail the precision gate on the first repository with a management
/// command in it. `logging.debug` is a logger, and `from myapp.pdb import
/// models` imports somebody's own module whose last segment reads like the
/// debugger's name.
#[test]
fn print_and_logging_are_not_python_sinks() {
    let config = Config { languages: Languages { php: false, python: true }, ..Config::default() };
    let out = run_on_langs(Box::new(LeftoverDebug::default()), &fixture("leftover_debug", "py/clean"), &config);
    assert!(out.is_empty(), "got {out:?}");
}
