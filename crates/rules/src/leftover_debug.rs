//! Flags console.log / console.debug / console.trace / console.dir / console.table
//! calls and `debugger` statements. Purely syntactic: a locally shadowed `console`
//! is still flagged, because a local variable called `console` is itself a leftover.
//! Indirect forms are out of scope by design: `console['log'](...)`,
//! `window.console.log(...)` and `globalThis.console.log(...)` are not flagged,
//! because matching them would cost more false positives than the miss is worth.

use globset::{Glob, GlobSetBuilder};
use locrin_core::finding::{Category, Confidence, Finding, Severity};
use tree_sitter::Node;

use crate::{clean_files, finding, line_text, Rule, RuleContext};

pub struct LeftoverDebug;

const FLAGGED: &[&str] = &["log", "debug", "trace", "dir", "table"];

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

fn walk(node: Node, src: &str, hits: &mut Vec<u32>) {
    if node.kind() == "debugger_statement" || is_debug_call(node, src) {
        hits.push(node.start_position().row as u32 + 1);
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, src, hits);
    }
}

impl Rule for LeftoverDebug {
    fn id(&self) -> &'static str {
        "leftover-debug"
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

    fn run(&self, ctx: &RuleContext) -> Vec<Finding> {
        let mut b = GlobSetBuilder::new();
        for g in &ctx.config.debug_allowed {
            if let Ok(glob) = Glob::new(g) {
                b.add(glob);
            }
        }
        let allowed = b.build().unwrap_or_else(|_| GlobSetBuilder::new().build().unwrap());
        let mut out = Vec::new();
        for file in clean_files(ctx) {
            if allowed.is_match(&file.rel) {
                continue;
            }
            let mut hits = Vec::new();
            walk(file.tree.root_node(), &file.source, &mut hits);
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
        out
    }
}
