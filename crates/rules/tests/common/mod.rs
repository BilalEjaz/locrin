#![allow(dead_code)]

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use locrin_core::config::Config;
use locrin_core::entry::EntryPoints;
use locrin_core::finding::Finding;
use locrin_core::index::{content_hash, Index};
use locrin_core::indexer;
use locrin_core::parse::{parse_file, ParsedFile};
use locrin_core::previous::Previous;
use locrin_core::resolve::Resolver;
use locrin_core::walk::{canonical_root, source_files, WalkOptions};
use locrin_rules::{run_rules, Rule, RuleContext};

/// A config that turns one rule on, for the rules that ship off. `run_on` goes
/// through `run_rules`, so a rule whose `enabled_by_default` is false produces
/// nothing under `Config::default()`.
pub fn rule_on(id: &str) -> Config {
    let mut rules = std::collections::BTreeMap::new();
    rules.insert(id.to_string(), locrin_core::config::RuleOverride { enabled: Some(true), severity: None });
    Config { rules, ..Config::default() }
}

pub fn fixture(rule: &str, bucket: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(rule).join(bucket)
}

pub fn parse_dir(root: &Path) -> Vec<ParsedFile> {
    let root = canonical_root(root);
    source_files(&root, &WalkOptions::default())
        .unwrap()
        .into_iter()
        .filter_map(|p| parse_file(&root, &p).unwrap())
        .collect()
}

/// Indexes a fixture directory in memory the way the CLI indexes a repository,
/// so graph rules see real edges, symbols, and entry points.
pub fn index_dir(root: &Path, files: &[ParsedFile], config: &Config) -> (Index, EntryPoints) {
    let root = canonical_root(root);
    let indexed: HashSet<String> = files.iter().map(|f| f.rel.clone()).collect();
    let resolver = Resolver::new(&root, indexed);
    let mut ix = Index::open_in_memory().unwrap();
    for f in files {
        indexer::record(&mut ix, f, &content_hash(&f.source), &resolver).unwrap();
    }
    (ix, EntryPoints::detect(&root, &config.entry_points).unwrap())
}

/// An empty previous state, borrowed for the whole test run: a context needs a
/// reference rather than a value.
pub fn no_previous() -> &'static Previous {
    static P: std::sync::OnceLock<Previous> = std::sync::OnceLock::new();
    P.get_or_init(Previous::default)
}

/// A context over fixture files that are already parsed and indexed, for the
/// tests that call a rule directly instead of going through `run_on`.
pub fn ctx_for<'a>(
    files: &'a [ParsedFile],
    config: &'a Config,
    index: &'a Index,
    entries: &'a EntryPoints,
    root: &'a Path,
) -> RuleContext<'a> {
    RuleContext { files, config, index: Some(index), entries, root, offline: true, previous: no_previous() }
}

/// Runs one rule over a fixture directory with nothing known about the previous
/// version of it, which is what almost every rule's tests want.
pub fn run_on(rule: Box<dyn Rule>, root: &Path, config: &Config) -> Vec<Finding> {
    run_on_with(rule, root, config, &Previous::default())
}

/// The same, for the rules whose answer is a change: the caller says what the
/// previous version of the fixture said. Fixture runs are always offline; no
/// test may depend on a network call.
pub fn run_on_with(rule: Box<dyn Rule>, root: &Path, config: &Config, previous: &Previous) -> Vec<Finding> {
    let root = canonical_root(root);
    let files = parse_dir(&root);
    let (ix, entries) = index_dir(&root, &files, config);
    let ctx = RuleContext {
        files: &files,
        config,
        index: Some(&ix),
        entries: &entries,
        root: &root,
        offline: true,
        previous,
    };
    run_rules(&[rule], &ctx).unwrap()
}

/// (file, start line) pairs in the order the rule produced them.
pub fn hits(findings: &[Finding]) -> Vec<(String, u32)> {
    findings.iter().map(|f| (f.file.clone(), f.span.start_line)).collect()
}
