use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::Context;
use tree_sitter::{Node, Parser, Tree};

use crate::lang::Language;
use crate::symbols::Symbol;
use crate::tree;

#[derive(Debug)]
pub struct ParsedFile {
    pub path: PathBuf,
    pub rel: String,
    pub language: Language,
    pub source: String,
    pub tree: Tree,
    pub has_error: bool,
    /// The file's symbol table, extracted on first use by
    /// [`crate::symbols::symbols_of`] and shared from then on.
    ///
    /// Extracting walks the whole tree and allocates, and the callers that want
    /// it want it once per finding rather than once per file: a file with a
    /// hundred findings walked its tree a hundred times. A `OnceLock` rather
    /// than a `OnceCell` because rules run over files in a rayon pool, so a
    /// `&ParsedFile` crosses threads and the file has to stay `Sync`.
    pub(crate) symbols: OnceLock<Vec<Symbol>>,
}

/// The bytes to hand the parser.
///
/// tree-sitter's lexer reads byte 0 as end of input: on a source holding a NUL
/// (a composite-key separator in a template literal is the case that found this)
/// the template scanner stops at that byte, the parser errors on it, and one
/// error excludes the whole file from every rule. The parser is handed a copy
/// with every NUL replaced by 0x01, one byte for one byte so every span still
/// indexes the original; every reader keeps the file as written.
///
/// 0x01 is a control character no grammar rule matches specially, and it is as
/// valid as any other character inside a string or a template. Borrowed rather
/// than copied for the overwhelming majority of files, which hold no NUL.
fn parser_bytes(source: &str) -> Cow<'_, [u8]> {
    if !source.as_bytes().contains(&0) {
        return Cow::Borrowed(source.as_bytes());
    }
    Cow::Owned(source.as_bytes().iter().map(|&b| if b == 0 { 1 } else { b }).collect())
}

/// Whether the tree has a parse error the engine cannot see past.
///
/// The JSX-text scanner refuses a bare `&` (tree-sitter-javascript #366, open
/// upstream), leaving an ERROR node among the element's text children with the
/// rest of the tree intact; that one shape is tolerated so `BODY & NUTRITION`
/// in a heading does not exempt a 2000-line screen from every rule. Everything
/// else is an error: tree-sitter recovers by dropping nodes, and a rule reading
/// a recovered tree reads what was dropped as absent.
fn has_blocking_error(root: Node, src: &str) -> bool {
    tree::error_nodes(root).into_iter().any(|n| !is_tolerated_jsx_text_error(n, src))
}

/// Whether one error node is the scanner refusing a run of JSX text.
///
/// The refusal parents to the element itself, between the text before it and
/// the close tag, and holds identifiers for the words it swallowed. A broken
/// tag inside an element parents there too and looks the same, so the text is
/// what separates them: a run carrying `<`, `{` or `}` is markup the parser
/// gave up on, not text, and whatever it dropped is invisible to the rules.
fn is_tolerated_jsx_text_error(node: Node, src: &str) -> bool {
    // A MISSING node is a token the parser invented that is not in the file, so
    // it is never tolerated however it is spelled; `is_error` excludes it.
    if !node.is_error() {
        return false;
    }
    let Some(parent) = node.parent() else { return false };
    if !matches!(parent.kind(), "jsx_element" | "jsx_fragment") {
        return false;
    }
    if tree::text(node, src).contains(['<', '{', '}']) {
        return false;
    }
    // One flat refusal, not a refusal wrapped around further wreckage: the walk
    // stops at the outermost error, so anything broken further in is only
    // visible from here.
    let mut cursor = node.walk();
    let mut children = node.children(&mut cursor);
    !children.any(|c| c.has_error())
}

pub fn parse_source(path: &Path, rel: &str, source: String) -> Option<ParsedFile> {
    let language = Language::from_path(path)?;
    let mut parser = Parser::new();
    parser.set_language(&language.grammar()).expect("grammar version matches tree-sitter runtime");
    let tree = parser.parse(parser_bytes(&source).as_ref(), None)?;
    let has_error = has_blocking_error(tree.root_node(), &source);
    Some(ParsedFile {
        path: path.to_path_buf(),
        rel: rel.to_string(),
        language,
        source,
        tree,
        has_error,
        symbols: OnceLock::new(),
    })
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

    /// A NUL byte inside a template literal is what FastLift writes as a
    /// composite-key separator, and tree-sitter's lexer reads byte 0 as end of
    /// input: the template scanner stopped at that byte, the parser errored on
    /// it, and that one error excluded the whole file from every rule. The
    /// parser sees a substitute; every reader keeps the NUL.
    ///
    /// The matching `line_text` assertion lives in the rules e2e instead: rules
    /// depends on core, so core cannot call back into it from a unit test.
    #[test]
    fn a_nul_inside_a_template_literal_parses() {
        let src = "const a = 1, b = 2;\nconst k = `${a}\0${b}`;\n".to_string();
        let p = parse_source(Path::new("x/key.ts"), "x/key.ts", src.clone()).unwrap();
        assert!(!p.has_error, "tree: {}", p.tree.root_node().to_sexp());
        // Every reader slices `source` by the byte ranges the tree reports, so
        // the file has to stay exactly as written, NUL and all.
        assert_eq!(p.source, "const a = 1, b = 2;\nconst k = `${a}\0${b}`;\n");
        // One byte for one byte: the node covering the substituted byte has to
        // slice the original back out as the NUL that was written there.
        let at = p.source.find('\0').unwrap();
        let node = p.tree.root_node().descendant_for_byte_range(at, at + 1).unwrap();
        assert!(
            p.source[node.byte_range()].contains('\0'),
            "sliced {:?} out of the original",
            &p.source[node.byte_range()]
        );

        // `.ts` is the TypeScript grammar; every `.tsx`, `.js`, `.jsx`, `.mjs`
        // and `.cjs` file in a scanned repo goes through the TSX one, and the
        // two share the external scanner that reads byte 0 as end of input, so
        // the substitute has to hold on both or half a repo stays excluded.
        for path in ["x/key.tsx", "x/key.js"] {
            let p = parse_source(Path::new(path), path, src.clone()).unwrap();
            assert!(!p.has_error, "{path} tree: {}", p.tree.root_node().to_sexp());
        }
    }

    /// The copy is the exception, not the rule: a file with no NUL, which is
    /// every file but a handful, is handed to the parser without being copied.
    #[test]
    fn parser_bytes_borrows_when_there_is_no_nul() {
        assert!(matches!(parser_bytes("const a = 1;\n"), Cow::Borrowed(_)));
        assert_eq!(parser_bytes("a\0b").as_ref(), b"a\x01b");
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

    /// The JSX-text scanner refuses a bare `&` (tree-sitter-javascript #366),
    /// which is how FastLift writes `BODY & NUTRITION` in a heading. The refusal
    /// leaves one ERROR node among the element's text children and the rest of
    /// the tree intact, so the file is readable and must not be exempt from
    /// every rule over one ampersand.
    #[test]
    fn a_bare_ampersand_in_jsx_text_is_tolerated() {
        let src = "function A() { return <Label>BODY & NUTRITION</Label>; }\n".to_string();
        let p = parse_source(Path::new("x/a.tsx"), "x/a.tsx", src).unwrap();
        assert!(!p.has_error, "tree: {}", p.tree.root_node().to_sexp());
        // The tolerance is about what the engine can see, not about the tree
        // being clean: tree-sitter still reports the refusal.
        assert!(p.tree.root_node().has_error(), "the grammar still errors; only our verdict changed");
    }

    /// The refusal that is tolerated is a text run inside an element. An error
    /// anywhere else, an attribute the parser could not read among them, drops
    /// nodes the rules would then read as absent, so it stays an error.
    #[test]
    fn a_broken_attribute_is_still_an_error() {
        let src = "function A() { return <Label a=>x</Label>; }\n".to_string();
        let p = parse_source(Path::new("x/a.tsx"), "x/a.tsx", src).unwrap();
        assert!(p.has_error, "tree: {}", p.tree.root_node().to_sexp());
    }

    /// The fifth FastLift file the gate excludes is not an ampersand: the parser
    /// invents a MISSING identifier for `as import('./t').Seg[]`. A token that is
    /// not in the file is exactly what the engine cannot see past, so that file
    /// stays excluded rather than being read from a tree the parser made up.
    #[test]
    fn an_import_type_with_an_array_suffix_is_still_an_error() {
        let src = "const s = j as import('./t').Seg[];\n".to_string();
        let p = parse_source(Path::new("x/t.ts"), "x/t.ts", src).unwrap();
        assert!(p.has_error, "tree: {}", p.tree.root_node().to_sexp());
    }

    /// Every refusal inside an element parents to `jsx_element` and holds an
    /// identifier, whether what it swallowed was an ampersand or a broken tag,
    /// so the text is the only thing that separates them: a run carrying `<`,
    /// `{` or `}` is markup the parser gave up on, not text.
    #[test]
    fn a_stray_bracket_in_jsx_text_is_still_an_error() {
        for src in
            ["function A() { return <Label>a </b</Label>; }\n", "function A() { return <Label>a } b</Label>; }\n"]
        {
            let p = parse_source(Path::new("x/a.tsx"), "x/a.tsx", src.to_string()).unwrap();
            assert!(p.has_error, "{src:?} tree: {}", p.tree.root_node().to_sexp());
        }
    }

    /// The same tolerance covers a stray `>`: the scanner refuses the run, the
    /// refusal parents to the element among its text children, and only `<`,
    /// `{` and `}` mark a run as markup the parser gave up on. `5 > 3` in a
    /// label is prose, and it must not exempt the screen around it from every
    /// rule.
    #[test]
    fn a_stray_closing_angle_bracket_in_jsx_text_is_tolerated() {
        for src in
            ["function A() { return <Label>a > b</Label>; }\n", "function A() { return <Label>5 > 3 wins</Label>; }\n"]
        {
            let p = parse_source(Path::new("x/a.tsx"), "x/a.tsx", src.to_string()).unwrap();
            assert!(!p.has_error, "{src:?} tree: {}", p.tree.root_node().to_sexp());
        }
    }

    /// What a bare `&` costs depends on what follows it in the same text run.
    /// The parser recovers by reading the remainder as an expression, and a full
    /// stop or a comma with more text after it makes that expression a member
    /// access or a sequence: the recovery then swallows the element itself, the
    /// ERROR node lands at the top of the file instead of among an element's
    /// text children, and the file is excluded from every rule. A run that ends
    /// at the full stop has nothing for the parser to read past it, so it stays
    /// tolerated.
    #[test]
    fn an_ampersand_whose_text_run_carries_on_past_a_full_stop_is_still_an_error() {
        for src in [
            "function A() { return <Label>tea & toast. Lovely</Label>; }\n",
            "function A() { return <Label>a & b, c</Label>; }\n",
            "function A() { return <Label>AT&T. now</Label>; }\n",
        ] {
            let p = parse_source(Path::new("x/a.tsx"), "x/a.tsx", src.to_string()).unwrap();
            assert!(p.has_error, "{src:?} tree: {}", p.tree.root_node().to_sexp());
        }
        for src in
            ["function A() { return <Label>tea & toast.</Label>; }\n", "function A() { return <Label>R&D.</Label>; }\n"]
        {
            let p = parse_source(Path::new("x/a.tsx"), "x/a.tsx", src.to_string()).unwrap();
            assert!(!p.has_error, "{src:?} tree: {}", p.tree.root_node().to_sexp());
        }
    }
}
