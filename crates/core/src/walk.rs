use std::path::{Path, PathBuf};

use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::WalkBuilder;

use crate::lang::Language;

pub const DEFAULT_EXCLUDES: &[&str] = &[
    "**/node_modules/**",
    "**/dist/**",
    "**/build/**",
    "**/coverage/**",
    "**/.expo/**",
    "**/android/**",
    "**/ios/**",
    "**/.git/**",
    "**/.locrin-cache/**",
];

#[derive(Debug, Default, Clone)]
pub struct WalkOptions {
    pub excludes: Vec<String>,
}

fn build_globset(extra: &[String]) -> anyhow::Result<GlobSet> {
    let mut b = GlobSetBuilder::new();
    for g in DEFAULT_EXCLUDES.iter().map(|s| s.to_string()).chain(extra.iter().cloned()) {
        b.add(Glob::new(&g)?);
    }
    Ok(b.build()?)
}

/// Walk `root` and return every supported source file, sorted, as absolute paths.
///
pub fn source_files(root: &Path, opts: &WalkOptions) -> anyhow::Result<Vec<PathBuf>> {
    let excludes = build_globset(&opts.excludes)?;
    let mut out = Vec::new();
    for entry in WalkBuilder::new(root).hidden(false).git_ignore(true).build() {
        let entry = entry?;
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let path = entry.path();
        let rel = path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/");
        if excludes.is_match(&rel) {
            continue;
        }
        if Language::from_path(path).is_none() {
            continue;
        }
        out.push(path.to_path_buf());
    }
    out.sort();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mini")
    }

    #[test]
    fn finds_source_files_and_skips_ignored_and_declarations() {
        let files = source_files(&fixture(), &WalkOptions::default()).unwrap();
        let names: Vec<String> = files
            .iter()
            .map(|p| p.strip_prefix(fixture()).unwrap().to_string_lossy().replace('\\', "/"))
            .collect();
        assert_eq!(names, vec!["src/index.ts", "src/util.ts"]);
    }

    #[test]
    fn extra_excludes_apply() {
        let opts = WalkOptions { excludes: vec!["src/util.ts".into()] };
        let files = source_files(&fixture(), &opts).unwrap();
        assert_eq!(files.len(), 1);
    }
}
