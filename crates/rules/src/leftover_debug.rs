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

use std::sync::OnceLock;

use globset::{Glob, GlobSet, GlobSetBuilder};
use locrin_core::finding::{Category, Confidence, Finding, Severity};
use tree_sitter::Node;

use crate::{clean_files, finding, line_text, Language, Rule, RuleContext, Scope, ALL};

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
}

const FLAGGED: &[&str] = &["log", "debug", "trace", "dir", "table"];

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

fn walk(node: Node, src: &str, language: Language, hits: &mut Vec<u32>) {
    let hit = match language {
        Language::TypeScript | Language::Tsx | Language::JavaScript => {
            node.kind() == "debugger_statement" || is_debug_call(node, src)
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
        walk(child, src, language, hits);
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
            let mut hits = Vec::new();
            walk(file.tree.root_node(), &file.source, file.language, &mut hits);
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
