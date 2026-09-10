//! The git-derived scopes: a pull request's working tree against its base
//! (`--base`), the commits since a deploy tag (`--since`), and what is staged for
//! the next commit (the pre-commit hook). Paths come back from git relative to
//! the repository top level, which may sit above the root Locrin was pointed at,
//! so every path is re-rooted here.

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

/// Whether `root` is inside a git work tree that has at least one commit, which
/// is what a diff against HEAD needs. False for no git, no repository, or an
/// unborn branch.
///
/// This is a question, not a step of a run, so every way of failing is one
/// answer: a caller asks it to choose a scope and has another scope to fall back
/// to, and an error here would turn "there is nothing to diff against" into a
/// hook that reports a problem instead of checking the code.
pub fn has_head(root: &Path) -> bool {
    git_command(root).args(["rev-parse", "--verify", "--quiet", "HEAD"]).output().is_ok_and(|out| out.status.success())
}

/// Paths staged for the next commit (added, copied, modified, renamed), relative
/// to the repository top level, in git's order. Works on an unborn branch, where
/// git diffs the index against the empty tree.
///
/// This is the pre-commit hook's scope, and it is the index and not the working
/// tree: what is about to become a commit is what the gate answers for.
///
/// Unlike `changed_files` this does not drop a path whose file is not on disk. A
/// deletion is already excluded by the filter git applies, but a file staged and
/// then removed from the working tree still comes back here, and it is the
/// caller that decides what to do about one.
pub fn staged_files(root: &Path) -> anyhow::Result<Vec<String>> {
    let top = top_level(root)?;
    let names = nul_separated(&git(&top, &["diff", "--cached", "--name-only", "-z", "--diff-filter=ACMR"])?);
    Ok(names
        .into_iter()
        .filter_map(|n| {
            // The top level may sit above the root Locrin was pointed at, so a
            // staged path outside that root is not this run's business.
            let abs = top.join(&n);
            abs.starts_with(root).then(|| rel_path(root, &abs))
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// git as these tests drive it. The identity and the signing setting are
    /// pinned so a machine configured for a real developer still commits, and
    /// line endings are left alone so the bytes committed are the bytes written.
    fn run_git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.autocrlf=false",
            ])
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }

    /// An empty repository at a canonical root, which is what every path
    /// comparison in this module is written against.
    fn repo() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = canonical_root(dir.path());
        run_git(&root, &["init", "-q"]);
        (dir, root)
    }

    /// The first commit a repository ever makes is staged on an unborn branch,
    /// and a pre-commit hook has to survive it: there is no HEAD to diff the
    /// index against, so git diffs it against the empty tree.
    #[test]
    fn staged_files_lists_only_what_is_staged_on_an_unborn_branch() {
        let (_dir, root) = repo();
        std::fs::write(root.join("a.ts"), "export const a = 1;\n").unwrap();
        std::fs::write(root.join("b.ts"), "export const b = 2;\n").unwrap();
        run_git(&root, &["add", "a.ts"]);
        // b.ts is in the working tree and not in the index, which is exactly the
        // difference this function exists to draw.
        assert_eq!(staged_files(&root).unwrap(), vec!["a.ts".to_string()]);
    }

    #[test]
    fn staged_files_names_a_subdirectory_path_from_the_top_level() {
        let (_dir, root) = repo();
        std::fs::create_dir(root.join("src")).unwrap();
        std::fs::write(root.join("src/a.ts"), "export const a = 1;\n").unwrap();
        run_git(&root, &["add", "."]);
        run_git(&root, &["commit", "-qm", "base"]);
        std::fs::write(root.join("src/a.ts"), "export const a = 2;\n").unwrap();
        run_git(&root, &["add", "src/a.ts"]);
        // One spelling, with forward slashes on every platform, which is the
        // spelling a finding and the index both use.
        assert_eq!(staged_files(&root).unwrap(), vec!["src/a.ts".to_string()]);
    }

    /// A file staged and then removed from the working tree is still a staged
    /// change, so it is still listed. Dropping it here would hide the decision
    /// from the caller, which is the one place it can be made.
    #[test]
    fn staged_files_keeps_a_staged_path_whose_file_is_gone() {
        let (_dir, root) = repo();
        std::fs::write(root.join("a.ts"), "export const a = 1;\n").unwrap();
        run_git(&root, &["add", "a.ts"]);
        std::fs::remove_file(root.join("a.ts")).unwrap();
        assert_eq!(staged_files(&root).unwrap(), vec!["a.ts".to_string()]);
    }

    #[test]
    fn staged_files_is_empty_when_nothing_is_staged() {
        let (_dir, root) = repo();
        std::fs::write(root.join("a.ts"), "export const a = 1;\n").unwrap();
        assert!(staged_files(&root).unwrap().is_empty());
    }
}
