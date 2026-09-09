//! Flags console.log / console.debug / console.trace / console.dir / console.table
//! calls and `debugger` statements. Purely syntactic: a locally shadowed `console`
//! is still flagged, because a local variable called `console` is itself a leftover.
//! Indirect forms are out of scope by design: `console['log'](...)`,
//! `window.console.log(...)` and `globalThis.console.log(...)` are not flagged,
//! because matching them would cost more false positives than the miss is worth.

use std::sync::OnceLock;

use globset::{Glob, GlobSet, GlobSetBuilder};
use locrin_core::finding::{Category, Confidence, Finding, Severity};
use tree_sitter::Node;

use crate::{clean_files, finding, line_text, Rule, RuleContext, Scope};

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
    fn description(&self) -> &'static str {
        "console.log, console.debug, or debugger left in code"
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
        Ok(out)
    }
}
