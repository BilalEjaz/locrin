//! The two git-derived scopes: a pull request's working tree against its base
//! (`--base`) and the commits since a deploy tag (`--since`). Paths come back
//! from git relative to the repository top level, which may sit above the root
//! Locrin was pointed at, so every path is re-rooted here.

use std::path::Path;
use std::process::Command;

use anyhow::Context;
use locrin_core::parse::rel_path;
use locrin_core::walk::canonical_root;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffScope {
    Base(String),
    Since(String),
}

fn git(dir: &Path, args: &[&str]) -> anyhow::Result<Vec<u8>> {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .context("running git; is it installed and on PATH?")?;
    if !out.status.success() {
        anyhow::bail!("git {} failed: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(out.stdout)
}

fn nul_separated(bytes: &[u8]) -> Vec<String> {
    bytes.split(|b| *b == 0).filter(|s| !s.is_empty()).map(|s| String::from_utf8_lossy(s).into_owned()).collect()
}

pub fn changed_files(root: &Path, scope: &DiffScope) -> anyhow::Result<Vec<String>> {
    // A ref is data, and git reads an argument beginning with `-` as one of its
    // own options: `--since=--output=<path>` reached `git diff` as `--output`,
    // wrote the diff to that path and let the check print PASS. So a ref shaped
    // like an option is refused before any git process starts, and every
    // invocation below puts `--end-of-options` in front of the ref as well, so a
    // refactor that loses this guard still cannot let a ref be read as an option.
    let r = match scope {
        DiffScope::Base(r) | DiffScope::Since(r) => r,
    };
    if r.starts_with('-') {
        anyhow::bail!("ref may not start with '-': {r}");
    }
    let top = String::from_utf8(git(root, &["rev-parse", "--show-toplevel"])?)?.trim().to_string();
    let top = canonical_root(Path::new(&top));
    let names = match scope {
        DiffScope::Base(r) => {
            let mb = String::from_utf8(git(&top, &["merge-base", "--end-of-options", r, "HEAD"])?)?.trim().to_string();
            let mut v = nul_separated(&git(
                &top,
                &["diff", "--name-only", "--diff-filter=ACMR", "-z", "--end-of-options", &mb],
            )?);
            v.extend(nul_separated(&git(&top, &["ls-files", "--others", "--exclude-standard", "-z"])?));
            v
        }
        DiffScope::Since(r) => nul_separated(&git(
            &top,
            &["diff", "--name-only", "--diff-filter=ACMR", "-z", "--end-of-options", r, "HEAD"],
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
