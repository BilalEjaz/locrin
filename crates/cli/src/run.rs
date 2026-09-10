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
use locrin_core::previous::Previous;
use locrin_core::resolve::Resolver;
use locrin_core::walk::{all_files, canonical_path, canonical_root, source_files, WalkOptions};
use locrin_rules::{file_rules, graph_rules, rule_runs, run_file_rules, run_rules, RuleContext};
use rayon::prelude::*;

pub struct Options {
    pub root: PathBuf,
    pub paths: Vec<PathBuf>,
    pub changed_only: bool,
    pub json: bool,
    /// Never touch the network. See spec 9: the run serves the cached advisory
    /// snapshot or skips the rule that would have fetched, and never fails
    /// because a network call could not be made.
    pub offline: bool,
    /// The scope taken from git rather than from the command line: a pull
    /// request's working tree against its base, or the commits since a tag.
    pub diff: Option<crate::git::DiffScope>,
}

/// What one pass over the repository produced.
struct Run {
    findings: Vec<Finding>,
    files: usize,
    changed: usize,
    /// What the pass learned about the previous version of the files it reported
    /// for. The CLI needs only the findings; this exists so a test can check the
    /// capture without re-running the whole pipeline by hand, and so it is
    /// compiled only into the test build rather than carrying a dead-code
    /// suppression through production code.
    #[cfg(test)]
    previous: Previous,
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
    /// still exists or a `--changed` run could never report the finding it
    /// caused. "Changed" here means changed since the index's watermark, so only
    /// a run whose scope is that watermark may widen itself with this set. See
    /// the condition on `scope_is_watermark` in `pass`.
    ///
    /// Empty on every other run: only `--changed` may read it, and building it
    /// costs an edge query per changed file plus one per departed file. See
    /// `capture_before`.
    before: HashSet<String>,
    /// The read behind every file in `files`, for the findings cache written
    /// once the rules have run.
    reads: HashMap<String, FileRead>,
    /// What the index said about each changed file before this run overwrote it.
    /// A file with an entry has a known previous version; a file without one
    /// (never indexed, or unchanged and served from the cache) does not. See the
    /// capture in the recording pass below.
    previous: Previous,
}

/// How much of the repository one pass has to answer for, which is the one
/// question `index_files` cannot answer for itself.
struct Plan<'a> {
    /// Every candidate is either parsed or served from the findings cache. Set
    /// for a whole-repository check; clear for a run narrowed to a scope, which
    /// leaves everything outside it unread.
    report_all: bool,
    /// The files the run reports for, or None when it reports for everything.
    scope: Option<&'a HashSet<String>>,
    /// Whether anything will read `Indexed::before`. Only a `--changed` run may,
    /// so only a `--changed` run pays for the edge queries that build it.
    capture_before: bool,
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
    /// The cases this version of the file skips, empty for anything that is not
    /// a test file. Computed here rather than in the indexer because it is a
    /// tree walk and the indexer runs on the single index-connection thread.
    skipped: Vec<String>,
}

/// The paths named on the command line, in the two readings a run needs.
struct Explicit {
    /// The source files: what is parsed, and what file-rule findings are
    /// reported for.
    files: Vec<PathBuf>,
    /// Every existing file the command line named, before any language filter:
    /// the files named outright, and for a directory every file beneath it,
    /// source or not. A rule that answers for a file the engine does not parse
    /// can only learn that the run was asked about it from here, and a graph
    /// finding against such a file is kept on this authority rather than on the
    /// import graph's. `vulnerable-dependency` reads the lockfile and
    /// `supabase-table-without-rls` reads the SQL migrations; both are in no
    /// walk, no index and no import neighbourhood. See `lock_in_scope`,
    /// `rls_in_scope` and the graph `retain` in `pass`.
    raw: Vec<PathBuf>,
}

/// The files named on the command line, canonical and inside the root, or None
/// when nothing was named. A directory expands to the walked files beneath it,
/// so the config's excludes still apply inside it; a file is taken as named,
/// excluded or not, because naming a file is an instruction.
///
/// The two readings differ for a directory. `files` is what the engine parses
/// and reports file findings for, so it is the source files beneath it. `raw`
/// is what the run was *asked about*, so it is every file beneath it: naming a
/// directory names what is under it, and the files the rules read without
/// parsing (the lockfile, a SQL migration) are exactly the ones a language
/// filter drops. Left to the walk, `check .` would have answered for every file
/// in the repository except the ones those two rules read, which is not what
/// naming the root means.
///
/// The full listing is walked at most once and only when a directory is named,
/// which is the only case where it can say anything a named path does not. The
/// lockfile is added on top of it because a repository may have chosen to
/// gitignore its lockfile, and a file the walk skips is still a file the rule
/// reads.
fn explicit_files(
    root: &Path,
    paths: &[PathBuf],
    walked: &[PathBuf],
    lockfile_rel: Option<&str>,
    walk_opts: &WalkOptions,
) -> anyhow::Result<Option<Explicit>> {
    if paths.is_empty() {
        return Ok(None);
    }
    let lockfile = lockfile_rel.map(|rel| root.join(rel));
    let mut files = Vec::new();
    let mut raw = Vec::new();
    let mut everything: Option<Vec<PathBuf>> = None;
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
            // A directory names the files under it and not itself: nothing
            // reports against a directory, and the walk already applied the
            // config's excludes.
            files.extend(walked.iter().filter(|f| f.starts_with(&canon)).cloned());
            if everything.is_none() {
                everything = Some(all_files(root, walk_opts)?);
            }
            let all = everything.as_ref().expect("just filled");
            raw.extend(all.iter().filter(|f| f.starts_with(&canon)).cloned());
            if let Some(lock) = lockfile.as_ref().filter(|lock| lock.starts_with(&canon)) {
                raw.push(lock.clone());
            }
        } else {
            raw.push(canon.clone());
            if Language::from_path(&canon).is_some() {
                files.push(canon);
            }
        }
    }
    for v in [&mut files, &mut raw] {
        v.sort();
        v.dedup();
    }
    Ok(Some(Explicit { files, raw }))
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

/// The test cases `rel` skipped at `rev`, for the git-sourced half of the
/// previous snapshot.
///
/// A file that was not there at that revision, or that will not parse, skipped
/// nothing: everything in the version on disk is new, which is the answer a
/// newly added test file needs.
///
/// Only test files are read. The snapshot holds nothing else yet, and a `--base`
/// run against a large pull request would otherwise fetch and parse every changed
/// file a second time (they are all parsed at HEAD already) to learn nothing. A
/// later field that needs the whole previous file widens this.
///
/// None means "this revision has nothing trustworthy to say about the file", and
/// the caller records no entry for it rather than a wrong one. That is what a
/// base version with parse errors gives: tree-sitter recovers from a syntax
/// error by dropping nodes, so an `it.skip` that was there can be missing from
/// the extraction, and asserting the empty set from a recovered tree would call
/// an old skip a new one and would also overwrite whatever the index knew.
fn skipped_at(root: &Path, rev: &str, rel: &str) -> anyhow::Result<Option<HashSet<String>>> {
    if !locrin_core::testcases::is_test_file(rel) {
        return Ok(Some(HashSet::new()));
    }
    let Some(source) = crate::git::show_at(root, rev, rel)? else { return Ok(Some(HashSet::new())) };
    let Some(file) = parse_source(&root.join(rel), rel, source) else { return Ok(Some(HashSet::new())) };
    if file.has_error {
        return Ok(None);
    }
    Ok(Some(locrin_core::testcases::extract(&file).into_iter().filter(|c| c.skipped).map(|c| c.name).collect()))
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
///
/// The three fields of `Plan` are what the caller knows and this function
/// cannot; they travel together because each one is an answer to the same
/// question, how much of the repository this run has to answer for.
fn index_files(
    root: &Path,
    candidates: &[PathBuf],
    plan: &Plan,
    key: &CacheKey,
    resolver: &Resolver,
    ix: &mut Index,
) -> anyhow::Result<Indexed> {
    let Plan { report_all, scope, capture_before } = *plan;
    // Only a run that has to answer for the whole repository ever serves a
    // cached row: every `serve` below sits behind `report_all`. Loading and
    // deserialising every row for a narrowed run (a hook's `--changed`, a named
    // path, a `--base` against a pull request's merge base) would be a fixed
    // cost proportional to the repository, charged against the three hundred
    // milliseconds such a run is allowed.
    let cached = if report_all { cache::load_all(ix)? } else { HashMap::new() };
    let mut files = Vec::new();
    let mut served: Vec<Finding> = Vec::new();
    let mut changed: HashSet<String> = HashSet::new();
    let mut before: HashSet<String> = HashSet::new();
    let mut reads: HashMap<String, FileRead> = HashMap::new();
    let mut previous = Previous::default();
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
            if capture_before {
                before.extend(edges::from_file(ix, &rel)?.into_iter().filter_map(|e| e.to_rel));
            }
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
    //
    // The skipped-case walk goes here for the same reason: the recording pass
    // below owns the index connection and runs on one thread, so a tree walk it
    // did would be serial time nothing else can overlap.
    let parsed: Vec<Parsed> = pending
        .into_par_iter()
        .map(|p| {
            let Pending { path, rel, hash, is_changed, source, stat } = p;
            let file = parse_source(&path, &rel, source);
            let skipped = file.as_ref().map(locrin_core::testcases::skipped_names).unwrap_or_default();
            Parsed { hash, is_changed, stat, file, skipped }
        })
        .collect();
    for Parsed { hash, is_changed, stat, file, skipped } in parsed {
        let Some(parsed) = file else { continue };
        if parsed.has_error {
            eprintln!("warning: parse errors in {}; excluded from rules", parsed.rel);
        }
        if is_changed {
            changed.insert(parsed.rel.clone());
            // What the index holds right now describes the version this run is
            // about to replace, so it is the previous version and it has to be
            // read before the record below overwrites it. A file with no `files`
            // row has no previous version at all, which is not the same as one
            // whose previous version skipped nothing: no entry is recorded for
            // it, and a rule reads that as "everything here is new".
            let known = ix.file_hash(&parsed.rel)?.is_some();
            any_new |= !known;
            if known {
                let was = ix.skipped_tests(&parsed.rel)?;
                previous.skipped_tests.insert(parsed.rel.clone(), was);
            }
            indexer::record_with_stat(ix, &parsed, &hash, resolver, stat, &skipped)?;
            // Whatever the cache holds for this file describes bytes that are
            // gone. The rows this run writes replace them, and until it does the
            // absence is the safe state: a miss costs a parse, a stale hit
            // reports findings from a version nobody can see.
            cache::clear(ix, &parsed.rel)?;
        } else if ix.file_hash(&parsed.rel)?.is_some() {
            // An unchanged file that reached the parser is one a narrowed run
            // named (or one a full run could not serve from the cache), and the
            // rules are about to run over it. It is unchanged, so what the index
            // holds for it is both its stored version and its previous one, and
            // without an entry a rule that answers with a change reads it as
            // having no history and reports every skip in it as new, on every
            // run. The repair pass below captures the same thing for the same
            // reason.
            previous.skipped_tests.insert(parsed.rel.clone(), ix.skipped_tests(&parsed.rel)?);
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
    //
    // Every file this pass touches is unchanged, so what the index holds for it
    // is both its stored version and its previous one, and it has to be captured
    // here: the repair puts the file into `files`, the rules then run over it,
    // and without an entry a rule that answers with a change would read an
    // untouched file as having no known previous version and report every skip
    // in it as new.
    if any_new {
        let by_rel: HashMap<String, &PathBuf> = candidates.iter().map(|p| (rel_path(root, p), p)).collect();
        for rel in ix.files_with_unresolved_edges()? {
            if changed.contains(&rel) {
                continue;
            }
            // An importer this run already parsed is re-recorded from what is in
            // hand: reading and parsing it again would build the same tree.
            if let Some((file, read)) = files.iter().find(|f| f.rel == rel).zip(reads.get(&rel)) {
                previous.skipped_tests.insert(rel.clone(), ix.skipped_tests(&rel)?);
                let skipped = locrin_core::testcases::skipped_names(file);
                indexer::record_with_stat(ix, file, &read.hash, resolver, read.stat, &skipped)?;
                continue;
            }
            let Some(path) = by_rel.get(&rel) else { continue };
            // Before the read, for the same reason as the pass above.
            let stat = file_stat(path);
            let Some((source, hash)) = read_source(path)? else { continue };
            let Some(parsed) = parse_source(path, &rel, source) else { continue };
            previous.skipped_tests.insert(rel.clone(), ix.skipped_tests(&rel)?);
            let skipped = locrin_core::testcases::skipped_names(&parsed);
            indexer::record_with_stat(ix, &parsed, &hash, resolver, stat, &skipped)?;
            reads.insert(rel.clone(), FileRead { hash, stat });
            files.push(parsed);
        }
    }
    // A deletion is a change like any other, and the finding it causes lands
    // somewhere else: the file that was the last importer of an export leaves,
    // and the export is dead in a file this run never touched. The departed
    // file's edges are the only record of that link, and `remove_missing` is
    // about to delete them, so its targets are captured here while they still
    // exist. See `Indexed::before`.
    if capture_before {
        let kept: HashSet<&str> = present.iter().map(|s| s.as_str()).collect();
        for rel in ix.all_files()? {
            if kept.contains(rel.as_str()) {
                continue;
            }
            before.extend(edges::from_file(ix, &rel)?.into_iter().filter_map(|e| e.to_rel));
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
    Ok(Indexed { files, served, changed, before, reads, previous })
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
/// graph-rule findings are reported for the scope plus the files its edges touch
/// after the re-indexing, because an import from a scoped file is what keeps an
/// export alive in a file outside it (spec 3.2). `--changed`, whose scope is the
/// index's watermark, also reaches the targets those edges pointed at before the
/// re-indexing: dropping an import is what makes an export dead, and the edge has
/// to be captured before it is replaced. A scope that did not come from the
/// watermark does not get that widening; see the comment at the `retain` below.
///
/// The files the engine does not parse are the exception to all of that. The
/// lockfile and the SQL migrations are read by graph rules and reached by no
/// walk, no index and no import neighbourhood, so a scoped run answers for one
/// only when the scope names it outright: the run keeps a raw scope beside the
/// source-file scope, holding every existing file the scope named, and a graph
/// finding survives when the neighbourhood holds its file or the raw scope
/// does. A scope that names none of a rule's files drops that rule from the run
/// rather than paying for reads whose findings it would discard. See
/// `lock_in_scope` and `rls_in_scope`.
fn pass(root: &Path, opts: &Options, record: bool) -> anyhow::Result<Run> {
    let config = Config::load(root)?;
    let walk_opts = WalkOptions { excludes: config.excludes.clone() };
    let walked = source_files(root, &walk_opts)?;
    // Which file the advisory rule would answer for, found without reading it.
    // Located once and handed to everything that asks: the question costs a
    // `stat` per candidate lockfile and has one answer for the whole pass.
    let lockfile_rel = locrin_core::lockfile::locate(root);
    let explicit = explicit_files(root, &opts.paths, &walked, lockfile_rel, &walk_opts)?;
    let mut candidates = walked;
    if let Some(e) = &explicit {
        candidates.extend(e.files.iter().cloned());
        candidates.sort();
        candidates.dedup();
    }
    let rels: Vec<String> = candidates.iter().map(|p| rel_path(root, p)).collect();
    let mut scope: Option<HashSet<String>> =
        explicit.as_ref().map(|e| e.files.iter().map(|p| rel_path(root, p)).collect());
    // The same scope before the language filter: every path the run was pointed
    // at, source file or not. See `lock_in_scope` below.
    let mut raw_scope: Option<HashSet<String>> =
        explicit.as_ref().map(|e| e.raw.iter().map(|p| rel_path(root, p)).collect());
    // A diff-derived scope is a scope like any other: it narrows what is parsed
    // and what is reported, and everything downstream (file findings to the
    // scope, graph findings to the scope plus what its edges touch) already
    // knows what to do with one.
    if let Some(diff) = &opts.diff {
        let listed: Vec<String> = crate::git::changed_files(root, diff)?;
        let walked: HashSet<&str> = rels.iter().map(|s| s.as_str()).collect();
        // Excludes apply to a diff-derived scope: generated code in a pull
        // request is still generated code.
        scope = Some(listed.iter().filter(|r| walked.contains(r.as_str())).cloned().collect());
        raw_scope = Some(listed.into_iter().collect());
    }
    // Whether this run's scope reaches the two files a graph rule reads without
    // the engine parsing them: the lockfile and the SQL migrations.
    //
    // Neither is a source file, so neither is in a walk, in the index or in an
    // import neighbourhood: a scope can only contain one by naming it. Left to
    // the graph filter at the end of this function, every advisory and every
    // row-level-security finding on a scoped run was produced and then
    // discarded, after the rule had paid for its reads and, online, its
    // requests. So the rule is dropped from the run instead, and the findings it
    // does produce when its files *are* named are kept on the raw scope's
    // authority rather than on the neighbourhood's.
    //
    // `--changed` is the one scope that can name neither: that scope comes from
    // the index, which holds source files only, so an edit to a lockfile or to a
    // migration is invisible to it and neither rule ever runs. A
    // whole-repository run has no scope at all and is unchanged.
    let lock_in_scope = if opts.changed_only {
        false
    } else {
        raw_scope.as_ref().is_none_or(|raw| lockfile_rel.is_some_and(|rel| raw.contains(rel)))
    };
    let rls_in_scope = if opts.changed_only {
        false
    } else {
        raw_scope.as_ref().is_none_or(|raw| raw.iter().any(|rel| locrin_rules::supabase::rls::is_migration(rel)))
    };
    let resolver = Resolver::new(root, rels.iter().cloned().collect());
    let mut ix = if record { Index::open(root)? } else { Index::open_in_memory()? };

    let rules = file_rules();
    let enabled: Vec<&'static str> = rules.iter().filter(|r| rule_runs(r.as_ref(), &config)).map(|r| r.id()).collect();
    let config_hash = cache::config_hash(&config);
    let key = CacheKey { config_hash: &config_hash, enabled: &enabled };

    // A whole-repository check has to answer for every file, so each candidate is
    // either parsed or served from the cache. A named path or `--changed` answers
    // for its scope and leaves the rest unread. A run filling a throwaway index
    // has no choice but to parse everything: no cache was ever written for it.
    let report_all = !record || (scope.is_none() && !opts.changed_only);
    // `--changed` takes the changed set as its scope. That scope IS the index's
    // watermark, which is what licenses both the capture of the pre-record edges
    // below and the widening at the end of this function. Every other run reads
    // neither, so it does not pay to build them.
    let scope_is_watermark = scope.is_none() && opts.changed_only;
    let plan = Plan { report_all, scope: scope.as_ref(), capture_before: scope_is_watermark };
    let mut indexed = index_files(root, &candidates, &plan, &key, &resolver, &mut ix)?;
    // The index's memory of each changed file, captured before it was overwritten.
    let mut previous = std::mem::take(&mut indexed.previous);
    // For a diff scope git is the better witness and overrides it. The index
    // remembers the last run, which on a fresh CI clone is nothing at all and on
    // a warm one is whenever the developer last ran locrin; the base commit is
    // the "before" a pull request is actually judged against, and it is the same
    // commit `changed_files` diffed to build this scope.
    if let Some(diff) = &opts.diff {
        let rev = crate::git::base_rev(root, diff)?;
        for rel in scope.iter().flatten() {
            if let Some(was) = skipped_at(root, &rev, rel)? {
                previous.skipped_tests.insert(rel.clone(), was);
            }
        }
    }

    let entries = EntryPoints::detect(root, &config.entry_points)?;
    // The file rules go across the pool, a file at a time: five rules over a
    // couple of thousand parsed files is the largest cost left in a cold run and
    // each file is independent of every other. The graph rules run every time,
    // over the whole index: they are SQL and a change anywhere can move their
    // answer. The borrow of `ix` ends inside this block, before the cache write
    // takes it mutably.
    let base = RuleContext {
        files: &indexed.files,
        config: &config,
        index: None,
        entries: &entries,
        root,
        offline: opts.offline,
        previous: &previous,
    };
    let fresh = run_file_rules(&rules, &indexed.files, &base)?;
    let graph = {
        let ctx = RuleContext { index: Some(&ix), ..base };
        let mut rules = graph_rules();
        // Not a config decision and not the registry's business: this run could
        // not report what these rules would find, so it does not ask.
        if !lock_in_scope {
            rules.retain(|r| r.id() != "vulnerable-dependency");
        }
        if !rls_in_scope {
            rules.retain(|r| r.id() != "supabase-table-without-rls");
        }
        run_rules(&rules, &ctx)?
    };
    if record {
        write_cache(&mut ix, &indexed, &fresh, &key)?;
    }

    if scope_is_watermark {
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
        // `indexed.before` is derived from the index's watermark, not from this
        // run's scope: it holds the old forward targets of every file the index
        // considers changed or gone, wherever in the repository they are. Adding
        // it is right only when the scope IS that watermark, which is `--changed`
        // and nothing else. (A whole-repository run needs no widening at all:
        // `scope` is None there and every graph finding is reported.)
        //
        // For a named path and for a `--base`/`--since` diff the scope comes from
        // the command line or from git, so folding the watermark in would make
        // the verdict a function of run history: `check --base HEAD` on a warm
        // index would report a dead export caused by an uncommitted deletion, and
        // the identical command run again would print PASS, the first run having
        // consumed that deletion from the index. Those scopes therefore reach the
        // scope plus its neighbours after re-indexing and nothing else, so the
        // answer depends only on the tree and the ref.
        //
        // The trade: a pull request that deletes the last importer of an export
        // does not see that dead export in its own view. The next whole-repository
        // check reports it.
        if scope_is_watermark {
            wide.extend(indexed.before);
        }
        wide.extend(neighbours(&ix, scope)?);
        // A graph finding can name a file no neighbourhood contains, because
        // nothing imports it: the lockfile and a SQL migration are the two that
        // exist today. The raw scope is the run's record of what it was asked
        // about, source or not, so a finding against a file the scope named is
        // kept on that authority rather than on the graph's. `--changed` has no
        // raw scope at all, which is why it reports neither.
        let raw_scope = raw_scope.as_ref();
        graph.retain(|f| wide.contains(&f.file) || raw_scope.is_some_and(|raw| raw.contains(&f.file)));
    }
    findings.extend(graph);
    Ok(Run {
        findings,
        files,
        changed,
        #[cfg(test)]
        previous,
    })
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
pub fn scan(root: &Path, offline: bool) -> anyhow::Result<(usize, usize)> {
    let root = canonical_root(root);
    let opts = Options { root: root.clone(), paths: vec![], changed_only: false, json: false, offline, diff: None };
    let run = pass(&root, &opts, true)?;
    Ok((run.files, run.changed))
}

/// Snapshots every current finding into the baseline, ignoring whatever the
/// baseline already holds: `create` is a fresh line in the sand, not a merge.
///
/// `offline` is the same promise `check --offline` makes, and it belongs here
/// for the same reason: `create` runs every rule, `vulnerable-dependency`
/// included, so without it a repository drawing its first line in the sand on a
/// machine with no network waits out the advisory requests' timeouts.
pub fn baseline_create(root: &Path, offline: bool) -> anyhow::Result<usize> {
    let root = canonical_root(root);
    let opts = Options { root: root.clone(), paths: vec![], changed_only: false, json: false, offline, diff: None };
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
///
/// `offline` as in [`baseline_create`]: accepting one finding runs the whole
/// check that produced it.
pub fn baseline_accept(root: &Path, id: &str, reason: &str, offline: bool) -> anyhow::Result<bool> {
    let root = canonical_root(root);
    let opts = Options { root: root.clone(), paths: vec![], changed_only: false, json: false, offline, diff: None };
    let findings = full_findings(&root, &opts, false)?;
    let mut b = Baseline::load(&root)?;
    let Some(f) = findings.iter().find(|f| f.id == id) else { return Ok(false) };
    let author = std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_else(|_| "unknown".into());
    b.accept(f, reason, &author);
    b.save(&root)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    // The tests here set `LOCRIN_CACHE_DIR`, which is process-wide, so they
    // take turns on the lock the whole binary's tests share rather than one of
    // their own: a lock per module would not stop this module racing another.
    use crate::ENV_LOCK;

    /// The rules that answer with a change rather than a state need to know what
    /// the file used to say, and for a `--changed` run the index is where that
    /// comes from: it holds the last recorded version until this run replaces it.
    ///
    /// A first run has no previous version for anything, and reporting one would
    /// make every legacy skip in a repository look like it was introduced by
    /// whoever happened to run locrin first.
    #[test]
    fn a_changed_run_reads_what_the_previous_version_skipped() {
        let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        // The index path comes from the environment, which the lock above keeps
        // this test's for as long as it runs.
        std::env::set_var("LOCRIN_CACHE_DIR", dir.path().join(".cache"));
        let root = canonical_root(dir.path());
        std::fs::create_dir_all(root.join("src")).unwrap();
        let rel = "src/a.test.ts";
        let path = root.join(rel);
        std::fs::write(&path, "it.skip(\"one\", () => {});\nit(\"two\", () => {});\n").unwrap();

        let full =
            Options { root: root.clone(), paths: vec![], changed_only: false, json: false, offline: true, diff: None };
        let first = pass(&root, &full, true).unwrap();
        assert!(
            first.previous.skipped_tests.is_empty(),
            "a file the index had never seen has no previous version: {:?}",
            first.previous.skipped_tests
        );

        std::fs::write(&path, "it.skip(\"one\", () => {});\nit.skip(\"two\", () => {});\n").unwrap();
        let changed = Options { changed_only: true, ..full };
        let second = pass(&root, &changed, true).unwrap();
        assert_eq!(
            second.previous.skipped_tests.get(rel),
            Some(&HashSet::from(["one".to_string()])),
            "the run has to see the version it replaced, where only the first case was skipped"
        );

        std::env::remove_var("LOCRIN_CACHE_DIR");
    }

    /// A run that indexes a file it has never seen re-records the unchanged
    /// importers that still hold an unresolved edge, and those files then go to
    /// the rules like any other. They are unchanged, so the index still holds
    /// their previous version, and the pass has to capture it: without that, a
    /// pull request that adds one new file would make every skipped test in
    /// every unresolved importer look like it was skipped by this change.
    #[test]
    fn the_repair_pass_captures_the_previous_version_of_the_files_it_re_records() {
        let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("LOCRIN_CACHE_DIR", dir.path().join(".cache"));
        let root = canonical_root(dir.path());
        std::fs::create_dir_all(root.join("src")).unwrap();
        // The unresolved import is what puts this file in the repair pass, and
        // the skip is what the pass has to remember about it.
        let rel = "src/a.test.ts";
        std::fs::write(root.join(rel), "import { gone } from \"./missing\";\nit.skip(\"one\", () => { gone(); });\n")
            .unwrap();

        let full =
            Options { root: root.clone(), paths: vec![], changed_only: false, json: false, offline: true, diff: None };
        pass(&root, &full, true).unwrap();

        // A file the index has never seen is what triggers the repair pass; the
        // test file itself is untouched.
        std::fs::write(root.join("src/b.ts"), "export const b = 1;\n").unwrap();
        let changed = Options { changed_only: true, ..full };
        let second = pass(&root, &changed, true).unwrap();
        assert_eq!(
            second.previous.skipped_tests.get(rel),
            Some(&HashSet::from(["one".to_string()])),
            "an unchanged file the repair pass re-recorded still has the version the index held: {:?}",
            second.previous.skipped_tests
        );

        std::env::remove_var("LOCRIN_CACHE_DIR");
    }
}
