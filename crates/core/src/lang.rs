use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    TypeScript,
    Tsx,
    JavaScript,
    Php,
    Python,
}

/// The three languages that share the TypeScript and TSX grammars, and with them
/// every node kind the JavaScript-shaped rules and the import and symbol readers
/// are written against. A rule that has not been taught PHP or Python asks for
/// this set rather than for [`ALL`].
pub const JS_FAMILY: &[Language] = &[Language::TypeScript, Language::Tsx, Language::JavaScript];

/// Every language the engine parses.
pub const ALL: &[Language] =
    &[Language::TypeScript, Language::Tsx, Language::JavaScript, Language::Php, Language::Python];

impl Language {
    pub fn from_path(path: &Path) -> Option<Language> {
        let name = path.file_name()?.to_str()?;
        if name.ends_with(".d.ts") || name.ends_with(".d.mts") || name.ends_with(".d.cts") {
            return None;
        }
        match path.extension()?.to_str()? {
            "ts" | "mts" | "cts" => Some(Language::TypeScript),
            "tsx" => Some(Language::Tsx),
            "js" | "jsx" | "mjs" | "cjs" => Some(Language::JavaScript),
            "php" | "phtml" => Some(Language::Php),
            "py" | "pyi" => Some(Language::Python),
            _ => None,
        }
    }

    /// JavaScript is parsed with the TSX grammar, not the TypeScript grammar.
    /// React projects routinely put JSX in plain `.js` and `.jsx` files, and the
    /// TypeScript grammar rejects JSX, so those files would come back with
    /// `has_error = true` and be silently skipped by every rule. The TSX grammar
    /// accepts the same JavaScript constructs plus JSX, and it avoids pulling in a
    /// third grammar crate.
    pub fn grammar(&self) -> tree_sitter::Language {
        match self {
            Language::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Language::Tsx | Language::JavaScript => tree_sitter_typescript::LANGUAGE_TSX.into(),
            // `LANGUAGE_PHP`, not `LANGUAGE_PHP_ONLY`: a `.php` file opens in
            // HTML and switches into PHP at `<?php`, and only the full grammar
            // reads that prefix.
            Language::Php => tree_sitter_php::LANGUAGE_PHP.into(),
            Language::Python => tree_sitter_python::LANGUAGE.into(),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Language::TypeScript => "typescript",
            Language::Tsx => "tsx",
            Language::JavaScript => "javascript",
            Language::Php => "php",
            Language::Python => "python",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn detects_supported_extensions() {
        assert_eq!(Language::from_path(Path::new("a/b.ts")), Some(Language::TypeScript));
        assert_eq!(Language::from_path(Path::new("a/b.tsx")), Some(Language::Tsx));
        assert_eq!(Language::from_path(Path::new("a/b.js")), Some(Language::JavaScript));
        assert_eq!(Language::from_path(Path::new("a/b.mjs")), Some(Language::JavaScript));
        assert_eq!(Language::from_path(Path::new("a/b.cjs")), Some(Language::JavaScript));
        assert_eq!(Language::from_path(Path::new("a/b.jsx")), Some(Language::JavaScript));
    }

    #[test]
    fn skips_declarations_and_unknown() {
        assert_eq!(Language::from_path(Path::new("a/b.d.ts")), None);
        assert_eq!(Language::from_path(Path::new("a/b.d.mts")), None);
        assert_eq!(Language::from_path(Path::new("a/b.d.cts")), None);
        assert_eq!(Language::from_path(Path::new("a/b.rb")), None);
        assert_eq!(Language::from_path(Path::new("a/README.md")), None);
    }

    #[test]
    fn detects_php_and_python() {
        assert_eq!(Language::from_path(Path::new("app/Http/Kernel.php")), Some(Language::Php));
        assert_eq!(Language::from_path(Path::new("views/x.phtml")), Some(Language::Php));
        assert_eq!(Language::from_path(Path::new("bot/main.py")), Some(Language::Python));
        assert_eq!(Language::from_path(Path::new("bot/types.pyi")), Some(Language::Python));
        assert_eq!(Language::Php.as_str(), "php");
        assert_eq!(Language::Python.as_str(), "python");
        assert_eq!(JS_FAMILY.len(), 3);
        assert_eq!(ALL.len(), 5);
    }
}
