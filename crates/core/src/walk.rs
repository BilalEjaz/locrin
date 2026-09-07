use std::path::{Component, Path, PathBuf, Prefix, MAIN_SEPARATOR};
use std::sync::Arc;

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

/// Counters describing the work a walk actually did.
#[derive(Debug, Default, Clone)]
pub struct WalkStats {
    /// Number of directory entries the walker yielded, before any filtering.
    ///
    /// Pruned directories are never yielded, so this drops when an exclude
    /// matches a directory rather than only its files.
    pub visited: usize,
}

fn build_globset(extra: &[String]) -> anyhow::Result<GlobSet> {
    let mut b = GlobSetBuilder::new();
    for g in DEFAULT_EXCLUDES.iter().map(|s| s.to_string()).chain(extra.iter().cloned()) {
        b.add(Glob::new(&g)?);
    }
    Ok(b.build()?)
}

fn rel_of(path: &Path, root: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace(MAIN_SEPARATOR, "/")
}

/// Drop the Windows verbatim prefix that `canonicalize` adds, so results stay readable.
///
/// Verbatim UNC paths are left alone: there is no shorter spelling of them.
fn strip_verbatim_prefix(path: PathBuf) -> PathBuf {
    let mut comps = path.components();
    if let Some(Component::Prefix(prefix)) = comps.next() {
        if let Prefix::VerbatimDisk(letter) = prefix.kind() {
            let rest: PathBuf = comps.filter(|c| !matches!(c, Component::RootDir)).collect();
            return PathBuf::from(format!("{}:{}{}", letter as char, MAIN_SEPARATOR, rest.display()));
        }
    }
    path
}

/// The absolute form of `root`, falling back to the caller's path if it cannot be resolved.
pub fn canonical_root(root: &Path) -> PathBuf {
    match std::fs::canonicalize(root) {
        Ok(p) => strip_verbatim_prefix(p),
        Err(_) => root.to_path_buf(),
    }
}

/// Walk `root` and return every supported source file, sorted, as absolute paths.
///
pub fn source_files(root: &Path, opts: &WalkOptions) -> anyhow::Result<Vec<PathBuf>> {
    Ok(walk_with_stats(root, opts)?.0)
}

/// Same as [`source_files`], plus counters describing how much of the tree was walked.
pub fn walk_with_stats(root: &Path, opts: &WalkOptions) -> anyhow::Result<(Vec<PathBuf>, WalkStats)> {
    let root = canonical_root(root);
    let excludes = Arc::new(build_globset(&opts.excludes)?);

    let mut builder = WalkBuilder::new(&root);
    builder.hidden(false).git_ignore(true);
    let filter_excludes = Arc::clone(&excludes);
    let filter_root = root.clone();
    builder.filter_entry(move |entry| {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            return true;
        }
        let rel = rel_of(entry.path(), &filter_root);
        if rel.is_empty() {
            return true;
        }
        // A trailing slash lets directory globs such as `**/node_modules/**`
        // match the directory itself, so the walker never descends into it.
        !filter_excludes.is_match(format!("{rel}/"))
    });

    let mut out = Vec::new();
    let mut stats = WalkStats::default();
    for entry in builder.build() {
        let entry = entry?;
        stats.visited += 1;
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let path = entry.path();
        let rel = rel_of(path, &root);
        if excludes.is_match(&rel) {
            continue;
        }
        if Language::from_path(path).is_none() {
            continue;
        }
        out.push(path.to_path_buf());
    }
    out.sort();
    Ok((out, stats))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mini")
    }

    fn rel_names(files: &[PathBuf]) -> Vec<String> {
        let root = canonical_root(&fixture());
        files.iter().map(|p| rel_of(p, &root)).collect()
    }

    #[test]
    fn finds_source_files_and_skips_ignored_and_declarations() {
        let files = source_files(&fixture(), &WalkOptions::default()).unwrap();
        assert!(files.iter().all(|p| p.is_absolute()), "walk must return absolute paths: {files:?}");
        assert_eq!(rel_names(&files), vec![".git_fake/objects/x.ts", "src/index.ts", "src/util.ts"]);
    }

    #[test]
    fn extra_excludes_apply() {
        let all = source_files(&fixture(), &WalkOptions::default()).unwrap();
        let opts = WalkOptions { excludes: vec!["src/util.ts".into()] };
        let files = source_files(&fixture(), &opts).unwrap();
        assert_eq!(files.len(), all.len() - 1);
        assert!(!files.iter().any(|p| p.ends_with("util.ts")));
    }

    #[test]
    fn excluded_directories_are_pruned_not_only_filtered() {
        let (all, all_stats) = walk_with_stats(&fixture(), &WalkOptions::default()).unwrap();
        assert!(all.iter().any(|p| p.ends_with("x.ts")), "fixture must contain the prunable file");

        let opts = WalkOptions { excludes: vec!["**/.git_fake/**".into()] };
        let (files, stats) = walk_with_stats(&fixture(), &opts).unwrap();
        assert!(!files.iter().any(|p| p.ends_with("x.ts")), "excluded file must not be returned");
        assert!(
            stats.visited < all_stats.visited,
            "excluded directory must be pruned, not descended: {} visited with the exclude, {} without",
            stats.visited,
            all_stats.visited
        );
    }
}
