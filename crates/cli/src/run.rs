use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::Context;
use locrin_core::baseline::Baseline;
use locrin_core::cache::{self, CachedFile};
use locrin_core::config::Config;
use locrin_core::edges;
use locrin_core::entry::EntryPoints;
use locrin_core::finding::{Finding, Verdict};
use locrin_core::index::{content_hash, file_stat, Index};
use locrin_core::indexer;
use locrin_core::lang::Language;
use locrin_core::parse::{parse_source, rel_path, ParsedFile};
use locrin_core::resolve::Resolver;
use locrin_core::walk::{canonical_path, canonical_root, source_files, WalkOptions};
use locrin_rules::{file_rules, graph_rules, run_file_rules, run_rules, RuleContext};
use rayon::prelude::*;

pub struct Options {
    pub root: PathBuf,
    pub paths: Vec<PathBuf>,
    pub changed_only: bool,
    pub json: bool,
    /// The scope taken from git rather than from the command line: a pull
    /// request's working tree against its base, or the commits since a tag.
    pub diff: Option<crate::git::DiffScope>,
}

/// What one pass over the repository produced.
struct Run {
    findings: Vec<Finding>,
    files: usize,
    changed: usize,
}

/// The two things that decide whether a cached row may be served: the hash of
/// the config the rules run under, and which file rules are on. A row written
/// under a different config, or under a rule set that did not include one of the
/// rules this run wants, is not this run's answer.
struct CacheKey<'a> {
    config_hash: &'a str,
    enabled: &'a [&'static str],
}

/// What a run kept from reading one file: the hash of the bytes it read and the
/// stat those bytes had. The two belong to the same read, which is why they
/// travel together.
struct FileRead {
    hash: String,
    stat: (i64, i64),
}

struct Indexed {
    files: Vec<ParsedFile>,
    /// The file-rule findings of every unchanged file the cache could answer for.
    served: Vec<Finding>,
    /// The files whose content changed this run.
    changed: HashSet<String>,
    /// The targets the changed files' edges pointed at *before* this run
    /// re-recorded them. Dropping an import is what makes an export dead in the
    /// file that is no longer imported, and once the edge is gone that file is
    /// no longer a neighbour of anything: the link has to be captured while it
    /// still exists or a narrowed run could never report the finding it caused.
    before: HashSet<String>,
    /// The read behind every file in `files`, for the findings cache written
    /// once the rules have run.
    reads: HashMap<String, FileRead>,
}

/// A candidate that has been read and judged, waiting to be parsed.
struct Pending {
    path: PathBuf,
    rel: String,
    hash: String,
    is_changed: bool,
    source: String,
    /// The size and modification time taken before the read, so it belongs to
    /// the bytes in `source` and never to a later save. See
    /// [`indexer::record_with_stat`].
    stat: (i64, i64),
}

/// A candidate that has been through the parser, waiting to be recorded. `file`
/// is None when the path had no language the engine parses.
struct Parsed {
    hash: String,
    is_changed: bool,
    stat: (i64, i64),
    file: Option<ParsedFile>,
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

/// Reads a candidate and hashes the bytes it read, or None when those bytes are
/// not valid UTF-8. The caller decides what to say about that: the main pass
/// warns once, the repair pass below is silent because the main pass has already
/// spoken for the same file.
fn read_source(path: &Path) -> anyhow::Result<Option<(String, String)>> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let Ok(source) = String::from_utf8(bytes) else { return Ok(None) };
    let hash = content_hash(&source);
    Ok(Some((source, hash)))
}

/// The cached file-rule findings for a file whose content has not moved, when
/// the cache holds every enabled rule for exactly this content under exactly
/// this config. Anything less is a miss: serving part of a file's findings would
/// report the file as cleaner than it is.
fn serve(cached: &HashMap<String, CachedFile>, rel: &str, hash: &str, key: &CacheKey) -> Option<Vec<Finding>> {
    let entry = cached.get(rel)?;
    if entry.content_hash != hash || entry.config_hash != key.config_hash {
        return None;
    }
    let mut out = Vec::new();
    for rule in key.enabled {
        out.extend(entry.by_rule.get(*rule)?.iter().cloned());
    }
    Some(out)
}

/// The files whose edges touch any file in `set`, in either direction, in the
/// index as it is now.
fn neighbours(ix: &Index, set: &HashSet<String>) -> anyhow::Result<HashSet<String>> {
    let mut out = HashSet::new();
    for rel in set {
        out.extend(edges::from_file(ix, rel)?.into_iter().filter_map(|e| e.to_rel));
        out.extend(edges::dependents(ix, rel)?);
    }
    Ok(out)
}

/// Reads and hashes every candidate, serves the unchanged ones from the findings
/// cache, parses the rest, records the changed ones, and returns what the rules
/// need.
///
/// `report_all` is what separates a run that has to answer for the whole
/// repository from one that answers for a scope. When it is set, every candidate
/// is either parsed or served from the cache; when it is not, a candidate
/// outside the scope that has not changed is never opened.
///
/// A file that is not valid UTF-8 is not a source file this engine can reason
/// about, so it is reported once and skipped rather than aborting the run.
///
/// A run owns the whole write side of the index: it opens one transaction,
/// records, prunes the files that left the repository, and commits once. Pruning
/// lives here rather than in the callers so that the transaction has a single
/// scope, and so a run that fails part way commits nothing at all.
///
/// That single scope is also why the two ways a candidate can go wrong end
/// differently. A candidate that cannot be read (permission denied, deleted
/// between the walk and the read) aborts the run with exit 2 and, since the run
/// is one transaction, throws away every file this run had indexed so far: an
/// unreadable file is an environment problem the operator has to see, and an
/// index built while the engine could not see part of the repository would call
/// live code dead. A candidate that is not valid UTF-8 only warns and is
/// skipped: a binary blob carrying a source extension is a repository quirk the
/// engine tolerates, and the rest of the repository is still worth indexing.
fn index_files(
    root: &Path,
    candidates: &[PathBuf],
    report_all: bool,
    scope: Option<&HashSet<String>>,
    key: &CacheKey,
    resolver: &Resolver,
    ix: &mut Index,
) -> anyhow::Result<Indexed> {
    let cached = cache::load_all(ix)?;
    let mut files = Vec::new();
    let mut served: Vec<Finding> = Vec::new();
    let mut changed: HashSet<String> = HashSet::new();
    let mut before: HashSet<String> = HashSet::new();
    let mut reads: HashMap<String, FileRead> = HashMap::new();
    let mut present: Vec<String> = Vec::with_capacity(candidates.len());
    let mut any_new = false;
    ix.begin()?;
    // Reading and deciding is cheap and touches the index, so it stays here, on
    // the one thread that owns the connection. Parsing is neither, so it goes to
    // the pool below.
    let mut pending: Vec<Pending> = Vec::new();
    for path in candidates {
        let rel = rel_path(root, path);
        present.push(rel.clone());
        let in_scope = scope.is_some_and(|s| s.contains(&rel));
        // The stat is taken before the read, always, because it is stored beside
        // the hash of the bytes this run reads. A stat taken after the read
        // would belong to whatever is on disk by then: a save landing in between
        // would be recorded as the new stat beside the old hash, and every later
        // narrowed run would match that stat and skip the file for good.
        let stat = file_stat(path);
        // Size and modification time answer "did this file change?" without
        // opening the file, which is what keeps a check from reading the whole
        // repository. The stat may only say "certainly unchanged": anything else
        // falls through to the read and the content hash below, so a file
        // touched without being edited is still recognised as unchanged.
        if !in_scope && ix.unchanged_by_stat(&rel, stat.0, stat.1)? {
            if !report_all {
                continue;
            }
            // The stat says these are the bytes the index recorded, so the hash
            // the index holds is this file's hash: the cache can answer for it
            // without the file ever being opened. A miss falls through to the
            // read, because a full report cannot leave the file out.
            let hash = ix.file_hash(&rel)?.unwrap_or_default();
            if let Some(hit) = serve(&cached, &rel, &hash, key) {
                served.extend(hit);
                continue;
            }
        }
        let Some((source, hash)) = read_source(path)? else {
            eprintln!("warning: {rel} is not valid UTF-8; skipped");
            continue;
        };
        let is_changed = ix.changed(&rel, &hash)?;
        if is_changed {
            // The edges this file has right now belong to the version about to
            // be replaced. See `Indexed::before`.
            before.extend(edges::from_file(ix, &rel)?.into_iter().filter_map(|e| e.to_rel));
        } else {
            // The file was touched but not edited: the stat disagreed and the
            // hash overruled it. Nothing is recorded for such a file, so the
            // stale stat has to be replaced here or this run's read and hash are
            // repeated by every run after it. A full run refreshes it too: the
            // stat may have moved without the bytes moving (a checkout, a
            // formatter, a stash pop), so store the current one and the next
            // run can skip the read.
            ix.refresh_stat(&rel, stat.0, stat.1)?;
            if !in_scope {
                if !report_all {
                    continue;
                }
                if let Some(hit) = serve(&cached, &rel, &hash, key) {
                    served.extend(hit);
                    continue;
                }
            }
        }
        pending.push(Pending { path: path.clone(), rel, hash, is_changed, source, stat });
    }
    // Parsing is the single largest cost of a cold run and needs nothing but the
    // source text, so it runs across the pool. `collect` keeps candidate order,
    // which is what the recording pass below and the returned `files` rely on.
    let parsed: Vec<Parsed> = pending
        .into_par_iter()
        .map(|p| {
            let Pending { path, rel, hash, is_changed, source, stat } = p;
            let file = parse_source(&path, &rel, source);
            Parsed { hash, is_changed, stat, file }
        })
        .collect();
    for Parsed { hash, is_changed, stat, file } in parsed {
        let Some(parsed) = file else { continue };
        if parsed.has_error {
            eprintln!("warning: parse errors in {}; excluded from rules", parsed.rel);
        }
        if is_changed {
            changed.insert(parsed.rel.clone());
            any_new |= ix.file_hash(&parsed.rel)?.is_none();
            indexer::record_with_stat(ix, &parsed, &hash, resolver, stat)?;
            // Whatever the cache holds for this file describes bytes that are
            // gone. The rows this run writes replace them, and until it does the
            // absence is the safe state: a miss costs a parse, a stale hit
            // reports findings from a version nobody can see.
            cache::clear(ix, &parsed.rel)?;
        }
        reads.insert(parsed.rel.clone(), FileRead { hash, stat });
        files.push(parsed);
    }
    // A file that leaves the repository leaves its importers pointing at nothing:
    // `Index::remove_missing` marks those edges unresolved without touching the
    // importers, which are unchanged and so are never re-indexed. When the file
    // comes back the edges would stay unresolved and the graph rules would call
    // the returned file dead. So whenever a run records a file the index had not
    // seen before, re-record the importers that still hold an unresolved edge.
    if any_new {
        let by_rel: HashMap<String, &PathBuf> = candidates.iter().map(|p| (rel_path(root, p), p)).collect();
        for rel in ix.files_with_unresolved_edges()? {
            if changed.contains(&rel) {
                continue;
            }
            // An importer this run already parsed is re-recorded from what is in
            // hand: reading and parsing it again would build the same tree.
            if let Some((file, read)) = files.iter().find(|f| f.rel == rel).zip(reads.get(&rel)) {
                indexer::record_with_stat(ix, file, &read.hash, resolver, read.stat)?;
                continue;
            }
            let Some(path) = by_rel.get(&rel) else { continue };
            // Before the read, for the same reason as the pass above.
            let stat = file_stat(path);
            let Some((source, hash)) = read_source(path)? else { continue };
            let Some(parsed) = parse_source(path, &rel, source) else { continue };
            indexer::record_with_stat(ix, &parsed, &hash, resolver, stat)?;
            reads.insert(rel.clone(), FileRead { hash, stat });
            files.push(parsed);
        }
    }
    // The candidate list is the whole repository on every run, so pruning rows
    // for files that went away is always safe.
    ix.remove_missing(&present)?;
    ix.commit()?;
    // A file the repair pass had to read is in `files` now, and the rules are
    // about to produce its findings fresh. Anything the cache served for it
    // would be the same answer a second time, so the parse wins.
    let parsed_rels: HashSet<&str> = files.iter().map(|f| f.rel.as_str()).collect();
    served.retain(|f| !parsed_rels.contains(f.file.as_str()));
    Ok(Indexed { files, served, changed, before, reads })
}

/// One row per (file this run parsed, enabled file rule), so the next run can
/// have those findings without parsing anything. An empty vector is written too:
/// "checked, clean" is an answer, and a rule with no row for a file is a miss.
///
/// This is a second transaction, deliberately not the run's. The rules run
/// between the two, and holding the index's write lock across them would make a
/// concurrent locrin (an editor hook beside a terminal) wait out the whole rule
/// pass rather than the indexing. The split costs nothing: the rows for a
/// changed file were cleared inside the run's transaction, so a crash in between
/// leaves that file with no cached rows at all, which the next run reads as a
/// miss and answers by parsing it.
fn write_cache(ix: &mut Index, indexed: &Indexed, fresh: &[Finding], key: &CacheKey) -> anyhow::Result<()> {
    let mut by_file_rule: HashMap<(&str, &str), Vec<Finding>> = HashMap::new();
    for f in fresh {
        by_file_rule.entry((f.file.as_str(), f.rule.as_str())).or_default().push(f.clone());
    }
    ix.begin()?;
    for file in &indexed.files {
        let Some(read) = indexed.reads.get(&file.rel) else { continue };
        for rule in key.enabled {
            let found = by_file_rule.get(&(file.rel.as_str(), *rule)).map(Vec::as_slice).unwrap_or_default();
            cache::put(ix, &file.rel, &read.hash, key.config_hash, rule, found)?;
        }
    }
    ix.commit()
}

/// One pass over the repository: walk, index what must be indexed, run the file
/// rules over what was parsed and the graph rules over the index, serve every
/// other file's file-rule findings from the cache, and narrow the report to the
/// scope when there is one.
///
/// `record` decides whether the run is allowed to leave its mark on the
/// repository's index. Every run has to fill an index before it can report
/// anything: the graph rules answer by querying one, and on a fresh cache (every
/// CI job, every fresh clone) the repository's index is empty. So a non-recording
/// run does index the whole repository, into a throwaway in-memory database that
/// is dropped when the run ends. The repository's own index is never opened, so
/// the watermark it holds does not move: an edit made between `check` and
/// `baseline accept` is still unseen to the next `check --changed`, which is the
/// thing a baseline command must never swallow. For the same reason a
/// non-recording run writes no findings cache.
///
/// The walk is always the whole repository, whatever was named on the command
/// line: the resolver has to know every file to resolve an import, and the
/// graph rules answer for the repository, not for a subset. Named paths narrow
/// what is parsed and what is reported, never what is indexed.
///
/// Scope semantics: file-rule findings are reported for the scope exactly;
/// graph-rule findings are reported for the scope plus the files its edges touch,
/// before and after the re-indexing, because dropping an import from a scoped
/// file is what makes an export dead in a file outside it (spec 3.2).
fn pass(root: &Path, opts: &Options, record: bool) -> anyhow::Result<Run> {
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
    let mut scope: Option<HashSet<String>> = explicit.as_ref().map(|e| e.iter().map(|p| rel_path(root, p)).collect());
    // A diff-derived scope is a scope like any other: it narrows what is parsed
    // and what is reported, and everything downstream (file findings to the
    // scope, graph findings to the scope plus what its edges touch) already
    // knows what to do with one.
    if let Some(diff) = &opts.diff {
        let listed: HashSet<String> = crate::git::changed_files(root, diff)?.into_iter().collect();
        let walked: HashSet<&str> = rels.iter().map(|s| s.as_str()).collect();
        // Excludes apply to a diff-derived scope: generated code in a pull
        // request is still generated code.
        scope = Some(listed.into_iter().filter(|r| walked.contains(r.as_str())).collect());
    }
    let resolver = Resolver::new(root, rels.iter().cloned().collect());
    let mut ix = if record { Index::open(root)? } else { Index::open_in_memory()? };

    let rules = file_rules();
    let enabled: Vec<&'static str> =
        rules.iter().filter(|r| config.rule_enabled_or(r.id(), r.enabled_by_default())).map(|r| r.id()).collect();
    let config_hash = cache::config_hash(&config);
    let key = CacheKey { config_hash: &config_hash, enabled: &enabled };

    // A whole-repository check has to answer for every file, so each candidate is
    // either parsed or served from the cache. A named path or `--changed` answers
    // for its scope and leaves the rest unread. A run filling a throwaway index
    // has no choice but to parse everything: no cache was ever written for it.
    let report_all = !record || (scope.is_none() && !opts.changed_only);
    let indexed = index_files(root, &candidates, report_all, scope.as_ref(), &key, &resolver, &mut ix)?;

    let entries = EntryPoints::detect(root, &config.entry_points)?;
    // The file rules go across the pool, a file at a time: five rules over a
    // couple of thousand parsed files is the largest cost left in a cold run and
    // each file is independent of every other. The graph rules run every time,
    // over the whole index: they are SQL and a change anywhere can move their
    // answer. The borrow of `ix` ends inside this block, before the cache write
    // takes it mutably.
    let fresh = run_file_rules(&rules, &indexed.files, &config, &entries)?;
    let graph = {
        let ctx = RuleContext { files: &indexed.files, config: &config, index: Some(&ix), entries: &entries };
        run_rules(&graph_rules(), &ctx)?
    };
    if record {
        write_cache(&mut ix, &indexed, &fresh, &key)?;
    }

    // `--changed` takes the changed set as its scope.
    if scope.is_none() && opts.changed_only {
        scope = Some(indexed.changed.clone());
    }
    let files = candidates.len();
    let changed = indexed.changed.len();
    let mut findings = indexed.served;
    findings.extend(fresh);
    let mut graph = graph;
    if let Some(scope) = &scope {
        findings.retain(|f| scope.contains(&f.file));
        let mut wide = scope.clone();
        wide.extend(indexed.before);
        wide.extend(neighbours(&ix, scope)?);
        graph.retain(|f| wide.contains(&f.file));
    }
    findings.extend(graph);
    Ok(Run { findings, files, changed })
}

/// Every current finding for a run, with the index updated when `record`.
fn full_findings(root: &Path, opts: &Options, record: bool) -> anyhow::Result<Vec<Finding>> {
    Ok(pass(root, opts, record)?.findings)
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

/// Indexes the repository and warms the findings cache, so the first `check`
/// after it (a hook, say) pays for nothing but the files that changed since.
pub fn scan(root: &Path) -> anyhow::Result<(usize, usize)> {
    let root = canonical_root(root);
    let opts = Options { root: root.clone(), paths: vec![], changed_only: false, json: false, diff: None };
    let run = pass(&root, &opts, true)?;
    Ok((run.files, run.changed))
}

/// Snapshots every current finding into the baseline, ignoring whatever the
/// baseline already holds: `create` is a fresh line in the sand, not a merge.
pub fn baseline_create(root: &Path) -> anyhow::Result<usize> {
    let root = canonical_root(root);
    let opts = Options { root: root.clone(), paths: vec![], changed_only: false, json: false, diff: None };
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
    let opts = Options { root: root.clone(), paths: vec![], changed_only: false, json: false, diff: None };
    let findings = full_findings(&root, &opts, false)?;
    let mut b = Baseline::load(&root)?;
    let Some(f) = findings.iter().find(|f| f.id == id) else { return Ok(false) };
    let author = std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_else(|_| "unknown".into());
    b.accept(f, reason, &author);
    b.save(&root)?;
    Ok(true)
}
