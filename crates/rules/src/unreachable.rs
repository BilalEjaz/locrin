//! Flags statements that follow an unconditional `return`, `throw`, `break`, or
//! `continue` in the same block. Sibling analysis only, no flow graph: what it
//! flags is dead beyond argument, and what needs a flow graph is left alone.

use locrin_core::finding::{Category, Confidence, Finding, Severity, Span};
use locrin_core::parse::ParsedFile;
use locrin_core::tree::line;
use tree_sitter::Node;

use crate::{anchor_for, clean_files, finding_at, line_text, Rule, RuleContext, Scope};

pub struct Unreachable;

const TERMINATORS: &[&str] = &["return_statement", "throw_statement", "break_statement", "continue_statement"];

/// Statements that take effect regardless of where they sit: hoisted, erased,
/// or empty. Code after a terminator made only of these is not dead.
const POSITION_FREE: &[&str] = &[
    "function_declaration",
    "generator_function_declaration",
    "type_alias_declaration",
    "interface_declaration",
    "ambient_declaration",
    "function_signature",
    "import_alias",
    "import_statement",
    "export_statement",
    "empty_statement",
];

fn body<'a>(node: Node<'a>) -> Vec<Node<'a>> {
    let mut cursor = node.walk();
    match node.kind() {
        "statement_block" | "program" => {
            node.named_children(&mut cursor).filter(|c| c.kind() != "comment" && c.kind() != "hash_bang_line").collect()
        }
        "switch_case" | "switch_default" => node.children_by_field_name("body", &mut cursor).collect(),
        _ => vec![],
    }
}

fn position_free(node: Node) -> bool {
    if POSITION_FREE.contains(&node.kind()) {
        return true;
    }
    // `var x;` is hoisted and does nothing where it stands; `var x = 1;` does.
    if node.kind() == "variable_declaration" {
        let mut cursor = node.walk();
        return node
            .named_children(&mut cursor)
            .all(|d| d.kind() != "variable_declarator" || d.child_by_field_name("value").is_none());
    }
    false
}

struct Dead<'a> {
    keyword: &'static str,
    terminator_line: u32,
    first: Node<'a>,
    last: Node<'a>,
}

fn scan<'a>(node: Node<'a>, out: &mut Vec<Dead<'a>>) {
    let stmts = body(node);
    for (i, s) in stmts.iter().enumerate() {
        let Some(kind) = TERMINATORS.iter().find(|t| **t == s.kind()) else { continue };
        let dead: Vec<Node> = stmts[i + 1..].iter().copied().filter(|n| !position_free(*n)).collect();
        if let (Some(first), Some(last)) = (dead.first(), dead.last()) {
            out.push(Dead {
                keyword: kind.trim_end_matches("_statement"),
                terminator_line: line(*s),
                first: *first,
                last: *last,
            });
        }
        break; // one finding per block: everything after the terminator is covered
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        scan(child, out);
    }
}

fn span(d: &Dead) -> Span {
    Span {
        start_line: line(d.first),
        start_col: d.first.start_position().column as u32,
        end_line: d.last.end_position().row as u32 + 1,
        end_col: d.last.end_position().column as u32,
    }
}

fn report(rule: &Unreachable, file: &ParsedFile, d: &Dead) -> Finding {
    let first_line = line(d.first);
    let fix = format!(
        "Delete the code after the `{}` on line {}, or move it before the `{}`",
        d.keyword, d.terminator_line, d.keyword
    );
    finding_at(rule, &file.rel, span(d), &anchor_for(file, first_line), line_text(file, first_line), &fix)
}

impl Rule for Unreachable {
    fn id(&self) -> &'static str {
        "unreachable"
    }
    fn description(&self) -> &'static str {
        "Code after an unconditional return, throw, break, or continue"
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
    fn confidence(&self) -> Confidence {
        Confidence::High
    }

    fn run(&self, ctx: &RuleContext) -> anyhow::Result<Vec<Finding>> {
        let mut out = Vec::new();
        for file in clean_files(ctx) {
            let mut dead = Vec::new();
            scan(file.tree.root_node(), &mut dead);
            dead.sort_by_key(|d| (line(d.first), d.first.start_position().column));
            out.extend(dead.iter().map(|d| report(self, file, d)));
        }
        Ok(out)
    }
}
