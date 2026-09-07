use rusqlite::params;
use tree_sitter::Node;

use crate::index::Index;
use crate::parse::ParsedFile;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub rel: String,
    pub kind: String,
    pub name: String,
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
    pub exported: bool,
}

fn text<'a>(node: Node, src: &'a str) -> &'a str {
    node.utf8_text(src.as_bytes()).unwrap_or("")
}

fn span_of(node: Node) -> (u32, u32, u32, u32) {
    let s = node.start_position();
    let e = node.end_position();
    (s.row as u32 + 1, s.column as u32, e.row as u32 + 1, e.column as u32)
}

fn push(out: &mut Vec<Symbol>, rel: &str, kind: &str, name: &str, node: Node, exported: bool) {
    let (sl, sc, el, ec) = span_of(node);
    out.push(Symbol {
        rel: rel.to_string(),
        kind: kind.to_string(),
        name: name.to_string(),
        start_line: sl,
        start_col: sc,
        end_line: el,
        end_col: ec,
        exported,
    });
}

fn collect(node: Node, src: &str, rel: &str, exported: bool, out: &mut Vec<Symbol>) {
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
                            push(out, rel, "const", text(name, src), child, exported);
                        }
                    }
                }
            }
            return;
        }
        "export_statement" => {
            if let Some(decl) = node.child_by_field_name("declaration") {
                collect(decl, src, rel, true, out);
            }
            return;
        }
        _ => return,
    };
    if let Some(name) = node.child_by_field_name("name") {
        push(out, rel, kind, text(name, src), node, exported);
    }
}

pub fn extract(file: &ParsedFile) -> Vec<Symbol> {
    let mut out = Vec::new();
    let root = file.tree.root_node();
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        collect(child, &file.source, &file.rel, false, &mut out);
    }
    out
}

pub fn enclosing_symbol(file: &ParsedFile, line: u32) -> Option<String> {
    extract(file).into_iter().find(|s| s.start_line <= line && line <= s.end_line).map(|s| s.name)
}

/// Replaces the stored symbols for `file.rel` atomically. The delete and every
/// insert share one transaction, so a failure part way through the loop rolls
/// the whole replacement back rather than leaving a partial symbol set that the
/// content-hash check would consider up to date and never repair.
pub fn store(index: &mut Index, file: &ParsedFile, syms: &[Symbol]) -> anyhow::Result<()> {
    let conn = index.conn();
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM symbols WHERE rel = ?1", params![file.rel])?;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO symbols(rel, kind, name, start_line, start_col, end_line, end_col, exported)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )?;
        for s in syms {
            stmt.execute(params![
                s.rel,
                s.kind,
                s.name,
                s.start_line,
                s.start_col,
                s.end_line,
                s.end_col,
                s.exported as i64
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
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
}
