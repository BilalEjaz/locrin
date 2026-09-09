//! Flags an exported name that no resolved import anywhere in the repository asks
//! for. Barrels are followed one level (spec 3.2). Entry points are exempt: a
//! framework or a runner imports them by convention the graph cannot see. Files
//! with no incoming edge at all are left to `dead-file`, which says it once.
//! Medium confidence because an unresolved import anywhere could be the consumer.

use std::collections::{HashMap, HashSet};

use locrin_core::edges;
use locrin_core::finding::{Category, Confidence, Finding, Severity, Span};
use locrin_core::symbols;

use crate::{finding_at, Rule, RuleContext, Scope};

pub struct DeadExport;

/// For every file, the names resolved edges take from it. `*` is the whole module.
pub(crate) fn imported_names(ctx: &RuleContext) -> anyhow::Result<HashMap<String, HashSet<String>>> {
    let mut out: HashMap<String, HashSet<String>> = HashMap::new();
    for e in edges::resolved(ctx.index)? {
        if let Some(to) = e.to_rel {
            out.entry(to).or_default().insert(e.name);
        }
    }
    Ok(out)
}

impl Rule for DeadExport {
    fn id(&self) -> &'static str {
        "dead-export"
    }
    fn description(&self) -> &'static str {
        "Exported symbol that no file imports"
    }
    fn scope(&self) -> Scope {
        Scope::Graph
    }
    fn category(&self) -> Category {
        Category::Erosion
    }
    fn default_severity(&self) -> Severity {
        Severity::Low
    }
    fn confidence(&self) -> Confidence {
        Confidence::Medium
    }

    fn run(&self, ctx: &RuleContext) -> anyhow::Result<Vec<Finding>> {
        let used = imported_names(ctx)?;
        let mut out = Vec::new();
        for s in symbols::exported(ctx.index)? {
            let Some(export_name) = s.export_name.as_deref() else { continue };
            if ctx.entries.is_entry(&s.rel) {
                continue;
            }
            let Some(names) = used.get(&s.rel) else { continue }; // no importer at all: dead-file territory
            if names.contains(export_name) || names.contains("*") {
                continue;
            }
            let evidence = format!("`{export_name}` is exported from {} but imported nowhere", s.rel);
            let fix = match s.kind.as_str() {
                "export" | "reexport" | "default" => "Remove the export; nothing imports it".to_string(),
                _ => format!(
                    "Drop the `export` keyword if `{}` is only used in this file, or delete it if it is unused",
                    s.name
                ),
            };
            let span =
                Span { start_line: s.start_line, start_col: s.start_col, end_line: s.end_line, end_col: s.end_col };
            out.push(finding_at(self, &s.rel, span, export_name, &evidence, &fix));
        }
        Ok(out)
    }
}
