//! Flags the debug statements a language leaves behind.
//!
//! JavaScript: console.log / console.debug / console.trace / console.dir /
//! console.table calls and `debugger` statements. Purely syntactic: a locally
//! shadowed `console` is still flagged, because a local variable called
//! `console` is itself a leftover. Indirect forms are out of scope by design:
//! `console['log'](...)`, `window.console.log(...)` and
//! `globalThis.console.log(...)` are not flagged, because matching them would
//! cost more false positives than the miss is worth.
//!
//! Two kinds of JavaScript or TypeScript file are exempt outright, both of them
//! from the public benchmark of 0.5.0, where this rule reported on 217
//! agent-written diffs and scored 24 percent precision.
//!
//! A script is exempt. All 16 agreed false positives were `console.log` pass
//! messages on the main path of a standalone node test script, files such as
//! `test/gate-matrix-test.mjs` and `spike/test/durable-object.mjs`: printing is
//! the whole output of a file like that, and there is no logger to route it
//! through. A file is a script when its first line starts with `#!`, or when a
//! package.json in the repository runs it directly, meaning a `scripts` value
//! holds `node <path>`, `node --<flag> <path>`, `tsx <path>` or
//! `ts-node <path>` whose path, resolved against that package.json's own
//! directory, is this file. PHP and Python are unaffected: Python's `print(` is
//! already never a sink, and PHP's sinks are the dump functions, which are
//! leftovers in a script as much as anywhere else.
//!
//! A test file is a script in the same sense. On the 0.6.0 benchmark every
//! remaining false positive of this rule, four of the seven findings, was a
//! pass or fail line at the end of `test/scope-prop-test.mjs`: no shebang, and
//! run by a `test/run.mjs` that finds its siblings with `readdirSync` and a
//! dynamic import, so neither of the signals above can see it. What names it
//! is the file itself. A file is a test when its name ends in `.test`, `.spec`,
//! `-test` or `_test` before the extension, or when a directory on its path is
//! exactly `test`, `tests`, `__tests__` or `spec`. The three true findings on
//! that benchmark were in API routes and a component, none of them a test.
//! The match is exact: `latest.ts`, `contest.ts` and `src/testing/` are not
//! tests and keep their findings.
//!
//! A file that has adopted `console` as its logger is exempt, meaning one
//! holding `CONSOLE_LOGGER_THRESHOLD` or more flagged `console` calls. Seven
//! of the disputed findings were status lines in one 3,000 line module of
//! huizongsong/deepchat that holds 39 `console.log` calls tagged
//! `[ThreadPresenter]` and imports no logger: console is that module's logger,
//! and reporting a line of it at a time says nothing a reader can act on.
//!
//! PHP: the dump family (`var_dump`, `print_r`, `var_export`, `dd`, `dump`,
//! `debug_zval_dump`) and `xdebug_break()`. `error_log`, `echo` and `printf`
//! are how PHP writes output on purpose and are never flagged. `print_r` and
//! `var_export` called with a literal `true` as their second argument (or as
//! the named `return` argument) print nothing: they return the rendering as a
//! string, which is how a project builds a log line or a test message, so
//! that form is not a sink either.
//!
//! Python: the debugger entry points (`breakpoint()`, `pdb.set_trace()` and the
//! `ipdb` and `pudb` spellings of it) and the imports that reach them. `print(`
//! is deliberately not a sink: it is how a script speaks, and flagging it would
//! fail the precision gate on the first repository holding a management
//! command.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use globset::{Glob, GlobSet, GlobSetBuilder};
use locrin_core::finding::{Category, Confidence, Finding, Severity};
use locrin_core::parse::ParsedFile;
use serde_json::Value;
use tree_sitter::Node;

use crate::{clean_files, finding, line_text, Language, Rule, RuleContext, Scope, ALL, JS_FAMILY};

#[derive(Default)]
pub struct LeftoverDebug {
    /// The compiled `debug_allowed` set, built on the first file this instance
    /// is run over and reused for every file after it. The file rules run one
    /// file at a time across the pool from a single set of rule instances, so
    /// compiling the globs inside `run` compiles them once per file: a few
    /// regexes against every file in the repository, on the path a pre-commit
    /// hook has three hundred milliseconds to finish. The list it was built
    /// from is stored beside it, so an instance asked to answer under a
    /// different config compiles that config's set rather than serving the
    /// first one's.
    allowed: OnceLock<(Vec<String>, GlobSet)>,
    /// The files each package.json runs with node, read once per directory and
    /// kept for the rest of the run, beside the root they were read under.
    ///
    /// A map behind a lock rather than one set behind a `OnceLock`, because the
    /// whole set cannot be computed from a context: the file pass hands each
    /// file a context holding only itself, so `ctx.files` is one file and not
    /// the tree. What a file does need is small and knowable from its own path,
    /// since a `scripts` entry names a path relative to its package.json, so
    /// only the directories above the file can name it. Each of those is read
    /// at most once per run however many files ask about it, and a run over a
    /// different root clears what the last one kept.
    scripts: Mutex<(PathBuf, HashMap<String, HashSet<String>>)>,
}

const FLAGGED: &[&str] = &["log", "debug", "trace", "dir", "table"];

/// How many flagged `console` calls make a file's console its logger.
///
/// Twenty, from the benchmark module described at the top of this file: 39
/// calls across 3,000 lines with no logger imported. The number is a constant
/// rather than a config key because the config has no per-rule options today
/// (`RuleOverride` carries `enabled`, `severity` and `languages` and refuses
/// anything else), and one exemption does not earn a new config surface.
const CONSOLE_LOGGER_THRESHOLD: usize = 20;

/// The commands that run a JavaScript or TypeScript file directly. Whatever
/// follows one of these in a `scripts` value, past any flags, is a path to a
/// file the project executes.
const NODE_RUNNERS: &[&str] = &["node", "tsx", "ts-node"];

/// The shell tokens that end a command, so a runner with nothing after it does
/// not swallow the next command's name as its path.
const COMMAND_BREAKS: &[&str] = &["&&", "||", ";", "|", "&"];

/// The PHP functions that exist to print a value at a developer and nothing
/// else. `error_log` is missing on purpose: it writes to the configured log and
/// is how a PHP application reports, not how it debugs.
const PHP_SINKS: &[&str] = &["var_dump", "print_r", "var_export", "dd", "dump", "debug_zval_dump", "xdebug_break"];

/// The Python debuggers. `breakpoint` is the built-in; the rest are reached
/// through a module of the same name, so one list answers both the call and the
/// import.
const PY_DEBUGGERS: &[&str] = &["pdb", "ipdb", "pudb"];

/// The globs whose files this rule stays quiet about. An unparseable glob is
/// dropped rather than failing the run: `Config::load` has already rejected the
/// bad pattern, so anything reaching here came from a caller that built its own.
fn allowed_set(globs: &[String]) -> GlobSet {
    let mut b = GlobSetBuilder::new();
    for g in globs {
        if let Ok(glob) = Glob::new(g) {
            b.add(glob);
        }
    }
    b.build().unwrap_or_else(|_| GlobSetBuilder::new().build().unwrap())
}

/// The directory names that hold tests by convention, matched as a whole path
/// segment: `src/testing/` is not one of them.
const TEST_DIRS: &[&str] = &["test", "tests", "__tests__", "spec"];

/// The endings that mark a file as a test, checked against its name with the
/// last extension removed, so `foo.test.ts` is read as `foo.test` and
/// `scope-prop-test.mjs` as `scope-prop-test`. Each ending carries its own
/// separator, which is what keeps `latest.ts` a module.
const TEST_SUFFIXES: &[&str] = &[".test", ".spec", "-test", "_test"];

/// Whether a repo-relative path names a test file: by a directory on its path
/// or by its own name. Pure on the path, so it needs no file read and no memo.
fn is_test_file(rel: &str) -> bool {
    let (dirs, name) = match rel.rsplit_once('/') {
        Some((dirs, name)) => (dirs, name),
        None => ("", rel),
    };
    if dirs.split('/').any(|d| TEST_DIRS.contains(&d)) {
        return true;
    }
    let stem = name.rsplit_once('.').map(|(stem, _)| stem).unwrap_or(name);
    TEST_SUFFIXES.iter().any(|s| stem.ends_with(s))
}

/// The directories whose package.json is asked about this file: the repository
/// root and every directory above the file, root first. A `scripts` value names
/// a path relative to its own package.json, so a package.json beside or below
/// the file cannot name it and is never read on its account.
///
/// One spelling escapes this and is left escaping it: a value that climbs out
/// of its own package with `..`, as `packages/a` running `node ../../tools/x.mjs`
/// does. Answering that would mean reading every package.json in the repository
/// for every file, and the miss costs a finding the rule already reported
/// before this change rather than a new false positive.
fn package_dirs(rel: &str) -> Vec<String> {
    let segments: Vec<&str> = rel.split('/').collect();
    let mut dirs = vec![String::new()];
    for i in 1..segments.len() {
        dirs.push(segments[..i].join("/"));
    }
    dirs
}

/// The paths a `scripts` value runs directly, as written in it. A runner's path
/// is its first argument that is neither a flag nor the end of the command, so
/// `node --test test/other.mjs` names the same file `node test/other.mjs` does,
/// and one value holding two commands names both.
fn runner_paths(script: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut tokens = script.split_whitespace();
    while let Some(token) = tokens.next() {
        if !NODE_RUNNERS.contains(&token) {
            continue;
        }
        for arg in tokens.by_ref() {
            if COMMAND_BREAKS.contains(&arg) {
                break;
            }
            if arg.starts_with('-') {
                continue;
            }
            out.push(arg);
            break;
        }
    }
    out
}

/// A path out of a script, resolved against the directory of the package.json
/// that wrote it and spelled the way a parsed file's `rel` is: forward slashes,
/// no `.` or `..` segments, no leading `./`. An absolute path, or one climbing
/// out of the repository, is nobody's file here and is dropped.
fn resolve(dir: &str, path: &str) -> Option<String> {
    let path = path.trim_matches(|c| c == '"' || c == '\'');
    if path.is_empty() || path.starts_with('/') || path.starts_with('\\') {
        return None;
    }
    let mut parts: Vec<&str> = if dir.is_empty() { Vec::new() } else { dir.split('/').collect() };
    for segment in path.split(['/', '\\']) {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// Every file the package.json in `dir` runs with node, as repo-relative paths.
///
/// Read leniently, the way every other project file this engine reads is: a
/// directory with no package.json, one that cannot be read, and one that is not
/// JSON each contribute nothing rather than failing the run. The metadata call
/// does not follow links, so a `package.json` that is a symlink to somewhere
/// outside the repository is not read.
fn script_targets(root: &Path, dir: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    let path = if dir.is_empty() { root.join("package.json") } else { root.join(dir).join("package.json") };
    if !std::fs::symlink_metadata(&path).map(|m| m.is_file()).unwrap_or(false) {
        return out;
    }
    let Ok(text) = std::fs::read_to_string(&path) else { return out };
    // Editors on Windows write a byte order mark into JSON and no parser takes
    // it, the same reason `project::read_project_file` strips one.
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let Ok(package) = serde_json::from_str::<Value>(text) else { return out };
    let Some(scripts) = package.get("scripts").and_then(Value::as_object) else { return out };
    for script in scripts.values().filter_map(Value::as_str) {
        out.extend(runner_paths(script).into_iter().filter_map(|p| resolve(dir, p)));
    }
    out
}

fn is_debug_call(node: Node, src: &str) -> bool {
    if node.kind() != "call_expression" {
        return false;
    }
    let Some(func) = node.child_by_field_name("function") else { return false };
    if func.kind() != "member_expression" {
        return false;
    }
    let obj = func.child_by_field_name("object").map(|n| n.utf8_text(src.as_bytes()).unwrap_or(""));
    let prop = func.child_by_field_name("property").map(|n| n.utf8_text(src.as_bytes()).unwrap_or(""));
    obj == Some("console") && prop.map(|p| FLAGGED.contains(&p)).unwrap_or(false)
}

/// The name a PHP `function_call_expression` calls, when it calls one plainly.
/// A call through a variable or a method call on an object has neither a `name`
/// nor a `qualified_name` under `function`, and neither is a leftover this rule
/// claims to find.
///
/// A file inside a namespace routinely roots a call to a global function with a
/// leading backslash, and `\var_dump($x)` is the same leftover as `var_dump($x)`.
/// One backslash and no other is what says so: `Acme\dump($x)` calls somebody
/// else's function that happens to share the name.
fn php_called_name<'a>(node: Node, src: &'a str) -> Option<&'a str> {
    if node.kind() != "function_call_expression" {
        return None;
    }
    let func = node.child_by_field_name("function")?;
    let text = func.utf8_text(src.as_bytes()).ok()?;
    match func.kind() {
        "name" => Some(text),
        "qualified_name" => {
            let rooted = text.strip_prefix('\\')?;
            (!rooted.contains('\\')).then_some(rooted)
        }
        _ => None,
    }
}

/// The PHP sinks that grow a `return` flag: with it set to `true` they print
/// nothing and hand the rendering back as a string.
const PHP_RETURN_MODE: &[&str] = &["print_r", "var_export"];

/// Whether a PHP call passes a literal `true` as its `return` flag, either as
/// the second positional argument (`print_r($x, true)`) or by name
/// (`print_r($x, return: true)`). Only the literal counts: `print_r($x, $flag)`
/// may print and stays a sink.
fn php_return_mode(node: Node, src: &str) -> bool {
    let Some(args) = node.child_by_field_name("arguments") else { return false };
    let mut cursor = args.walk();
    let is_true =
        |n: Node| n.kind() == "boolean" && n.utf8_text(src.as_bytes()).is_ok_and(|t| t.eq_ignore_ascii_case("true"));
    let hit = args.named_children(&mut cursor).filter(|n| n.kind() == "argument").enumerate().any(|(i, arg)| {
        let name = arg.child_by_field_name("name").and_then(|n| n.utf8_text(src.as_bytes()).ok());
        let positional_return = name.is_none() && i == 1;
        let named_return = name == Some("return");
        if !(positional_return || named_return) {
            return false;
        }
        let mut inner = arg.walk();
        let literal_true = arg.named_children(&mut inner).any(is_true);
        literal_true
    });
    hit
}

/// PHP function names are case-insensitive, so `VAR_DUMP($x)` is the same call
/// as `var_dump($x)` and the same leftover.
fn is_php_sink(node: Node, src: &str) -> bool {
    let Some(name) = php_called_name(node, src) else { return false };
    if !PHP_SINKS.iter().any(|s| name.eq_ignore_ascii_case(s)) {
        return false;
    }
    let returns = PHP_RETURN_MODE.iter().any(|s| name.eq_ignore_ascii_case(s));
    !(returns && php_return_mode(node, src))
}

/// A Python call to a debugger: the `breakpoint()` built-in, or `set_trace()`
/// on one of the debugger modules. An attribute call is matched on the object
/// and the attribute together, so an unrelated `tracer.set_trace()` is left
/// alone.
fn is_py_sink(node: Node, src: &str) -> bool {
    if node.kind() != "call" {
        return false;
    }
    let Some(func) = node.child_by_field_name("function") else { return false };
    let text = |n: Node| n.utf8_text(src.as_bytes()).unwrap_or("");
    match func.kind() {
        "identifier" => text(func) == "breakpoint",
        "attribute" => {
            let object = func.child_by_field_name("object").map(text).unwrap_or("");
            let attribute = func.child_by_field_name("attribute").map(text).unwrap_or("");
            attribute == "set_trace" && PY_DEBUGGERS.contains(&object)
        }
        _ => false,
    }
}

/// A Python import of a debugger module, in any spelling: `import pdb`,
/// `import pdb as p` and `from pdb import set_trace` are all the line a reader
/// has to delete.
///
/// The module is read off the nodes that hold it rather than out of the
/// statement's text, because the module name is a path and a word match cannot
/// see where the path ends: `from myapp.pdb import models` carries the word
/// `pdb` and imports nobody's debugger. `import_statement` names its modules in
/// `name` children, one per comma, each either a `dotted_name` or an
/// `aliased_import` wrapping one; `import_from_statement` names its module in
/// `module_name`. Each is compared whole.
fn is_py_debug_import(node: Node, src: &str) -> bool {
    let is_debugger = |n: Node| n.utf8_text(src.as_bytes()).is_ok_and(|t| PY_DEBUGGERS.contains(&t));
    match node.kind() {
        "import_statement" => {
            let mut cursor = node.walk();
            // Bound rather than returned: the iterator borrows the cursor, and
            // a tail expression's temporaries outlive the block the cursor
            // lives in.
            let hit = node.children_by_field_name("name", &mut cursor).any(|n| match n.kind() {
                "aliased_import" => n.child_by_field_name("name").is_some_and(is_debugger),
                _ => is_debugger(n),
            });
            hit
        }
        "import_from_statement" => node.child_by_field_name("module_name").is_some_and(is_debugger),
        _ => false,
    }
}

/// Collects the lines to report, and counts the `console` calls among them as
/// it goes: the console count is what decides whether the file has adopted
/// console as its logger, and counting it here is one walk of the tree rather
/// than a second one for the files that would have been reported on.
fn walk(node: Node, src: &str, language: Language, hits: &mut Vec<u32>, console_calls: &mut usize) {
    let hit = match language {
        Language::TypeScript | Language::Tsx | Language::JavaScript => {
            if is_debug_call(node, src) {
                *console_calls += 1;
                true
            } else {
                node.kind() == "debugger_statement"
            }
        }
        Language::Php => is_php_sink(node, src),
        Language::Python => is_py_sink(node, src) || is_py_debug_import(node, src),
    };
    if hit {
        hits.push(node.start_position().row as u32 + 1);
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, src, language, hits, console_calls);
    }
}

impl LeftoverDebug {
    /// Whether this file is a script: one that says so on its first line, one
    /// that is a test by its path, or one a package.json above it runs
    /// directly. Asked only of JavaScript and TypeScript files, because a
    /// shebang on a PHP or Python file says how to run it and says nothing
    /// about a dump call left inside it, and a `breakpoint()` in a Python test
    /// is as much a leftover as one anywhere else.
    fn is_script(&self, root: &Path, file: &ParsedFile) -> bool {
        if !JS_FAMILY.contains(&file.language) {
            return false;
        }
        if file.source.strip_prefix('\u{feff}').unwrap_or(&file.source).starts_with("#!") {
            return true;
        }
        if is_test_file(&file.rel) {
            return true;
        }
        let mut memo = self.scripts.lock().unwrap_or_else(|e| e.into_inner());
        if memo.0 != root {
            memo.0 = root.to_path_buf();
            memo.1.clear();
        }
        package_dirs(&file.rel)
            .into_iter()
            .any(|dir| memo.1.entry(dir.clone()).or_insert_with(|| script_targets(root, &dir)).contains(&file.rel))
    }
}

impl Rule for LeftoverDebug {
    fn id(&self) -> &'static str {
        "leftover-debug"
    }
    fn description(&self) -> &'static str {
        "A debug statement left in code"
    }
    fn scope(&self) -> Scope {
        Scope::File
    }
    fn category(&self) -> Category {
        Category::Erosion
    }
    fn default_severity(&self) -> Severity {
        Severity::High
    }
    fn confidence(&self) -> Confidence {
        Confidence::High
    }
    /// Every language: each one has its own sinks and its own node kinds, and
    /// `walk` dispatches on the file's language rather than matching one
    /// grammar's shapes against another's tree.
    fn languages(&self) -> &'static [Language] {
        ALL
    }

    fn run(&self, ctx: &RuleContext) -> anyhow::Result<Vec<Finding>> {
        let memo =
            self.allowed.get_or_init(|| (ctx.config.debug_allowed.clone(), allowed_set(&ctx.config.debug_allowed)));
        let rebuilt;
        let allowed = if memo.0 == ctx.config.debug_allowed {
            &memo.1
        } else {
            rebuilt = allowed_set(&ctx.config.debug_allowed);
            &rebuilt
        };
        let mut out = Vec::new();
        for file in clean_files(ctx) {
            if allowed.is_match(&file.rel) {
                continue;
            }
            if self.is_script(ctx.root, file) {
                continue;
            }
            let mut hits = Vec::new();
            let mut console_calls = 0;
            walk(file.tree.root_node(), &file.source, file.language, &mut hits, &mut console_calls);
            // A file whose console is its logger reports nothing at all, rather
            // than the lines above the threshold: what is left in it is one
            // decision about the module and not a debug line per finding.
            if console_calls >= CONSOLE_LOGGER_THRESHOLD {
                continue;
            }
            hits.sort_unstable();
            hits.dedup();
            for line in hits {
                let text = line_text(file, line);
                out.push(finding(
                    self,
                    file,
                    line,
                    text,
                    "Remove the debug statement or route it through the project logger",
                ));
            }
        }
        Ok(out)
    }
}
