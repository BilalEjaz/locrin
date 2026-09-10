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
    /// How many parameters the symbol declares, or None when it is not callable.
    /// A class is not a zero-argument function, so it gets None rather than
    /// `Some(0)`: [`search`] compares counts, and a None never matches.
    pub params: Option<u32>,
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

/// How many parameters the declaration at `node` takes, or None when nothing
/// about it is callable.
///
/// A function declaration, generator included, carries the count on its own
/// `parameters` field. A const carries it on the declarator's `value`, which is
/// where an arrow or a function expression puts the same field. An arrow with a
/// single unparenthesised parameter (`x => x`) has no `formal_parameters` node
/// at all, only a `parameter` field, and that is one parameter. Everything else
/// answers None, which is not the same answer as zero: `search` matches on the
/// count, and a class must not rank as a zero-argument function.
fn params_of(node: Node) -> Option<u32> {
    let callable = match node.kind() {
        "function_declaration" | "generator_function_declaration" => node,
        "variable_declarator" => match node.child_by_field_name("value") {
            Some(v) if matches!(v.kind(), "arrow_function" | "function_expression") => v,
            _ => return None,
        },
        _ => return None,
    };
    match callable.child_by_field_name("parameters") {
        // Named children only: the commas and the parentheses are anonymous.
        Some(list) => Some(list.named_child_count() as u32),
        None => callable.child_by_field_name("parameter").map(|_| 1),
    }
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
        params: params_of(node),
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
            "INSERT INTO symbols(rel, kind, name, export_name, start_line, start_col, end_line, end_col, exported, params)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
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
                s.exported as i64,
                s.params
            ])?;
        }
        Ok(())
    })
}

/// Every exported symbol in the index, in a reproducible order.
pub fn exported(index: &Index) -> anyhow::Result<Vec<Symbol>> {
    let mut stmt = index.conn().prepare(
        "SELECT rel, kind, name, export_name, start_line, start_col, end_line, end_col, params
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
            params: r.get(8)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Words that carry no signal in an intent phrase, so a symbol sharing one of
/// them with the query has not been matched on anything.
pub const STOPWORDS: &[&str] = &[
    "a", "an", "the", "to", "for", "of", "in", "on", "that", "is", "with", "and", "or", "function", "helper", "method",
];

/// Splits an identifier or a phrase into lowercase tokens.
///
/// camelCase, PascalCase, snake_case, kebab-case and whitespace all separate,
/// and a run of capitals stays one token until a lowercase letter starts the
/// next word, so `DAY_RECORD_id` is day, record, id and `HTTPServer` is http,
/// server.
///
/// STOPWORDS are deliberately not applied here. They belong to the intent
/// phrase, which is prose; a symbol genuinely named `toKebab` is named `to` and
/// `kebab`, and dropping half of it would make the name unsearchable.
pub fn tokens(s: &str) -> Vec<String> {
    fn flush(cur: &mut String, out: &mut Vec<String>) {
        if !cur.is_empty() {
            out.push(std::mem::take(cur));
        }
    }
    let chars: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut cur = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if !c.is_alphanumeric() {
            flush(&mut cur, &mut out);
            continue;
        }
        if let Some(prev) = i.checked_sub(1).and_then(|p| chars.get(p)).copied() {
            let camel = c.is_uppercase() && prev.is_lowercase();
            let acronym_end =
                c.is_uppercase() && prev.is_uppercase() && chars.get(i + 1).is_some_and(|n| n.is_lowercase());
            if camel || acronym_end {
                flush(&mut cur, &mut out);
            }
        }
        cur.extend(c.to_lowercase());
    }
    flush(&mut cur, &mut out);
    out
}

/// The same list with the duplicates removed, order preserved, so an overlap is
/// a count of distinct shared tokens rather than of repetitions.
fn unique(list: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(list.len());
    for t in list {
        if !out.contains(&t) {
            out.push(t);
        }
    }
    out
}

/// What a caller is looking for: a name it would have written, the intent in
/// prose, and the signature it expects. Every field is optional; a query with
/// no name and no intent matches nothing, because nothing has been said.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Query {
    pub name: Option<String>,
    pub intent: Option<String>,
    pub params: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    pub rel: String,
    pub line: u32,
    pub kind: String,
    pub name: String,
    pub params: Option<u32>,
    pub exported: bool,
    /// The signature bit first, the name-token overlap second, so a tuple
    /// comparison is the ranking.
    pub score: (u8, u32),
}

/// Ranks every symbol in the index against the query: signature first (the
/// parameter count matches, when both are known), name-token overlap second.
/// Only symbols sharing at least one token with the query are returned, best
/// first, ties by rel then line. `limit` caps the result.
///
/// Deviation from spec 6, recorded so release two knows what to replace: the
/// spec asks for signature *vector* similarity first and name token overlap
/// second. Without the fingerprint index, which is release two, the signature
/// vector version one can honestly compute is the parameter count, and that is
/// what this ranks on. It is a real signal (a three-argument call site wants a
/// three-parameter function) and it is not a similarity score pretending to be
/// one.
pub fn search(index: &Index, q: &Query, limit: usize) -> anyhow::Result<Vec<Match>> {
    let mut wanted = unique(q.name.iter().flat_map(|n| tokens(n)).collect());
    for token in q.intent.iter().flat_map(|i| tokens(i)) {
        if STOPWORDS.contains(&token.as_str()) || wanted.contains(&token) {
            continue;
        }
        wanted.push(token);
    }
    if wanted.is_empty() {
        return Ok(Vec::new());
    }

    let mut stmt = index
        .conn()
        .prepare("SELECT rel, start_line, kind, name, params, exported FROM symbols ORDER BY rel, start_line")?;
    let rows = stmt.query_map([], |r| {
        Ok(Match {
            rel: r.get(0)?,
            line: r.get(1)?,
            kind: r.get(2)?,
            name: r.get(3)?,
            params: r.get(4)?,
            exported: r.get::<_, i64>(5)? != 0,
            score: (0, 0),
        })
    })?;

    let mut hits = Vec::new();
    for row in rows {
        let mut m = row?;
        let overlap = unique(tokens(&m.name)).into_iter().filter(|t| wanted.contains(t)).count() as u32;
        // Nothing in common is not a weak match, it is a different symbol.
        if overlap == 0 {
            continue;
        }
        // An unknown count on either side is not a match: `None == None` would
        // rank every class above a function that shares the same tokens.
        let sig = u8::from(q.params.is_some() && q.params == m.params);
        m.score = (sig, overlap);
        hits.push(m);
    }
    hits.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.rel.cmp(&b.rel)).then_with(|| a.line.cmp(&b.line)));
    hits.truncate(limit);
    Ok(hits)
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

    const PARAMS: &str = r#"function a(x, y) {}
export const b = (p) => p;
const c = function () {};
class D {}
const e = 1;
function* f(one, two, three) {}
const g = x => x;
"#;

    #[test]
    fn extracts_parameter_counts() {
        let p = parse_source(Path::new("src/p.ts"), "src/p.ts", PARAMS.to_string()).unwrap();
        let view: Vec<(String, Option<u32>)> = extract(&p).iter().map(|s| (s.name.clone(), s.params)).collect();
        assert_eq!(
            view,
            vec![
                ("a".to_string(), Some(2)),
                ("b".to_string(), Some(1)),
                ("c".to_string(), Some(0)),
                ("D".to_string(), None),
                ("e".to_string(), None),
                ("f".to_string(), Some(3)),
                // `x => x` carries its one parameter unparenthesised, so there is
                // no `formal_parameters` node to count.
                ("g".to_string(), Some(1)),
            ]
        );
    }

    #[test]
    fn tokens_split_every_casing() {
        assert_eq!(tokens("listFoodEntriesBetween"), vec!["list", "food", "entries", "between"]);
        assert_eq!(tokens("DAY_RECORD_id"), vec!["day", "record", "id"]);
        // `to` survives: STOPWORDS apply to the intent phrase, not to a name.
        assert_eq!(tokens("to-kebab"), vec!["to", "kebab"]);
    }

    const SEARCHABLE: &str = r#"export function listFoodEntries(a, b) {}
export function listFoodEntriesBetween(a, b, c) {}
export function foodTotal(a) {}
export function unrelated() {}
"#;

    fn searchable_index() -> Index {
        let mut ix = Index::open_in_memory().unwrap();
        let p = parse_source(Path::new("src/s.ts"), "src/s.ts", SEARCHABLE.to_string()).unwrap();
        let syms = extract(&p);
        store(&mut ix, &p, &syms).unwrap();
        ix
    }

    #[test]
    fn search_ranks_signature_then_overlap() {
        let ix = searchable_index();

        // Both `listFood*` share two tokens with `listEntries`; the three
        // parameter one matches the signature too, so it comes first.
        let q = Query { name: Some("listEntries".to_string()), intent: None, params: Some(3) };
        let hits = search(&ix, &q, 10).unwrap();
        let names: Vec<&str> = hits.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, vec!["listFoodEntriesBetween", "listFoodEntries"]);
        assert_eq!(hits[0].score, (1, 2));
        assert_eq!(hits[1].score, (0, 2));
        assert_eq!(hits[0].params, Some(3));
        assert_eq!(
            (hits[0].rel.as_str(), hits[0].line, hits[0].kind.as_str(), hits[0].exported),
            ("src/s.ts", 2, "function", true)
        );

        // The intent shares only `food`, which the three food symbols carry and
        // `unrelated` does not. All three score the same, so the set is what is
        // asserted, not an order within it.
        let q = Query { name: None, intent: Some("sum the food for a day".to_string()), params: None };
        let hits = search(&ix, &q, 10).unwrap();
        let mut names: Vec<&str> = hits.iter().map(|m| m.name.as_str()).collect();
        names.sort();
        assert_eq!(names, vec!["foodTotal", "listFoodEntries", "listFoodEntriesBetween"]);
        assert!(hits.iter().all(|m| m.score == (0, 1)), "{hits:?}");
    }

    #[test]
    fn search_caps_at_the_limit() {
        let ix = searchable_index();
        let q = Query { name: Some("food".to_string()), intent: None, params: None };
        assert_eq!(search(&ix, &q, 2).unwrap().len(), 2);
    }
}
