use std::path::Path;

use crate::config::Languages;

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
            // `.pyi` is left out for the reason `.d.ts` is: a stub file declares
            // signatures and holds no bodies, so it is not the code that runs and
            // every rule reading a body would be reporting on a shadow of it.
            "py" => Some(Language::Python),
            _ => None,
        }
    }

    /// Whether this language is one the run reads, given what the repository
    /// asked for in `[languages]`.
    ///
    /// The JavaScript family is always on: it is what the engine is, and there is
    /// no flag to turn it off. PHP and Python are read only when the repository
    /// has opted in, so the gate sits here rather than in each of the walker, the
    /// explicit-path list and the rules, which would each have to remember it.
    pub fn enabled(&self, langs: &Languages) -> bool {
        match self {
            Language::TypeScript | Language::Tsx | Language::JavaScript => true,
            Language::Php => langs.php,
            Language::Python => langs.python,
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
        // A `.pyi` stub is Python's `.d.ts`: signatures with no bodies, so every
        // rule that reads a body would report on a file that runs nothing.
        assert_eq!(Language::from_path(Path::new("a/b.pyi")), None);
        assert_eq!(Language::from_path(Path::new("a/b.rb")), None);
        assert_eq!(Language::from_path(Path::new("a/README.md")), None);
    }

    #[test]
    fn detects_php_and_python() {
        assert_eq!(Language::from_path(Path::new("app/Http/Kernel.php")), Some(Language::Php));
        assert_eq!(Language::from_path(Path::new("views/x.phtml")), Some(Language::Php));
        assert_eq!(Language::from_path(Path::new("bot/main.py")), Some(Language::Python));
        assert_eq!(Language::Php.as_str(), "php");
        assert_eq!(Language::Python.as_str(), "python");
        assert_eq!(JS_FAMILY.len(), 3);
        assert_eq!(ALL.len(), 5);
    }

    /// The JavaScript family is the engine's own language and is never gated.
    /// PHP and Python are read only once the repository has asked for them.
    #[test]
    fn php_and_python_are_enabled_only_by_the_config() {
        let off = Languages::default();
        for lang in JS_FAMILY {
            assert!(lang.enabled(&off), "{lang:?} must not need a flag");
        }
        assert!(!Language::Php.enabled(&off));
        assert!(!Language::Python.enabled(&off));

        let php_only = Languages { php: true, python: false };
        assert!(Language::Php.enabled(&php_only));
        assert!(!Language::Python.enabled(&php_only));

        let both = Languages { php: true, python: true };
        assert!(Language::Php.enabled(&both));
        assert!(Language::Python.enabled(&both));
    }
}
