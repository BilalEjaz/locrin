//! Import extraction: static imports, re-exports, `import()` with a literal, and
//! `require()` with a literal. Computed specifiers are skipped, never guessed
//! (spec 3.2); a file that builds its paths at runtime is a known blind spot.

use tree_sitter::Node;

use crate::parse::ParsedFile;
use crate::tree::{has_keyword, line, text};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportKind {
    Import,
    Reexport,
    Dynamic,
    Require,
}

impl ImportKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ImportKind::Import => "import",
            ImportKind::Reexport => "reexport",
            ImportKind::Dynamic => "dynamic",
            ImportKind::Require => "require",
        }
    }
}

/// One local name an import statement brings into scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub local: String,
    pub imported: String,
    pub line: u32,
    pub type_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    pub specifier: String,
    pub kind: ImportKind,
    pub line: u32,
    /// Names taken from the target module: `default`, `*` for the whole module,
    /// or the exported names. Empty for a side-effect import.
    pub names: Vec<String>,
    pub bindings: Vec<Binding>,
}

fn unquote(s: &str) -> String {
    s.trim_matches(|c| c == '"' || c == '\'').to_string()
}

/// The value of a plain string literal, or None for an empty one and for anything
/// with escapes or interpolation: a path the engine cannot read off the source, or
/// that is not a path at all, is not one it should record.
fn literal(node: Node, src: &str) -> Option<String> {
    if node.kind() != "string" {
        return None;
    }
    let mut cursor = node.walk();
    let parts: Vec<Node> = node.named_children(&mut cursor).collect();
    match parts.as_slice() {
        [] => None,
        [f] if f.kind() == "string_fragment" => Some(text(*f, src).to_string()),
        _ => None,
    }
}

fn import_statement(node: Node, src: &str) -> Option<Import> {
    let mut cursor = node.walk();
    // `import x = require("y")` carries its source inside the clause.
    if let Some(req) = node.children(&mut cursor).find(|c| c.kind() == "import_require_clause") {
        let specifier = literal(req.child_by_field_name("source")?, src)?;
        let local = text(req.named_child(0)?, src).to_string();
        return Some(Import {
            specifier,
            kind: ImportKind::Require,
            line: line(node),
            names: vec!["*".into()],
            bindings: vec![Binding { local, imported: "*".into(), line: line(node), type_only: false }],
        });
    }
    let specifier = literal(node.child_by_field_name("source")?, src)?;
    let statement_type_only = has_keyword(node, "type");
    let mut names = Vec::new();
    let mut bindings = Vec::new();
    let mut cursor = node.walk();
    if let Some(clause) = node.children(&mut cursor).find(|c| c.kind() == "import_clause") {
        let mut inner = clause.walk();
        for part in clause.named_children(&mut inner) {
            match part.kind() {
                "identifier" => {
                    names.push("default".into());
                    bindings.push(Binding {
                        local: text(part, src).into(),
                        imported: "default".into(),
                        line: line(part),
                        type_only: statement_type_only,
                    });
                }
                "namespace_import" => {
                    let Some(id) = part.named_child(0) else { continue };
                    names.push("*".into());
                    bindings.push(Binding {
                        local: text(id, src).into(),
                        imported: "*".into(),
                        line: line(part),
                        type_only: statement_type_only,
                    });
                }
                "named_imports" => {
                    let mut specs = part.walk();
                    for spec in part.named_children(&mut specs) {
                        if spec.kind() != "import_specifier" {
                            continue;
                        }
                        let Some(name) = spec.child_by_field_name("name") else { continue };
                        let imported = unquote(text(name, src));
                        let local = spec
                            .child_by_field_name("alias")
                            .map(|a| text(a, src).to_string())
                            .unwrap_or_else(|| imported.clone());
                        names.push(imported.clone());
                        bindings.push(Binding {
                            local,
                            imported,
                            line: line(spec),
                            type_only: statement_type_only || has_keyword(spec, "type"),
                        });
                    }
                }
                _ => {}
            }
        }
    }
    Some(Import { specifier, kind: ImportKind::Import, line: line(node), names, bindings })
}

fn export_statement(node: Node, src: &str) -> Option<Import> {
    let specifier = literal(node.child_by_field_name("source")?, src)?;
    let mut names = Vec::new();
    let mut saw_clause = false;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "export_clause" => {
                saw_clause = true;
                let mut specs = child.walk();
                for spec in child.named_children(&mut specs) {
                    if spec.kind() != "export_specifier" {
                        continue;
                    }
                    if let Some(name) = spec.child_by_field_name("name") {
                        names.push(unquote(text(name, src)));
                    }
                }
            }
            "namespace_export" => {
                saw_clause = true;
                names.push("*".into());
            }
            _ => {}
        }
    }
    if !saw_clause {
        names.push("*".into()); // `export * from "./x"`
    }
    Some(Import { specifier, kind: ImportKind::Reexport, line: line(node), names, bindings: vec![] })
}

fn call(node: Node, src: &str) -> Option<Import> {
    let func = node.child_by_field_name("function")?;
    let kind = match func.kind() {
        "import" => ImportKind::Dynamic,
        "identifier" if text(func, src) == "require" => ImportKind::Require,
        _ => return None,
    };
    let args = node.child_by_field_name("arguments")?;
    let specifier = literal(args.named_child(0)?, src)?;
    Some(Import { specifier, kind, line: line(node), names: vec!["*".into()], bindings: vec![] })
}

fn walk(node: Node, src: &str, out: &mut Vec<Import>) {
    let found = match node.kind() {
        "import_statement" => import_statement(node, src),
        "export_statement" => export_statement(node, src),
        "call_expression" => call(node, src),
        _ => None,
    };
    if let Some(i) = found {
        out.push(i);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, src, out);
    }
}

pub fn extract(file: &ParsedFile) -> Vec<Import> {
    let mut out = Vec::new();
    walk(file.tree.root_node(), &file.source, &mut out);
    out
}

/// One significant piece of source text, with comments already removed.
enum Tok {
    Word(String),
    Punct(char),
    /// A single- or double-quoted literal with no escapes, and the line it opened on.
    Literal(String, u32),
    /// A literal the scan will not read as a path: escaped, empty, unterminated, or
    /// a template literal, which may interpolate.
    Opaque,
}

/// Splits `source` into words, punctuation and string literals, dropping `//` and
/// `/* */` comments. Deliberately not a JavaScript lexer: it does not know regular
/// expression literals from division, so a quote inside a regex reads as the start
/// of a string. That string then runs to the end of the line and is discarded as
/// unterminated, which costs at most the specifiers on that one line.
fn tokenize(source: &str) -> Vec<Tok> {
    let chars: Vec<char> = source.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    let mut line = 1u32;
    // Steps over a backslash and whatever it escapes, keeping the line count right.
    let skip_escape = |i: &mut usize, line: &mut u32| {
        if chars.get(*i + 1) == Some(&'\n') {
            *line += 1;
        }
        *i = (*i + 2).min(chars.len());
    };
    while i < chars.len() {
        let c = chars[i];
        if c == '\n' {
            line += 1;
            i += 1;
            continue;
        }
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c == '/' && chars.get(i + 1) == Some(&'/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '/' && chars.get(i + 1) == Some(&'*') {
            i += 2;
            while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                if chars[i] == '\n' {
                    line += 1;
                }
                i += 1;
            }
            i = (i + 2).min(chars.len());
            continue;
        }
        if c == '\'' || c == '"' {
            let opened = line;
            let mut value = String::new();
            let (mut escaped, mut closed) = (false, false);
            i += 1;
            while i < chars.len() && chars[i] != '\n' {
                if chars[i] == '\\' {
                    escaped = true;
                    skip_escape(&mut i, &mut line);
                    continue;
                }
                if chars[i] == c {
                    closed = true;
                    i += 1;
                    break;
                }
                value.push(chars[i]);
                i += 1;
            }
            out.push(if closed && !escaped && !value.is_empty() { Tok::Literal(value, opened) } else { Tok::Opaque });
            continue;
        }
        if c == '`' {
            i += 1;
            while i < chars.len() {
                if chars[i] == '\\' {
                    skip_escape(&mut i, &mut line);
                    continue;
                }
                if chars[i] == '\n' {
                    line += 1;
                }
                let done = chars[i] == '`';
                i += 1;
                if done {
                    break;
                }
            }
            out.push(Tok::Opaque);
            continue;
        }
        if c.is_alphanumeric() || c == '_' || c == '$' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '$') {
                i += 1;
            }
            out.push(Tok::Word(chars[start..i].iter().collect()));
            continue;
        }
        out.push(Tok::Punct(c));
        i += 1;
    }
    out
}

/// Specifiers found by scanning the source text, for a file whose tree came back
/// with errors. Tree-sitter drops whole statements around an ERROR node, so the
/// syntactic extraction is partial; this text scan is not. Every hit is recorded
/// as the whole module (`*`), because the engine cannot tell which names a broken
/// file uses, and a file it cannot read must never make another file look dead.
pub fn extract_text_fallback(source: &str) -> Vec<Import> {
    let toks = tokenize(source);
    let mut out: Vec<Import> = Vec::new();
    for (i, tok) in toks.iter().enumerate() {
        let Tok::Literal(specifier, line) = tok else { continue };
        let before = |n: usize| i.checked_sub(n).map(|k| &toks[k]);
        // `from "x"` covers both export forms as well as every named import.
        let triggered = match before(1) {
            Some(Tok::Word(w)) => w == "from" || w == "import",
            Some(Tok::Punct('(')) => matches!(before(2), Some(Tok::Word(w)) if w == "import" || w == "require"),
            _ => false,
        };
        if !triggered || out.iter().any(|x| &x.specifier == specifier) {
            continue;
        }
        out.push(Import {
            specifier: specifier.clone(),
            kind: ImportKind::Import,
            line: *line,
            names: vec!["*".into()],
            bindings: vec![],
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse_source;
    use std::path::Path;

    const SRC: &str = r#"import React, { useState, type Foo as F } from "react";
import type { Bar } from "./bar";
import * as ns from "./ns";
import "./side";
import x = require("./req");
const y = require("./r2");
const z = await import("./dyn");
const w = require(name);
const v = import(`./${name}`);
export { d } from "./d";
export * from "./e";
export * as f from "./f";
export const g = 1;
import "";
"#;

    fn parsed() -> ParsedFile {
        parse_source(Path::new("src/a.ts"), "src/a.ts", SRC.to_string()).unwrap()
    }

    #[test]
    fn extracts_every_import_form_and_skips_computed_paths() {
        let imports = extract(&parsed());
        let view: Vec<(String, &str, Vec<String>)> =
            imports.iter().map(|i| (i.specifier.clone(), i.kind.as_str(), i.names.clone())).collect();
        assert_eq!(
            view,
            vec![
                ("react".into(), "import", vec!["default".into(), "useState".into(), "Foo".into()]),
                ("./bar".into(), "import", vec!["Bar".into()]),
                ("./ns".into(), "import", vec!["*".into()]),
                ("./side".into(), "import", vec![]),
                ("./req".into(), "require", vec!["*".into()]),
                ("./r2".into(), "require", vec!["*".into()]),
                ("./dyn".into(), "dynamic", vec!["*".into()]),
                ("./d".into(), "reexport", vec!["d".into()]),
                ("./e".into(), "reexport", vec!["*".into()]),
                ("./f".into(), "reexport", vec!["*".into()]),
            ]
        );
    }

    #[test]
    fn bindings_carry_local_names_lines_and_type_flags() {
        let imports = extract(&parsed());
        let react: Vec<(&str, &str, u32, bool)> =
            imports[0].bindings.iter().map(|b| (b.local.as_str(), b.imported.as_str(), b.line, b.type_only)).collect();
        assert_eq!(
            react,
            vec![("React", "default", 1, false), ("useState", "useState", 1, false), ("F", "Foo", 1, true)]
        );
        assert!(imports[1].bindings[0].type_only, "import type marks every binding");
        assert_eq!(imports[2].bindings[0].local, "ns");
        assert!(imports[3].bindings.is_empty(), "a side-effect import binds nothing");
        assert_eq!(imports[4].bindings[0].local, "x");
        assert!(imports[7].bindings.is_empty(), "a re-export binds nothing");
    }

    #[test]
    fn lines_point_at_the_statement() {
        let imports = extract(&parsed());
        assert_eq!(imports.iter().map(|i| i.line).collect::<Vec<_>>(), vec![1, 2, 3, 4, 5, 6, 7, 10, 11, 12]);
    }

    /// An unclosed JSX element: tree-sitter's recovery swallows both import
    /// statements that follow it.
    const BROKEN: &str = "const shell = <View>;\nimport { a } from \"./a\";\nimport { b } from \"./b\";\n";

    #[test]
    fn the_text_fallback_finds_imports_the_parser_dropped() {
        let file = parse_source(Path::new("src/broken.tsx"), "src/broken.tsx", BROKEN.to_string()).unwrap();
        assert!(file.has_error, "the fixture must be a file that failed to parse");
        let syntactic: Vec<String> = extract(&file).iter().map(|i| i.specifier.clone()).collect();
        assert!(syntactic.len() < 2, "recovery must drop at least one of the two imports: {syntactic:?}");

        let found = extract_text_fallback(&file.source);
        let view: Vec<(&str, u32, Vec<String>)> =
            found.iter().map(|i| (i.specifier.as_str(), i.line, i.names.clone())).collect();
        assert_eq!(view, vec![("./a", 2, vec!["*".to_string()]), ("./b", 3, vec!["*".to_string()])]);
        assert!(found.iter().all(|i| i.kind == ImportKind::Import && i.bindings.is_empty()));
    }

    #[test]
    fn the_text_scan_covers_every_form_skips_comments_and_dedups() {
        let src = concat!(
            "// import \"./comment\";\n",
            "/* require(\"./block\") */\n",
            "import \"./side\";\n",
            "import def from \"./a\";\n",
            "export { x } from \"./a\";\n",
            "export * from \"./b\";\n",
            "const c = require(\"./c\");\n",
            "const d = await import(\"./d\");\n",
            "const e = require(name);\n",
            "const f = import(`./${name}`);\n",
            "const g = \"./notanimport\";\n",
            "import x = require(\"./c\");\n",
            "import \"./esc\\ape\";\n",
        );
        let view: Vec<(String, u32)> =
            extract_text_fallback(src).iter().map(|i| (i.specifier.clone(), i.line)).collect();
        assert_eq!(
            view,
            vec![
                ("./side".to_string(), 3),
                ("./a".to_string(), 4),
                ("./b".to_string(), 6),
                ("./c".to_string(), 7),
                ("./d".to_string(), 8),
            ]
        );
    }
}
