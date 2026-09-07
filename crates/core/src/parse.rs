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
    parser.set_language(&language.grammar()).expect("grammar version matches tree-sitter runtime");
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

    /// Removes the temp directory on the way out of the test, including when an
    /// assertion panics part way through.
    struct Cleanup(PathBuf);

    impl Drop for Cleanup {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).ok();
        }
    }

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
        let src = "export function Row({ item }: { item: string }) { return <div className=\"r\">{item}</div>; }\n"
            .to_string();
        let p = parse_source(Path::new("x/Row.tsx"), "x/Row.tsx", src).unwrap();
        assert_eq!(p.language, Language::Tsx);
        assert!(!p.has_error);
    }

    #[test]
    fn parses_jsx_in_a_javascript_file() {
        let src = "export function Row({ item }) { return <div className=\"r\">{item}</div>; }\n".to_string();
        let p = parse_source(Path::new("x/Row.jsx"), "x/Row.jsx", src).unwrap();
        assert_eq!(p.language, Language::JavaScript);
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

    #[test]
    fn parse_file_reads_from_disk_and_skips_unsupported() {
        let dir = std::env::temp_dir().join(format!(
            "locrin-parse-file-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _cleanup = Cleanup(dir.clone());
        std::fs::create_dir_all(dir.join("sub")).unwrap();

        // Nested one level down so `rel` carries a separator: on Windows the raw
        // relative path is `sub\sum.ts`, and `rel_path` must normalise it to `/`.
        let ts = dir.join("sub").join("sum.ts");
        std::fs::write(&ts, "export const sum = (a: number, b: number): number => a + b;\n").unwrap();
        let parsed = parse_file(&dir, &ts).unwrap().expect("supported file parses");
        assert_eq!(parsed.rel, "sub/sum.ts");
        assert_eq!(parsed.language, Language::TypeScript);
        assert!(!parsed.has_error);

        // A path that was never written: unsupported extensions return Ok(None)
        // before the file is read, so a missing file is not an error here.
        let py = dir.join("sub").join("script.py");
        assert!(parse_file(&dir, &py).unwrap().is_none());
    }
}
