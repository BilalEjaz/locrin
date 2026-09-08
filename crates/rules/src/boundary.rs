//! Flags a resolved import that crosses a direction the config forbids (spec
//! 4.1, the config-driven half of boundary checking). Inferred boundaries are
//! release two.

use globset::{Glob, GlobMatcher, GlobSet, GlobSetBuilder};
use locrin_core::config::{Boundary, CONFIG_FILE};
use locrin_core::edges;
use locrin_core::finding::{Category, Confidence, Finding, Severity, Span};

use crate::{finding_at, Rule, RuleContext, Scope};

pub struct BoundaryViolation;

struct Compiled<'a> {
    boundary: &'a Boundary,
    from: GlobMatcher,
    forbid: GlobSet,
    allow: GlobSet,
    label: String,
}

fn set(globs: &[String]) -> anyhow::Result<GlobSet> {
    let mut b = GlobSetBuilder::new();
    for g in globs {
        b.add(Glob::new(g)?);
    }
    Ok(b.build()?)
}

fn compile(boundaries: &[Boundary]) -> anyhow::Result<Vec<Compiled<'_>>> {
    boundaries
        .iter()
        .map(|b| {
            let label = b.name.clone().unwrap_or_else(|| {
                format!("{} -> {}", b.from, if b.forbid.is_empty() { "outside allow list" } else { "forbidden" })
            });
            Ok(Compiled {
                boundary: b,
                from: Glob::new(&b.from)?.compile_matcher(),
                forbid: set(&b.forbid)?,
                allow: set(&b.allow)?,
                label,
            })
        })
        .collect()
}

impl Compiled<'_> {
    fn violated(&self, from: &str, to: &str) -> bool {
        if !self.from.is_match(from) {
            return false;
        }
        if !self.boundary.forbid.is_empty() {
            return self.forbid.is_match(to);
        }
        // An allow list: anything outside it is a violation, except the
        // boundary's own side, which may always talk to itself.
        !self.allow.is_match(to) && !self.from.is_match(to)
    }
}

impl Rule for BoundaryViolation {
    fn id(&self) -> &'static str {
        "boundary-violation"
    }
    fn description(&self) -> &'static str {
        "Import that crosses a direction the config forbids"
    }
    fn scope(&self) -> Scope {
        Scope::Graph
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
        if ctx.config.boundaries.is_empty() {
            return Ok(vec![]);
        }
        let compiled = compile(&ctx.config.boundaries)?;
        let mut out: Vec<Finding> = Vec::new();
        for e in edges::resolved(ctx.index)? {
            let Some(to) = e.to_rel.as_deref() else { continue };
            for c in &compiled {
                if !c.violated(&e.from_rel, to) {
                    continue;
                }
                let anchor = format!("{}\x1f{}", c.label, e.specifier);
                // Several names on one import line are one violation.
                if out.iter().any(|f| f.file == e.from_rel && f.span.start_line == e.line && f.related[0] == to) {
                    continue;
                }
                let evidence = format!(
                    "{} imports \"{}\" ({}), which the boundary `{}` forbids",
                    e.from_rel, e.specifier, to, c.label
                );
                let fix = format!(
                    "Move the shared code somewhere both sides may import, or change the boundary in {CONFIG_FILE}"
                );
                let span = Span { start_line: e.line, start_col: 0, end_line: e.line, end_col: 0 };
                let mut f = finding_at(self, &e.from_rel, span, &anchor, &evidence, &fix);
                f.related = vec![to.to_string()];
                out.push(f);
            }
        }
        Ok(out)
    }
}
