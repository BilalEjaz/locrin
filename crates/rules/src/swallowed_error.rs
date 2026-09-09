//! Flags three ways a file drops an error on the floor: a catch that does
//! nothing, a catch that only logs in a function whose callers read its result,
//! and a call to a same-file `async` function whose promise nobody holds.
//!
//! Everything here is one file's syntax tree. That is deliberate, and it is
//! where the blind spots are:
//!
//! - **A comment is not handling.** A catch body holding only `// ignore` is
//!   reported as empty. The comment says a human decided to swallow the error;
//!   it does not make the failure visible to anything at runtime. A repository
//!   that means it puts `locrin:allow` on the catch line, which is a decision
//!   written down where the next reader will see it.
//! - **Form 2 reads calls, not types.** "Callers use the result" means the file
//!   calls the enclosing function somewhere outside its own body in a position
//!   whose parent is not an expression statement. A caller in another file, a
//!   call through a variable, and a result used only for its side effects are
//!   all invisible, so the form under-reports rather than guesses. It is
//!   Medium confidence for that reason.
//! - **Form 3 resolves only what one file can resolve.** A call is a floating
//!   promise when it names an `async` function or arrow declared in the same
//!   file, or reaches an `async` method of the same class through `this`. A
//!   call through any other object is left alone, because one file's syntax
//!   cannot say what that object is: `renderer.unmount()` is not the file's own
//!   `async function unmount`, and matching on the property name alone flags it
//!   as one. An imported promise-returning function and a non-`async` function
//!   that returns a promise are left alone for the same reason: resolving them
//!   needs the graph, and a rule that guessed would flag every void call in the
//!   repository.
//! - **No flow analysis anywhere.** A `throw` or `return` reached through a
//!   helper called from the catch body reads as log-only, and a promise awaited
//!   through a variable two statements later reads as floating. Both need the
//!   cross-function analysis that release two adds.

use std::collections::HashSet;

use locrin_core::finding::{Category, Confidence, Finding, Severity};
use locrin_core::parse::ParsedFile;
use locrin_core::tree::{has_keyword, line, text};
use tree_sitter::Node;

use crate::{clean_files, finding, line_text, Rule, RuleContext, Scope};

pub struct SwallowedError;

const EMPTY_FIX: &str =
    "Handle the error, rethrow it, or log it with enough context to act on; an empty catch hides failures";
const LOG_ONLY_FIX: &str =
    "Return a failure value or rethrow; a caller that receives undefined cannot tell an error from an empty result";
const FLOATING_FIX: &str = "await it, or attach .catch, or mark it `void` if the result is deliberately dropped";

fn walk<'a>(node: Node<'a>, f: &mut impl FnMut(Node<'a>)) {
    f(node);
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, f);
    }
}

/// The statements of a catch body, comments dropped. See the module doc on why
/// dropping them is the point rather than a shortcut.
fn catch_statements<'a>(clause: Node<'a>) -> Vec<Node<'a>> {
    let Some(body) = clause.child_by_field_name("body") else { return vec![] };
    let mut cursor = body.walk();
    body.named_children(&mut cursor).filter(|c| c.kind() != "comment").collect()
}

/// Whether a statement is nothing but a `console.<anything>(...)` call.
fn console_only(stmt: Node, src: &str) -> bool {
    if stmt.kind() != "expression_statement" {
        return false;
    }
    let Some(call) = stmt.named_child(0).filter(|c| c.kind() == "call_expression") else { return false };
    let Some(callee) = call.child_by_field_name("function").filter(|c| c.kind() == "member_expression") else {
        return false;
    };
    callee.child_by_field_name("object").is_some_and(|o| text(o, src) == "console")
}

/// The name of the nearest enclosing function-like declaration, which is what a
/// caller would have written to reach the catch.
fn enclosing_function<'a>(node: Node<'a>, src: &str) -> Option<(Node<'a>, String)> {
    let mut current = node.parent();
    while let Some(n) = current {
        let named = match n.kind() {
            "function_declaration" | "generator_function_declaration" | "method_definition" => true,
            "variable_declarator" => n
                .child_by_field_name("value")
                .is_some_and(|v| matches!(v.kind(), "arrow_function" | "function_expression")),
            _ => false,
        };
        if named {
            if let Some(name) = n.child_by_field_name("name") {
                return Some((n, text(name, src).to_string()));
            }
        }
        current = n.parent();
    }
    None
}

/// Whether `name` is called outside `owner`'s own body in a position whose
/// parent is not an expression statement, which is the syntactic reading of
/// "a caller does something with what this returns".
fn result_used_elsewhere(root: Node, src: &str, owner: Node, name: &str) -> bool {
    let mut used = false;
    walk(root, &mut |n: Node| {
        if used || n.kind() != "call_expression" {
            return;
        }
        if n.start_byte() >= owner.start_byte() && n.end_byte() <= owner.end_byte() {
            return;
        }
        let callee = n.child_by_field_name("function");
        if !callee.is_some_and(|c| c.kind() == "identifier" && text(c, src) == name) {
            return;
        }
        used = n.parent().is_some_and(|p| p.kind() != "expression_statement");
    });
    used
}

/// The `async` names a file declares, split by how a call could reach them: a
/// function or arrow bound to a name is called directly, a method only through
/// an object. Keeping them apart is what stops `renderer.unmount()` matching the
/// file's own `async function unmount`.
#[derive(Default)]
struct Asyncs {
    functions: HashSet<String>,
    methods: HashSet<String>,
}

fn async_names(root: Node, src: &str) -> Asyncs {
    let mut out = Asyncs::default();
    walk(root, &mut |n: Node| {
        let (holder, name, methods) = match n.kind() {
            "function_declaration" => (n, n.child_by_field_name("name"), false),
            "method_definition" => (n, n.child_by_field_name("name"), true),
            "variable_declarator" => match n.child_by_field_name("value") {
                Some(v) if matches!(v.kind(), "arrow_function" | "function_expression") => {
                    (v, n.child_by_field_name("name"), false)
                }
                _ => return,
            },
            _ => return,
        };
        if let Some(name) = name.filter(|_| has_keyword(holder, "async")) {
            let set = if methods { &mut out.methods } else { &mut out.functions };
            set.insert(text(name, src).to_string());
        }
    });
    out
}

/// What a bare call statement targets, as far as one file can tell.
enum Callee<'a> {
    /// `name(...)`, which reaches a function or arrow the file declares.
    Bare(&'a str),
    /// `this.name(...)`, the one object a single file can resolve.
    ThisMethod(&'a str),
}

/// The target of a call standing alone as a statement, or `None` when the
/// statement is not a bare call or the callee is not something the file can
/// resolve.
fn floating_callee<'a>(stmt: Node, src: &'a str) -> Option<Callee<'a>> {
    if stmt.kind() != "expression_statement" {
        return None;
    }
    // An `await`ed or `void`ed call is an `await_expression` or a
    // `unary_expression`, so both fall out here rather than needing a check. So
    // does a settled promise: the callee of `f().then(g)` is a member of a call
    // expression, and a call expression is not `this`.
    let call = stmt.named_child(0).filter(|c| c.kind() == "call_expression")?;
    let callee = call.child_by_field_name("function")?;
    match callee.kind() {
        "identifier" => Some(Callee::Bare(text(callee, src))),
        "member_expression" if callee.child_by_field_name("object")?.kind() == "this" => {
            Some(Callee::ThisMethod(text(callee.child_by_field_name("property")?, src)))
        }
        _ => None,
    }
}

fn scan(rule: &SwallowedError, file: &ParsedFile) -> Vec<Finding> {
    let root = file.tree.root_node();
    let src = &file.source;
    let asyncs = async_names(root, src);
    let mut out: Vec<Finding> = Vec::new();
    walk(root, &mut |n: Node| {
        if n.kind() == "catch_clause" {
            let stmts = catch_statements(n);
            let at = line(n);
            if stmts.is_empty() {
                out.push(finding(rule, file, at, line_text(file, at), EMPTY_FIX));
            } else if stmts.iter().all(|s| console_only(*s, src)) {
                if let Some((owner, name)) = enclosing_function(n, src) {
                    if result_used_elsewhere(root, src, owner, &name) {
                        let evidence = format!("catch in {name} only logs; callers use its result");
                        let mut f = finding(rule, file, at, &evidence, LOG_ONLY_FIX);
                        f.confidence = Confidence::Medium;
                        out.push(f);
                    }
                }
            }
            return;
        }
        let floating = match floating_callee(n, src) {
            Some(Callee::Bare(name)) => asyncs.functions.contains(name),
            Some(Callee::ThisMethod(name)) => asyncs.methods.contains(name),
            None => false,
        };
        if floating {
            let at = line(n);
            out.push(finding(rule, file, at, line_text(file, at), FLOATING_FIX));
        }
    });
    out.sort_by_key(|f| (f.span.start_line, f.span.start_col));
    out
}

impl Rule for SwallowedError {
    fn id(&self) -> &'static str {
        "swallowed-error"
    }
    fn description(&self) -> &'static str {
        "An error that nothing acts on: an empty catch, a log-only catch, or an unheld promise"
    }
    fn scope(&self) -> Scope {
        Scope::File
    }
    fn category(&self) -> Category {
        Category::Erosion
    }
    fn default_severity(&self) -> Severity {
        Severity::Medium
    }
    /// High is the rule's own answer, which an empty catch and a floating
    /// promise keep. The log-only form lowers its findings to Medium after
    /// construction, because "the caller uses the result" is read off call
    /// sites in one file and not off types.
    fn confidence(&self) -> Confidence {
        Confidence::High
    }

    fn run(&self, ctx: &RuleContext) -> anyhow::Result<Vec<Finding>> {
        Ok(clean_files(ctx).flat_map(|file| scan(self, file)).collect())
    }
}
