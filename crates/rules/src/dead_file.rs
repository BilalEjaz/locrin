//! Flags a source file that no resolved import reaches and that is not an entry
//! point. Blind spot, by design: files that import only each other count as
//! imported and are not reported; precision beats recall for an advisory.

use locrin_core::finding::{Category, Confidence, Finding, Severity, Span};

use crate::dead_export::imported_names;
use crate::{finding_at, Rule, RuleContext, Scope};

pub struct DeadFile;

impl Rule for DeadFile {
    fn id(&self) -> &'static str {
        "dead-file"
    }
    fn description(&self) -> &'static str {
        "Source file that nothing imports and no framework loads"
    }
    fn scope(&self) -> Scope {
        Scope::Graph
    }
    fn category(&self) -> Category {
        Category::Erosion
    }
    fn default_severity(&self) -> Severity {
        Severity::Medium
    }
    fn confidence(&self) -> Confidence {
        Confidence::Medium
    }

    fn run(&self, ctx: &RuleContext) -> anyhow::Result<Vec<Finding>> {
        let imported = imported_names(ctx)?;
        let mut out = Vec::new();
        for rel in ctx.index.all_files()? {
            if imported.contains_key(&rel) || ctx.entries.is_entry(&rel) {
                continue;
            }
            let span = Span { start_line: 1, start_col: 0, end_line: 1, end_col: 0 };
            out.push(finding_at(
                self,
                &rel,
                span,
                "file",
                &format!("{rel} is imported nowhere and is not an entry point"),
                "Delete the file, or add it to entry_points in locrin.toml if a framework or script loads it by convention",
            ));
        }
        Ok(out)
    }
}
