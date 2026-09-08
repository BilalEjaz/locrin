//! Flags an imported binding the file never references. Purely syntactic: any
//! occurrence of the local name as an identifier, type identifier, shorthand
//! property, or JSX identifier outside the import statements counts as a use,
//! so shadowing means "not flagged" rather than "wrong". Under the classic JSX
//! runtime `React` (or the `@jsx` pragma factory) is referenced by every element.

use std::collections::HashSet;

use locrin_core::finding::{Category, Confidence, Finding, Severity};
use locrin_core::imports;
use tree_sitter::Node;

use crate::{clean_files, finding_at, line_span, Rule, RuleContext, Scope};

pub struct UnusedImport;

const REFERENCE_KINDS: &[&str] = &["identifier", "type_identifier", "shorthand_property_identifier", "jsx_identifier"];

struct Usage {
    names: HashSet<String>,
    has_jsx: bool,
}

fn collect(node: Node, src: &str, u: &mut Usage) {
    match node.kind() {
        "import_statement" => return,
        // `export { x } from "./y"` names x in y, not a local binding.
        "export_statement" if node.child_by_field_name("source").is_some() => return,
        k if REFERENCE_KINDS.contains(&k) => {
            u.names.insert(node.utf8_text(src.as_bytes()).unwrap_or("").to_string());
        }
        k if k.starts_with("jsx_") => u.has_jsx = true,
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect(child, src, u);
    }
}

/// The factory named by a `/** @jsx h */` pragma, if any.
fn jsx_factory(src: &str) -> Option<String> {
    let i = src.find("@jsx ")?;
    src[i + 5..].split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$')).next().map(String::from)
}

impl Rule for UnusedImport {
    fn id(&self) -> &'static str {
        "unused-import"
    }
    fn description(&self) -> &'static str {
        "Imported name that the file never references"
    }
    fn scope(&self) -> Scope {
        Scope::File
    }
    fn category(&self) -> Category {
        Category::Erosion
    }
    fn default_severity(&self) -> Severity {
        Severity::Low
    }
    fn confidence(&self) -> Confidence {
        Confidence::High
    }

    fn run(&self, ctx: &RuleContext) -> anyhow::Result<Vec<Finding>> {
        let mut out = Vec::new();
        for file in clean_files(ctx) {
            let mut usage = Usage { names: HashSet::new(), has_jsx: false };
            collect(file.tree.root_node(), &file.source, &mut usage);
            let factory = jsx_factory(&file.source);
            for import in imports::extract(file) {
                for b in &import.bindings {
                    if usage.names.contains(&b.local) {
                        continue;
                    }
                    if usage.has_jsx && (b.local == "React" || factory.as_deref() == Some(b.local.as_str())) {
                        continue;
                    }
                    let evidence = format!("`{}` is imported from \"{}\" but never used", b.local, import.specifier);
                    let fix = format!(
                        "Remove `{}` from the import, or the whole statement if nothing else from it is used",
                        b.local
                    );
                    let anchor = format!("{}\x1f{}", import.specifier, b.local);
                    out.push(finding_at(self, &file.rel, line_span(file, b.line), &anchor, &evidence, &fix));
                }
            }
        }
        Ok(out)
    }
}
