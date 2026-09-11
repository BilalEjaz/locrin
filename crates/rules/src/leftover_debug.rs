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
//! are how PHP writes output on purpose and are never flagged.
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
/// A call through a variable or a method call on an object has no `name` child
/// under `function`, and neither is a leftover this rule claims to find.
fn php_called_name<'a>(node: Node, src: &'a str) -> Option<&'a str> {
    if node.kind() != "function_call_expression" {
        return None;
    }
    let func = node.child_by_field_name("function")?;
    if func.kind() != "name" {
        return None;
    }
    func.utf8_text(src.as_bytes()).ok()
}

/// PHP function names are case-insensitive, so `VAR_DUMP($x)` is the same call
/// as `var_dump($x)` and the same leftover.
fn is_php_sink(node: Node, src: &str) -> bool {
    php_called_name(node, src).is_some_and(|name| PHP_SINKS.iter().any(|s| name.eq_ignore_ascii_case(s)))
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

/// A Python import of a debugger module, in either spelling: `import pdb` and
/// `from pdb import set_trace` are both the line a reader has to delete.
///
/// The whole statement is read as text rather than walked: an import statement
/// is one short line, the module names are identifiers, and the alternative is
/// three node kinds (`dotted_name`, `aliased_import`, `wildcard_import`) for a
/// question a word match answers exactly.
fn is_py_debug_import(node: Node, src: &str) -> bool {
    if !matches!(node.kind(), "import_statement" | "import_from_statement") {
        return false;
    }
    let text = node.utf8_text(src.as_bytes()).unwrap_or("");
    text.split(|c: char| !c.is_ascii_alphanumeric() && c != '_').any(|w| PY_DEBUGGERS.contains(&w))
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
