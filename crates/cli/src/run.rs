use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::Context;
use locrin_core::baseline::Baseline;
use locrin_core::config::Config;
use locrin_core::finding::{Finding, Verdict};
use locrin_core::index::{content_hash, Index};
use locrin_core::lang::Language;
use locrin_core::parse::{parse_source, rel_path, ParsedFile};
use locrin_core::symbols;
use locrin_core::walk::{canonical_root, source_files, WalkOptions};
use locrin_rules::{run_all, RuleContext};

pub struct Options {
    pub root: PathBuf,
    pub paths: Vec<PathBuf>,
    pub changed_only: bool,
    pub json: bool,
}

struct Indexed {
    files: Vec<ParsedFile>,
    changed: usize,
}

fn candidate_files(root: &Path, paths: &[PathBuf], config: &Config) -> anyhow::Result<Vec<PathBuf>> {
    if paths.is_empty() {
        let opts = WalkOptions { excludes: config.excludes.clone() };
        return source_files(root, &opts);
    }
    let mut out = Vec::new();
    for p in paths {
        let abs = if p.is_absolute() { p.clone() } else { root.join(p) };
        if abs.is_dir() {
            let opts = WalkOptions { excludes: config.excludes.clone() };
            out.extend(source_files(&abs, &opts)?);
        } else if Language::from_path(&abs).is_some() {
            out.push(abs);
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

/// Reads and hashes every candidate, parses the changed ones (or all when `parse_all`),
/// updates the index, and returns the parsed files that are in scope.
///
/// A file that is not valid UTF-8 is not a source file this engine can reason
/// about, so it is reported once and skipped rather than aborting the run: one
/// stray binary blob with a `.ts` extension must not stop a repository check.
///
/// Pruning rows for files that have gone away is deliberately not done here.
/// Only a caller knows whether the candidate list is the whole repository or an
/// explicitly named subset, and pruning on a subset would delete every other
/// file's row.
fn index_files(
    root: &Path,
    candidates: &[PathBuf],
    parse_all: bool,
    ix: &mut Index,
) -> anyhow::Result<Indexed> {
    let mut files = Vec::new();
    let mut changed = 0;
    for path in candidates {
        let rel = rel_path(root, path);
        let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        let source = match String::from_utf8(bytes) {
            Ok(s) => s,
            Err(_) => {
                eprintln!("warning: {rel} is not valid UTF-8; skipped");
                continue;
            }
        };
        let hash = content_hash(&source);
        let is_changed = ix.changed(&rel, &hash)?;
        if !is_changed && !parse_all {
            continue;
        }
        let Some(parsed) = parse_source(path, &rel, source) else { continue };
        let status = if parsed.has_error { "error" } else { "ok" };
        if parsed.has_error {
            eprintln!("warning: parse errors in {rel}; skipped");
        }
        if is_changed {
            changed += 1;
            ix.upsert_file(&rel, parsed.language.as_str(), &hash, status)?;
            let syms = symbols::extract(&parsed);
            symbols::store(ix, &parsed, &syms)?;
        }
        files.push(parsed);
    }
    Ok(Indexed { files, changed })
}

fn full_findings(root: &Path, opts: &Options) -> anyhow::Result<(Vec<Finding>, Config)> {
    let config = Config::load(root)?;
    let candidates = candidate_files(root, &opts.paths, &config)?;
    let mut ix = Index::open(root)?;
    let parse_all = !opts.changed_only;
    let indexed = index_files(root, &candidates, parse_all, &mut ix)?;
    // Only a whole-repository pass knows the full set of files that still
    // exist, so only a whole-repository pass may delete rows.
    if opts.paths.is_empty() && !opts.changed_only {
        let present: Vec<String> = candidates.iter().map(|p| rel_path(root, p)).collect();
        ix.remove_missing(&present)?;
    }
    let ctx = RuleContext { files: &indexed.files, config: &config };
    Ok((run_all(&ctx), config))
}

pub fn check(opts: &Options) -> anyhow::Result<Verdict> {
    let started = Instant::now();
    // The walker returns canonical absolute paths, so the root that `rel_path`
    // strips has to be canonical too or nothing would strip.
    let root = canonical_root(&opts.root);
    let (findings, _) = full_findings(&root, opts)?;
    let baseline = Baseline::load(&root)?;
    let findings = baseline.filter(findings);
    Ok(Verdict::from_findings(findings, started.elapsed().as_millis()))
}

pub fn scan(root: &Path) -> anyhow::Result<(usize, usize)> {
    let root = canonical_root(root);
    let config = Config::load(&root)?;
    let candidates = candidate_files(&root, &[], &config)?;
    let mut ix = Index::open(&root)?;
    let indexed = index_files(&root, &candidates, false, &mut ix)?;
    let present: Vec<String> = candidates.iter().map(|p| rel_path(&root, p)).collect();
    ix.remove_missing(&present)?;
    Ok((candidates.len(), indexed.changed))
}

/// Snapshots every current finding into the baseline, ignoring whatever the
/// baseline already holds: `create` is a fresh line in the sand, not a merge.
pub fn baseline_create(root: &Path) -> anyhow::Result<usize> {
    let root = canonical_root(root);
    let opts = Options { root: root.clone(), paths: vec![], changed_only: false, json: false };
    let (findings, _) = full_findings(&root, &opts)?;
    let mut b = Baseline::default();
    for f in &findings {
        b.accept(f, "baseline", "locrin");
    }
    b.save(&root)?;
    Ok(findings.len())
}

/// Accepts one current finding by id. Returns false when no finding in the
/// current check carries that id, so the caller can say so rather than writing
/// an entry that suppresses nothing.
pub fn baseline_accept(root: &Path, id: &str, reason: &str) -> anyhow::Result<bool> {
    let root = canonical_root(root);
    let opts = Options { root: root.clone(), paths: vec![], changed_only: false, json: false };
    let (findings, _) = full_findings(&root, &opts)?;
    let mut b = Baseline::load(&root)?;
    let Some(f) = findings.iter().find(|f| f.id == id) else { return Ok(false) };
    let author = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "unknown".into());
    b.accept(f, reason, &author);
    b.save(&root)?;
    Ok(true)
}
