//! The two git-derived scopes: a pull request's working tree against its base
//! (`--base`) and the commits since a deploy tag (`--since`). Paths come back
//! from git relative to the repository top level, which may sit above the root
//! Locrin was pointed at, so every path is re-rooted here.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Context;
use locrin_core::parse::rel_path;
use locrin_core::walk::canonical_root;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffScope {
    Base(String),
    Since(String),
}

/// Every git process this module runs, with its output pinned to English.
///
/// `show_at` decides whether a path was absent by reading git's stderr, and both
/// messages it matches are translated ones: under a German or Japanese locale
/// git prints the translation, the match fails, and a pull request that adds a
/// file would abort the run instead of reporting the file as new. `LC_ALL=C`
/// (with `LANGUAGE` removed, since it overrides `LC_ALL` for messages) makes
/// every call here answer in the language the code reads.
fn git_command(dir: &Path) -> Command {
    let mut c = Command::new("git");
    c.current_dir(dir).env("LC_ALL", "C").env_remove("LANGUAGE");
    c
}

fn git(dir: &Path, args: &[&str]) -> anyhow::Result<Vec<u8>> {
    let out = git_command(dir).args(args).output().context("running git; is it installed and on PATH?")?;
    if !out.status.success() {
        anyhow::bail!("git {} failed: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(out.stdout)
}

fn nul_separated(bytes: &[u8]) -> Vec<String> {
    bytes.split(|b| *b == 0).filter(|s| !s.is_empty()).map(|s| String::from_utf8_lossy(s).into_owned()).collect()
}

/// Refuses a ref shaped like a git option, before any git process starts.
///
/// A ref is data, and git reads an argument beginning with `-` as one of its own
/// options: `--since=--output=<path>` reached `git diff` as `--output`, wrote the
/// diff to that path and let the check print PASS. Every invocation in this
/// module also puts `--end-of-options` in front of the ref, so a refactor that
/// loses this guard still cannot let a ref be read as an option.
fn checked_ref(scope: &DiffScope) -> anyhow::Result<&str> {
    let r = match scope {
        DiffScope::Base(r) | DiffScope::Since(r) => r.as_str(),
    };
    if r.starts_with('-') {
        anyhow::bail!("ref may not start with '-': {r}");
    }
    Ok(r)
}

/// The repository's top level, which may sit above the root Locrin was pointed
/// at. Every git invocation runs there and every path git prints is relative to
/// it.
fn top_level(root: &Path) -> anyhow::Result<PathBuf> {
    let top = String::from_utf8(git(root, &["rev-parse", "--show-toplevel"])?)?.trim().to_string();
    Ok(canonical_root(Path::new(&top)))
}

/// The revision a diff scope compares against: the merge base for `--base`,
/// because a pull request answers for its own commits and not for what landed on
/// the base branch since it forked, and the ref itself for `--since`, which is a
/// deployment gate over an exact range.
///
/// This is what `changed_files` diffs against and what the previous version of a
/// file is read from, so the two can never disagree about which commit "before"
/// means.
pub fn base_rev(root: &Path, scope: &DiffScope) -> anyhow::Result<String> {
    let r = checked_ref(scope)?;
    match scope {
        DiffScope::Base(_) => {
            let top = top_level(root)?;
            Ok(String::from_utf8(git(&top, &["merge-base", "--end-of-options", r, "HEAD"])?)?.trim().to_string())
        }
        DiffScope::Since(_) => Ok(r.to_string()),
    }
}

/// The text of `rel` as it was at `rev`, or None when it was not there.
///
/// This is how a diff scope learns what a file used to say without the index
/// having any memory of that version: a fresh CI clone has an empty index, and
/// the pull request's base commit is the only "before" it has. The path handed to
/// git is relative to the repository top level, and `--end-of-options` keeps a
/// revision shaped like an option from being read as one.
///
/// None means the run may treat the file as new: git said the path is not in
/// that revision. Any other failure is an error, because a git that cannot be
/// run, or a revision that does not exist, is not the same as a file that did not
/// exist yet and must not be reported as one. A blob that is not valid UTF-8 is
/// also None: it is not a file this engine can parse, so there is nothing to
/// compare against.
///
/// `root` must be the canonical repository root, as `changed_files` requires:
/// `top_level` returns a canonical path, and `rel_path` re-roots `root.join(rel)`
/// against it, so a root that is not canonical yields a path git does not know.
pub fn show_at(root: &Path, rev: &str, rel: &str) -> anyhow::Result<Option<String>> {
    if rev.starts_with('-') {
        anyhow::bail!("revision may not start with '-': {rev}");
    }
    let top = top_level(root)?;
    let from_top = rel_path(&top, &root.join(rel));
    let spec = format!("{rev}:{from_top}");
    let out = git_command(&top)
        .args(["show", "--end-of-options", &spec])
        .output()
        .context("running git; is it installed and on PATH?")?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        // The two ways git says "that path is not in that revision". Anything
        // else (an unknown revision, a broken repository) is a real failure.
        // Both messages are translated by git, which is why `git_command` pins
        // the locale to C.
        if err.contains("does not exist") || err.contains("exists on disk, but not in") {
            return Ok(None);
        }
        anyhow::bail!("git show {spec} failed: {}", err.trim());
    }
    Ok(String::from_utf8(out.stdout).ok())
}

pub fn changed_files(root: &Path, scope: &DiffScope) -> anyhow::Result<Vec<String>> {
    checked_ref(scope)?;
    let top = top_level(root)?;
    let rev = base_rev(&top, scope)?;
    let names = match scope {
        DiffScope::Base(_) => {
            let mut v = nul_separated(&git(
                &top,
                &["diff", "--name-only", "--diff-filter=ACMR", "-z", "--end-of-options", &rev],
            )?);
            v.extend(nul_separated(&git(&top, &["ls-files", "--others", "--exclude-standard", "-z"])?));
            v
        }
        DiffScope::Since(_) => nul_separated(&git(
            &top,
            &["diff", "--name-only", "--diff-filter=ACMR", "-z", "--end-of-options", &rev, "HEAD"],
        )?),
    };
    let mut out: Vec<String> = names
        .into_iter()
        .filter_map(|n| {
            let abs = top.join(&n);
            (abs.is_file() && abs.starts_with(root)).then(|| rel_path(root, &abs))
        })
        .collect();
    out.sort();
    out.dedup();
    Ok(out)
}
