use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::Context;
use locrin_core::baseline::Baseline;
use locrin_core::config::Config;
use locrin_core::entry::EntryPoints;
use locrin_core::finding::{Finding, Verdict};
use locrin_core::index::{content_hash, Index};
use locrin_core::indexer;
use locrin_core::lang::Language;
use locrin_core::parse::{parse_source, rel_path, ParsedFile};
use locrin_core::resolve::Resolver;
use locrin_core::walk::{canonical_path, canonical_root, source_files, WalkOptions};
use locrin_rules::{run_all, RuleContext};
use rayon::prelude::*;

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

/// A candidate that has been read and judged, waiting to be parsed.
struct Pending {
    path: PathBuf,
    rel: String,
    hash: String,
    is_changed: bool,
    source: String,
}

/// The files named on the command line, canonical and inside the root, or None
/// when nothing was named. A directory expands to the walked files beneath it,
/// so the config's excludes still apply inside it; a file is taken as named,
/// excluded or not, because naming a file is an instruction.
fn explicit_files(root: &Path, paths: &[PathBuf], walked: &[PathBuf]) -> anyhow::Result<Option<Vec<PathBuf>>> {
    if paths.is_empty() {
        return Ok(None);
    }
    let mut out = Vec::new();
    for p in paths {
        let abs = if p.is_absolute() { p.clone() } else { root.join(p) };
        // Canonicalise before anything else: `src/../src/dirty.ts` and
        // `src/dirty.ts` are one file, and only one of them may reach the index
        // or a finding id.
        let canon = canonical_path(&abs).with_context(|| format!("no such path: {}", p.display()))?;
        if !canon.starts_with(root) {
            anyhow::bail!("path is outside the repository root: {}", p.display());
        }
        if canon.is_dir() {
            out.extend(walked.iter().filter(|f| f.starts_with(&canon)).cloned());
        } else if Language::from_path(&canon).is_some() {
            out.push(canon);
        }
    }
    out.sort();
    out.dedup();
    Ok(Some(out))
}

/// Reads and hashes every candidate, parses the ones that changed, are in scope,
/// or all of them when `parse_all`, records the changed ones when `record`, and
/// returns the parsed files.
///
/// A file that is not valid UTF-8 is not a source file this engine can reason
/// about, so it is reported once and skipped rather than aborting the run.
///
/// `record` is what separates a run that observes the repository from one that
/// merely reads it. The baseline commands read every file to find the finding
/// they were asked about; if they also stamped the hashes they saw, an edit made
/// between two commands would look already-seen and the next `--changed` check
/// would skip it. Only `check` and `scan` are entitled to move the watermark.
///
/// A recording run owns the whole write side of the index: it opens one
/// transaction, records, prunes the files that left the repository, and commits
/// once. Pruning lives here rather than in the callers so that the transaction
/// has a single scope, and so a run that fails part way commits nothing at all.
fn index_files(
    root: &Path,
    candidates: &[PathBuf],
    parse_all: bool,
    scope: Option<&HashSet<String>>,
    record: bool,
    resolver: &Resolver,
    ix: &mut Index,
) -> anyhow::Result<Indexed> {
    let mut files = Vec::new();
    let mut changed = 0;
    let mut recorded: HashSet<String> = HashSet::new();
    let mut present: Vec<String> = Vec::with_capacity(candidates.len());
    let mut any_new = false;
    if record {
        ix.begin()?;
    }
    // Reading and deciding is cheap and touches the index, so it stays here, on
    // the one thread that owns the connection. Parsing is neither, so it goes to
    // the pool below.
    let mut pending: Vec<Pending> = Vec::new();
    for path in candidates {
        let rel = rel_path(root, path);
        present.push(rel.clone());
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
        let in_scope = scope.is_some_and(|s| s.contains(&rel));
        if !is_changed && !parse_all && !in_scope {
            continue;
        }
        pending.push(Pending { path: path.clone(), rel, hash, is_changed, source });
    }
    // Parsing is the single largest cost of a cold run and needs nothing but the
    // source text, so it runs across the pool. `collect` keeps candidate order,
    // which is what the recording pass below and the returned `files` rely on.
    let parsed: Vec<(String, bool, Option<ParsedFile>)> = pending
        .into_par_iter()
        .map(|p| {
            let Pending { path, rel, hash, is_changed, source } = p;
            let file = parse_source(&path, &rel, source);
            (hash, is_changed, file)
        })
        .collect();
    for (hash, is_changed, file) in parsed {
        let Some(parsed) = file else { continue };
        if parsed.has_error {
            eprintln!("warning: parse errors in {}; excluded from rules", parsed.rel);
        }
        if is_changed {
            changed += 1;
            if record {
                any_new |= ix.file_hash(&parsed.rel)?.is_none();
                indexer::record(ix, &parsed, &hash, resolver)?;
                recorded.insert(parsed.rel.clone());
            }
        }
        files.push(parsed);
    }
    // A file that leaves the repository leaves its importers pointing at nothing:
    // `Index::remove_missing` marks those edges unresolved without touching the
    // importers, which are unchanged and so are never re-indexed. When the file
    // comes back the edges would stay unresolved and the graph rules would call
    // the returned file dead. So whenever a run records a file the index had not
    // seen before, re-record the importers that still hold an unresolved edge.
    if record && any_new {
        let by_rel: HashMap<String, &PathBuf> = candidates.iter().map(|p| (rel_path(root, p), p)).collect();
        for rel in ix.files_with_unresolved_edges()? {
            if recorded.contains(&rel) {
                continue;
            }
            let Some(path) = by_rel.get(&rel) else { continue };
            let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
            let Ok(source) = String::from_utf8(bytes) else { continue };
            let hash = content_hash(&source);
            let Some(parsed) = parse_source(path, &rel, source) else { continue };
            indexer::record(ix, &parsed, &hash, resolver)?;
            if !files.iter().any(|f| f.rel == rel) {
                files.push(parsed);
            }
        }
    }
    // The candidate list is the whole repository on every run, so pruning rows
    // for files that went away is safe whenever the run is recording.
    if record {
        ix.remove_missing(&present)?;
        ix.commit()?;
    }
    Ok(Indexed { files, changed })
}

/// Every current finding for a run. `record` decides whether the pass is allowed
/// to leave its mark on the index; see `index_files`.
///
/// The walk is always the whole repository, whatever was named on the command
/// line: the resolver has to know every file to resolve an import, and the
/// graph rules answer for the repository, not for a subset. Named paths narrow
/// what is parsed and what is reported, never what is indexed.
fn full_findings(root: &Path, opts: &Options, record: bool) -> anyhow::Result<Vec<Finding>> {
    let config = Config::load(root)?;
    let walked = source_files(root, &WalkOptions { excludes: config.excludes.clone() })?;
    let explicit = explicit_files(root, &opts.paths, &walked)?;
    let mut candidates = walked;
    if let Some(e) = &explicit {
        candidates.extend(e.iter().cloned());
        candidates.sort();
        candidates.dedup();
    }
    let rels: Vec<String> = candidates.iter().map(|p| rel_path(root, p)).collect();
    let scope: Option<HashSet<String>> = explicit.as_ref().map(|e| e.iter().map(|p| rel_path(root, p)).collect());
    let resolver = Resolver::new(root, rels.iter().cloned().collect());
    let mut ix = Index::open(root)?;
    // A whole-repository check parses everything because, until the findings
    // cache lands (plan 2 part B), that is the only way to run the file rules
    // over every file. A named path or `--changed` parses only what it must.
    let parse_all = scope.is_none() && !opts.changed_only;
    let indexed = index_files(root, &candidates, parse_all, scope.as_ref(), record, &resolver, &mut ix)?;
    let entries = EntryPoints::detect(root, &config.entry_points)?;
    let ctx = RuleContext { files: &indexed.files, config: &config, index: &ix, entries: &entries };
    let mut findings = run_all(&ctx)?;
    if let Some(scope) = &scope {
        findings.retain(|f| scope.contains(&f.file));
    } else if opts.changed_only {
        let changed: HashSet<&str> = indexed.files.iter().map(|f| f.rel.as_str()).collect();
        findings.retain(|f| changed.contains(f.file.as_str()));
    }
    Ok(findings)
}

pub fn check(opts: &Options) -> anyhow::Result<Verdict> {
    let started = Instant::now();
    // The walker returns canonical absolute paths, so the root that `rel_path`
    // strips has to be canonical too or nothing would strip.
    let root = canonical_root(&opts.root);
    let findings = full_findings(&root, opts, true)?;
    let baseline = Baseline::load(&root)?;
    let findings = baseline.filter(findings);
    Ok(Verdict::from_findings(findings, started.elapsed().as_millis()))
}

pub fn scan(root: &Path) -> anyhow::Result<(usize, usize)> {
    let root = canonical_root(root);
    let config = Config::load(&root)?;
    let candidates = source_files(&root, &WalkOptions { excludes: config.excludes.clone() })?;
    let rels: Vec<String> = candidates.iter().map(|p| rel_path(&root, p)).collect();
    let resolver = Resolver::new(&root, rels.iter().cloned().collect());
    let mut ix = Index::open(&root)?;
    let indexed = index_files(&root, &candidates, false, None, true, &resolver, &mut ix)?;
    Ok((candidates.len(), indexed.changed))
}

/// Snapshots every current finding into the baseline, ignoring whatever the
/// baseline already holds: `create` is a fresh line in the sand, not a merge.
pub fn baseline_create(root: &Path) -> anyhow::Result<usize> {
    let root = canonical_root(root);
    let opts = Options { root: root.clone(), paths: vec![], changed_only: false, json: false };
    let findings = full_findings(&root, &opts, false)?;
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
    let findings = full_findings(&root, &opts, false)?;
    let mut b = Baseline::load(&root)?;
    let Some(f) = findings.iter().find(|f| f.id == id) else { return Ok(false) };
    let author = std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_else(|_| "unknown".into());
    b.accept(f, reason, &author);
    b.save(&root)?;
    Ok(true)
}
