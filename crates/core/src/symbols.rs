use rusqlite::params;
use tree_sitter::Node;

use crate::index::Index;
use crate::parse::ParsedFile;
use crate::tree::{has_keyword, text};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub rel: String,
    pub kind: String,
    pub name: String,
    /// The name other files import this symbol by, or None when it is not exported.
    /// Differs from `name` for `export default` and `export { a as b }`.
    pub export_name: Option<String>,
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
    pub exported: bool,
}

/// How a declaration is exported, if at all.
#[derive(Clone, Copy)]
enum Export {
    No,
    Named,
    Default,
}

fn span_of(node: Node) -> (u32, u32, u32, u32) {
    let s = node.start_position();
    let e = node.end_position();
    (s.row as u32 + 1, s.column as u32, e.row as u32 + 1, e.column as u32)
}

fn push(out: &mut Vec<Symbol>, rel: &str, kind: &str, name: &str, export_name: Option<String>, node: Node) {
    let (sl, sc, el, ec) = span_of(node);
    out.push(Symbol {
        rel: rel.to_string(),
        kind: kind.to_string(),
        name: name.to_string(),
        exported: export_name.is_some(),
        export_name,
        start_line: sl,
        start_col: sc,
        end_line: el,
        end_col: ec,
    });
}

fn export_name(export: Export, own: &str) -> Option<String> {
    match export {
        Export::No => None,
        Export::Named => Some(own.to_string()),
        Export::Default => Some("default".to_string()),
    }
}

fn unquote(s: &str) -> String {
    s.trim_matches(|c| c == '"' || c == '\'').to_string()
}

fn collect(node: Node, src: &str, rel: &str, export: Export, out: &mut Vec<Symbol>) {
    let kind = match node.kind() {
        "function_declaration" | "generator_function_declaration" => "function",
        "class_declaration" | "abstract_class_declaration" => "class",
        "interface_declaration" => "interface",
        "type_alias_declaration" => "type",
        "enum_declaration" => "enum",
        "lexical_declaration" | "variable_declaration" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "variable_declarator" {
                    if let Some(name) = child.child_by_field_name("name") {
                        if name.kind() == "identifier" {
                            let name = text(name, src);
                            push(out, rel, "const", name, export_name(export, name), child);
                        }
                    }
                }
            }
            return;
        }
        "export_statement" => {
            collect_export(node, src, rel, out);
            return;
        }
        _ => return,
    };
    if let Some(name) = node.child_by_field_name("name") {
        let name = text(name, src);
        push(out, rel, kind, name, export_name(export, name), node);
    }
}

fn collect_export(node: Node, src: &str, rel: &str, out: &mut Vec<Symbol>) {
    let default = has_keyword(node, "default");
    if let Some(decl) = node.child_by_field_name("declaration") {
        let before = out.len();
        collect(decl, src, rel, if default { Export::Default } else { Export::Named }, out);
        // `export default function () {}`: a declaration with no name to export under.
        if out.len() == before && default {
            push(out, rel, "default", "default", Some("default".to_string()), node);
        }
        return;
    }
    if node.child_by_field_name("value").is_some() {
        // `export default <expression>`: an anonymous class, an object, a literal.
        push(out, rel, "default", "default", Some("default".to_string()), node);
        return;
    }
    let kind = if node.child_by_field_name("source").is_some() { "reexport" } else { "export" };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "export_clause" => {
                let mut inner = child.walk();
                for spec in child.named_children(&mut inner) {
                    if spec.kind() != "export_specifier" {
                        continue;
                    }
                    let Some(name) = spec.child_by_field_name("name") else { continue };
                    let local = unquote(text(name, src));
                    let alias = spec.child_by_field_name("alias").map(|a| unquote(text(a, src)));
                    push(out, rel, kind, &local, Some(alias.unwrap_or_else(|| local.clone())), spec);
                }
            }
            "namespace_export" => {
                // `export * as ns from "./x"`: the whole module under one name.
                if let Some(id) = child.named_child(0) {
                    push(out, rel, "reexport", "*", Some(unquote(text(id, src))), node);
                }
            }
            _ => {}
        }
    }
}

pub fn extract(file: &ParsedFile) -> Vec<Symbol> {
    let mut out = Vec::new();
    let root = file.tree.root_node();
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        collect(child, &file.source, &file.rel, Export::No, &mut out);
    }
    out
}

pub fn enclosing_symbol(file: &ParsedFile, line: u32) -> Option<String> {
    extract(file).into_iter().find(|s| s.start_line <= line && line <= s.end_line).map(|s| s.name)
}

/// Replaces the stored symbols for `file.rel` atomically. The delete and every
/// insert share one savepoint, so a failure part way through the loop rolls the
/// whole replacement back rather than leaving a partial symbol set that the
/// content-hash check would consider up to date and never repair.
pub fn store(index: &mut Index, file: &ParsedFile, syms: &[Symbol]) -> anyhow::Result<()> {
    index.savepoint("symbols", |tx| {
        tx.execute("DELETE FROM symbols WHERE rel = ?1", params![file.rel])?;
        let mut stmt = tx.prepare(
            "INSERT INTO symbols(rel, kind, name, export_name, start_line, start_col, end_line, end_col, exported)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )?;
        for s in syms {
            stmt.execute(params![
                s.rel,
                s.kind,
                s.name,
                s.export_name,
                s.start_line,
                s.start_col,
                s.end_line,
                s.end_col,
                s.exported as i64
            ])?;
        }
        Ok(())
    })
}

/// Every exported symbol in the index, in a reproducible order.
pub fn exported(index: &Index) -> anyhow::Result<Vec<Symbol>> {
    let mut stmt = index.conn().prepare(
        "SELECT rel, kind, name, export_name, start_line, start_col, end_line, end_col
         FROM symbols WHERE export_name IS NOT NULL ORDER BY rel, start_line, start_col",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(Symbol {
            rel: r.get(0)?,
            kind: r.get(1)?,
            name: r.get(2)?,
            export_name: r.get(3)?,
            start_line: r.get(4)?,
            start_col: r.get(5)?,
            end_line: r.get(6)?,
            end_col: r.get(7)?,
            exported: true,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::Index;
    use crate::parse::parse_source;
    use std::path::Path;

    const SRC: &str = r#"import { x } from "./x";
export function main(): number {
  return 1;
}
function local(): void {}
export const answer = 42;
const hidden = 1, alsoHidden = 2;
export class Thing {}
export type Id = string;
interface Shape { w: number }
export enum Color { Red }
"#;

    fn parsed() -> crate::parse::ParsedFile {
        parse_source(Path::new("src/a.ts"), "src/a.ts", SRC.to_string()).unwrap()
    }

    #[test]
    fn extracts_top_level_symbols_with_export_flag() {
        let syms = extract(&parsed());
        let view: Vec<(String, String, bool)> =
            syms.iter().map(|s| (s.kind.clone(), s.name.clone(), s.exported)).collect();
        assert_eq!(
            view,
            vec![
                ("function".into(), "main".into(), true),
                ("function".into(), "local".into(), false),
                ("const".into(), "answer".into(), true),
                ("const".into(), "hidden".into(), false),
                ("const".into(), "alsoHidden".into(), false),
                ("class".into(), "Thing".into(), true),
                ("type".into(), "Id".into(), true),
                ("interface".into(), "Shape".into(), false),
                ("enum".into(), "Color".into(), true),
            ]
        );
        let main = &syms[0];
        assert_eq!((main.start_line, main.end_line), (2, 4));
    }

    #[test]
    fn enclosing_symbol_finds_the_function_around_a_line() {
        let p = parsed();
        assert_eq!(enclosing_symbol(&p, 3).as_deref(), Some("main"));
        assert_eq!(enclosing_symbol(&p, 1), None);
    }

    #[test]
    fn store_replaces_symbols_for_a_file() {
        let mut ix = Index::open_in_memory().unwrap();
        let p = parsed();
        let syms = extract(&p);
        store(&mut ix, &p, &syms).unwrap();
        store(&mut ix, &p, &syms).unwrap();
        let n: i64 =
            ix.conn().query_row("SELECT count(*) FROM symbols WHERE rel='src/a.ts'", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 9);
        assert_eq!(n as usize, syms.len());

        // A shorter list must leave exactly the shorter count, so the delete and
        // the inserts land as one replacement rather than accumulating.
        store(&mut ix, &p, &syms[..2]).unwrap();
        let n: i64 =
            ix.conn().query_row("SELECT count(*) FROM symbols WHERE rel='src/a.ts'", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 2);
    }

    #[test]
    fn store_surfaces_failure_and_leaves_files_alone() {
        let mut ix = Index::open_in_memory().unwrap();
        ix.upsert_file("src/a.ts", "typescript", "h1", "ok").unwrap();
        ix.conn().execute_batch("DROP TABLE symbols").unwrap();

        let p = parsed();
        let syms = extract(&p);
        assert!(store(&mut ix, &p, &syms).is_err());

        let files: i64 = ix.conn().query_row("SELECT count(*) FROM files", [], |r| r.get(0)).unwrap();
        assert_eq!(files, 1);
        assert_eq!(ix.file_hash("src/a.ts").unwrap().as_deref(), Some("h1"));
    }

    const EXPORTS: &str = r#"const a = 1;
function b() {}
export { a, b as c };
export default function main() {}
export { d } from "./d";
export * as e from "./e";
export * from "./f";
export const g = 2;
"#;

    #[test]
    fn export_names_cover_clauses_defaults_and_reexports() {
        let p = parse_source(Path::new("src/x.ts"), "src/x.ts", EXPORTS.to_string()).unwrap();
        let syms = extract(&p);
        let view: Vec<(String, String, Option<String>)> =
            syms.iter().map(|s| (s.kind.clone(), s.name.clone(), s.export_name.clone())).collect();
        assert_eq!(
            view,
            vec![
                ("const".into(), "a".into(), None),
                ("function".into(), "b".into(), None),
                ("export".into(), "a".into(), Some("a".into())),
                ("export".into(), "b".into(), Some("c".into())),
                ("function".into(), "main".into(), Some("default".into())),
                ("reexport".into(), "d".into(), Some("d".into())),
                ("reexport".into(), "*".into(), Some("e".into())),
                ("const".into(), "g".into(), Some("g".into())),
            ]
        );
        assert!(syms.iter().all(|s| s.exported == s.export_name.is_some()));
    }

    #[test]
    fn anonymous_default_exports_are_default_symbols() {
        for src in ["export default class {}\n", "export default function () {}\n", "export default 42;\n"] {
            let p = parse_source(Path::new("src/y.ts"), "src/y.ts", src.to_string()).unwrap();
            let syms = extract(&p);
            assert_eq!(syms.len(), 1, "{src}");
            assert_eq!(
                (syms[0].kind.as_str(), syms[0].name.as_str(), syms[0].export_name.as_deref()),
                ("default", "default", Some("default")),
                "{src}"
            );
        }
    }

    #[test]
    fn exported_query_returns_only_exported_symbols_in_order() {
        let mut ix = Index::open_in_memory().unwrap();
        let p = parse_source(Path::new("src/x.ts"), "src/x.ts", EXPORTS.to_string()).unwrap();
        let syms = extract(&p);
        store(&mut ix, &p, &syms).unwrap();
        let names: Vec<String> = exported(&ix).unwrap().into_iter().map(|s| s.export_name.unwrap()).collect();
        assert_eq!(names, vec!["a", "c", "default", "d", "e", "g"]);
    }
}
