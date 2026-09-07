use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    TypeScript,
    Tsx,
    JavaScript,
}

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
            _ => None,
        }
    }

    pub fn grammar(&self) -> tree_sitter::Language {
        match self {
            Language::TypeScript | Language::JavaScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Language::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Language::TypeScript => "typescript",
            Language::Tsx => "tsx",
            Language::JavaScript => "javascript",
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
        assert_eq!(Language::from_path(Path::new("a/b.py")), None);
        assert_eq!(Language::from_path(Path::new("a/README.md")), None);
    }
}
