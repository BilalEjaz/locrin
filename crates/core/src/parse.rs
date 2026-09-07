use std::path::{Path, PathBuf};

use anyhow::Context;
use tree_sitter::{Parser, Tree};

use crate::lang::Language;

#[derive(Debug)]
pub struct ParsedFile {
    pub path: PathBuf,
    pub rel: String,
    pub language: Language,
    pub source: String,
    pub tree: Tree,
    pub has_error: bool,
}

pub fn parse_source(path: &Path, rel: &str, source: String) -> Option<ParsedFile> {
    let language = Language::from_path(path)?;
    let mut parser = Parser::new();
    parser
        .set_language(&language.grammar())
        .expect("grammar version matches tree-sitter runtime");
    let tree = parser.parse(source.as_bytes(), None)?;
    let has_error = tree.root_node().has_error();
    Some(ParsedFile { path: path.to_path_buf(), rel: rel.to_string(), language, source, tree, has_error })
}

pub fn rel_path(root: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or(path);
    rel.to_string_lossy().replace('\\', "/")
}

pub fn parse_file(root: &Path, path: &Path) -> anyhow::Result<Option<ParsedFile>> {
    if Language::from_path(path).is_none() {
        return Ok(None);
    }
    let source = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(parse_source(path, &rel_path(root, path), source))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn parses_typescript_and_reports_no_error() {
        let src = "export function add(a: number, b: number): number { return a + b; }\n".to_string();
        let p = parse_source(Path::new("x/add.ts"), "x/add.ts", src).unwrap();
        assert_eq!(p.language, Language::TypeScript);
        assert!(!p.has_error);
        assert_eq!(p.tree.root_node().kind(), "program");
    }

    #[test]
    fn parses_tsx_with_jsx() {
        let src = "export function Row({ item }: { item: string }) { return <div className=\"r\">{item}</div>; }\n".to_string();
        let p = parse_source(Path::new("x/Row.tsx"), "x/Row.tsx", src).unwrap();
        assert_eq!(p.language, Language::Tsx);
        assert!(!p.has_error);
    }

    #[test]
    fn flags_syntax_errors_without_panicking() {
        let src = "export function broken( { return 1;\n".to_string();
        let p = parse_source(Path::new("x/broken.ts"), "x/broken.ts", src).unwrap();
        assert!(p.has_error);
    }

    #[test]
    fn unsupported_language_is_none() {
        assert!(parse_source(Path::new("x/a.py"), "x/a.py", "print(1)".to_string()).is_none());
    }
}
