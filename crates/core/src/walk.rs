use std::path::{Component, Path, PathBuf, Prefix, MAIN_SEPARATOR};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::{WalkBuilder, WalkState};

use crate::config::Languages;
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
    /// The languages the repository has asked for beyond the JavaScript family.
    /// All false by default, which is the answer for a caller that has no config
    /// to hand: a walk cannot start reading PHP because it forgot to ask.
    pub languages: Languages,
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

/// How many workers the walk spreads itself over.
///
/// Capped because the walk is bound by the filesystem rather than by the CPU:
/// past a handful of workers the extra threads queue on the same disk instead of
/// finding more work.
fn walk_threads() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).min(8)
}

/// Take a lock held only for pushes and never across a fallible call, so a
/// poisoned lock means a worker panicked and the data behind it is still whole.
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
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

/// The absolute, symlink-resolved form of `path`, spelled the way the walker
/// spells the paths it yields.
///
/// Errors when the path cannot be resolved, which on every supported platform
/// means it does not exist. Callers that must compare a caller-supplied path
/// against walked paths need this rather than [`canonical_root`]'s silent
/// fallback: an unresolved path that is quietly kept as typed would compare
/// unequal to the same file's walked spelling.
pub fn canonical_path(path: &Path) -> std::io::Result<PathBuf> {
    std::fs::canonicalize(path).map(strip_verbatim_prefix)
}

/// The absolute form of `root`, falling back to the caller's path if it cannot be resolved.
pub fn canonical_root(root: &Path) -> PathBuf {
    canonical_path(root).unwrap_or_else(|_| root.to_path_buf())
}

/// Walk `root` and return every supported source file, sorted, as absolute paths.
///
pub fn source_files(root: &Path, opts: &WalkOptions) -> anyhow::Result<Vec<PathBuf>> {
    Ok(walk_with_stats(root, opts)?.0)
}

/// Walk `root` and return every file it holds, source or not, sorted, as
/// absolute paths.
///
/// The excludes and the gitignore rules are the walk's own, so this is the same
/// tree [`source_files`] sees without the language filter on the end of it. A
/// run narrowed to a named directory needs it: naming a directory names every
/// file under it, and the files a rule reads without parsing (a migration, a
/// lockfile) are exactly the ones the language filter drops.
pub fn all_files(root: &Path, opts: &WalkOptions) -> anyhow::Result<Vec<PathBuf>> {
    Ok(walk_inner(root, opts, false)?.0)
}

/// Same as [`source_files`], plus counters describing how much of the tree was walked.
pub fn walk_with_stats(root: &Path, opts: &WalkOptions) -> anyhow::Result<(Vec<PathBuf>, WalkStats)> {
    walk_inner(root, opts, true)
}

/// The walk both public entry points share. `source_only` is the language
/// filter: set for the files the engine parses, clear for every file there is.
fn walk_inner(root: &Path, opts: &WalkOptions, source_only: bool) -> anyhow::Result<(Vec<PathBuf>, WalkStats)> {
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

    builder.threads(walk_threads());

    let out = Mutex::new(Vec::new());
    let visited = AtomicUsize::new(0);
    // The sequential walk aborted on the first bad entry. The pool cannot return
    // early, so the first error is parked here and returned once the walk stops.
    let failure: Mutex<Option<ignore::Error>> = Mutex::new(None);

    builder.build_parallel().run(|| {
        let out = &out;
        let visited = &visited;
        let failure = &failure;
        let excludes = Arc::clone(&excludes);
        let root = root.clone();
        Box::new(move |result| {
            let entry = match result {
                Ok(entry) => entry,
                Err(err) => {
                    let mut slot = lock(failure);
                    if slot.is_none() {
                        *slot = Some(err);
                    }
                    return WalkState::Quit;
                }
            };
            visited.fetch_add(1, Ordering::Relaxed);
            if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                return WalkState::Continue;
            }
            let path = entry.path();
            let rel = rel_of(path, &root);
            if excludes.is_match(&rel) {
                return WalkState::Continue;
            }
            // An unsupported extension and a language the repository has not
            // asked for are the same answer here: not a file this run parses.
            if source_only && !Language::from_path(path).is_some_and(|l| l.enabled(&opts.languages)) {
                return WalkState::Continue;
            }
            // The lock is taken once per matching file, never while reading the
            // directory, so the workers contend for it only briefly.
            lock(out).push(path.to_path_buf());
            WalkState::Continue
        })
    });

    if let Some(err) = lock(&failure).take() {
        return Err(err.into());
    }

    let mut out = out.into_inner().unwrap_or_else(|e| e.into_inner());
    out.sort();
    let stats = WalkStats { visited: visited.load(Ordering::Relaxed) };
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

    /// The listing a narrowed run needs: the same tree, the same excludes, and
    /// the files the language filter drops kept.
    #[test]
    fn all_files_keeps_what_the_language_filter_drops_and_still_prunes_the_excludes() {
        let files = all_files(&fixture(), &WalkOptions::default()).unwrap();
        let names = rel_names(&files);
        assert!(names.contains(&"package.json".to_string()), "{names:?}");
        assert!(names.contains(&"src/types.d.ts".to_string()), "a declaration file is a file: {names:?}");
        assert!(names.contains(&"src/index.ts".to_string()), "{names:?}");
        assert!(!names.iter().any(|n| n.starts_with("node_modules/")), "the excludes still prune: {names:?}");
        assert!(!names.iter().any(|n| n.starts_with("dist/")), "the gitignore still applies: {names:?}");
    }

    /// The walk is where a language the repository has not asked for stops: a
    /// `.php` file under a TypeScript repository is not read, not parsed and not
    /// reported on until `[languages]` says so.
    #[test]
    fn languages_the_config_has_not_asked_for_are_not_walked() {
        let dir = std::env::temp_dir().join(format!("locrin-walk-langs-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/a.ts"), "export const a = 1;\n").unwrap();
        std::fs::write(dir.join("src/b.php"), "<?php\n").unwrap();
        std::fs::write(dir.join("src/c.py"), "x = 1\n").unwrap();

        let root = canonical_root(&dir);
        let names = |opts: &WalkOptions| -> Vec<String> {
            let files = source_files(&dir, opts).unwrap();
            files.iter().map(|p| rel_of(p, &root)).collect()
        };

        assert_eq!(names(&WalkOptions::default()), vec!["src/a.ts"]);

        let php = WalkOptions { languages: Languages { php: true, python: false }, ..Default::default() };
        assert_eq!(names(&php), vec!["src/a.ts", "src/b.php"]);

        let both = WalkOptions { languages: Languages { php: true, python: true }, ..Default::default() };
        assert_eq!(names(&both), vec!["src/a.ts", "src/b.php", "src/c.py"]);

        // The listing that skips the language filter keeps every file either way.
        let all = all_files(&dir, &WalkOptions::default()).unwrap();
        assert_eq!(all.len(), 3, "{all:?}");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn extra_excludes_apply() {
        let all = source_files(&fixture(), &WalkOptions::default()).unwrap();
        let opts = WalkOptions { excludes: vec!["src/util.ts".into()], ..Default::default() };
        let files = source_files(&fixture(), &opts).unwrap();
        assert_eq!(files.len(), all.len() - 1);
        assert!(!files.iter().any(|p| p.ends_with("util.ts")));
    }

    /// The walk runs across a thread pool. This pins its output to the output of
    /// a single-threaded [`ignore::Walk`] carrying the identical filters, so a
    /// change to the pool cannot quietly change which files the engine sees.
    #[test]
    fn walking_across_the_pool_matches_a_sequential_walk() {
        let root = canonical_root(&fixture());
        let excludes = build_globset(&[]).unwrap();

        let filter_root = root.clone();
        let filter_excludes = build_globset(&[]).unwrap();
        let mut builder = WalkBuilder::new(&root);
        builder.hidden(false).git_ignore(true);
        builder.filter_entry(move |entry| {
            if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                return true;
            }
            let rel = rel_of(entry.path(), &filter_root);
            if rel.is_empty() {
                return true;
            }
            !filter_excludes.is_match(format!("{rel}/"))
        });

        let mut sequential = Vec::new();
        let mut visited = 0usize;
        for entry in builder.build() {
            let entry = entry.unwrap();
            visited += 1;
            if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                continue;
            }
            let path = entry.path();
            if excludes.is_match(rel_of(path, &root)) {
                continue;
            }
            if !Language::from_path(path).is_some_and(|l| l.enabled(&Languages::default())) {
                continue;
            }
            sequential.push(path.to_path_buf());
        }
        sequential.sort();
        assert!(!sequential.is_empty(), "the fixture must yield files for this comparison to mean anything");

        let (parallel, stats) = walk_with_stats(&fixture(), &WalkOptions::default()).unwrap();
        assert_eq!(parallel, sequential, "the pooled walk must return exactly the sequential walk's files");
        assert_eq!(stats.visited, visited, "the pooled walk must visit exactly as many entries");
    }

    #[test]
    fn excluded_directories_are_pruned_not_only_filtered() {
        let (all, all_stats) = walk_with_stats(&fixture(), &WalkOptions::default()).unwrap();
        assert!(all.iter().any(|p| p.ends_with("x.ts")), "fixture must contain the prunable file");

        let opts = WalkOptions { excludes: vec!["**/.git_fake/**".into()], ..Default::default() };
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
