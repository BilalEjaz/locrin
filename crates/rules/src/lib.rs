pub mod boundary;
pub mod dead_export;
pub mod dead_file;
pub mod leftover_commented;
pub mod leftover_debug;
pub mod leftover_marker;
pub mod unreachable;
pub mod unused_import;

use std::collections::HashMap;

use locrin_core::config::Config;
use locrin_core::entry::EntryPoints;
use locrin_core::finding::{make_id, Category, Confidence, Finding, Severity, Span};
use locrin_core::index::Index;
use locrin_core::parse::ParsedFile;
use locrin_core::symbols::enclosing_symbol;
use rayon::prelude::*;

pub use locrin_core::ALLOW_MARK;

/// What a rule reads. A `File` rule looks only at the files parsed this run and
/// its findings can be cached per file. A `Graph` rule queries the index and
/// runs every time, because a change anywhere can move its answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    File,
    Graph,
}

/// Everything a rule is allowed to see: the parsed files of this run, the
/// repository config, the index, and the entry points. Rules never touch the
/// filesystem themselves.
///
/// The index is optional because a file rule never reads it: the parallel file
/// pass hands each file a context of its own, and a per-file context cannot
/// carry a database connection that is neither `Sync` nor cheap to clone. A
/// graph rule takes it through [`RuleContext::index`], which says so plainly
/// when it is missing instead of leaving the rule to unwrap nothing.
pub struct RuleContext<'a> {
    pub files: &'a [ParsedFile],
    pub config: &'a Config,
    pub index: Option<&'a Index>,
    pub entries: &'a EntryPoints,
}

impl<'a> RuleContext<'a> {
    /// The index, or an error naming the mistake. Reaching for one that is not
    /// there means a graph rule was put in a file-rule pass, which is a wiring
    /// error in the caller and not something a repository can provoke.
    pub fn index(&self) -> anyhow::Result<&'a Index> {
        self.index.ok_or_else(|| anyhow::anyhow!("graph rule run without an index"))
    }
}

/// `Sync` because the file pass runs one rule set across the pool: several
/// threads hold the same `&dyn Rule` at once. Every rule the engine ships is a
/// unit struct, so the bound costs nothing; a rule that wanted per-run mutable
/// state would have to say so with a lock, which is the right thing to make it
/// say.
pub trait Rule: Sync {
    fn id(&self) -> &'static str;
    /// One line for reporters and documentation; SARIF shows it as the rule's short description.
    fn description(&self) -> &'static str;
    fn scope(&self) -> Scope;
    fn category(&self) -> Category;
    fn default_severity(&self) -> Severity;
    fn confidence(&self) -> Confidence;
    /// Whether the rule runs when the config says nothing about it. Almost every
    /// rule ships on; one that cannot yet meet the spec 10.2 precision gate on a
    /// repository it knows nothing about ships off and says so in its own doc.
    fn enabled_by_default(&self) -> bool {
        true
    }
    fn run(&self, ctx: &RuleContext) -> anyhow::Result<Vec<Finding>>;
}

/// The files a file rule is allowed to look at: every parsed file whose tree came
/// back without a syntax error. Rules iterate this instead of `ctx.files`, so a
/// file that failed to parse is exempt from every rule rather than from whichever
/// rules happened to check `has_error`.
pub fn clean_files<'a>(ctx: &'a RuleContext) -> impl Iterator<Item = &'a ParsedFile> {
    let files: &'a [ParsedFile] = ctx.files;
    files.iter().filter(|f| !f.has_error)
}

/// The trimmed text of a 1-based line, or `""` when the line is out of range.
pub fn line_text(file: &ParsedFile, line: u32) -> &str {
    file.source.lines().nth(line.saturating_sub(1) as usize).unwrap_or("").trim()
}

/// A span covering one whole line.
pub fn line_span(file: &ParsedFile, line: u32) -> Span {
    let text = line_text(file, line);
    Span { start_line: line, start_col: 0, end_line: line, end_col: text.len() as u32 }
}

/// The identity anchor for a finding: the enclosing symbol name if there is
/// one, otherwise the line text. Anchoring on the symbol is what keeps a
/// finding's id stable when unrelated lines move around it.
pub fn anchor_for(file: &ParsedFile, line: u32) -> String {
    enclosing_symbol(file, line).unwrap_or_else(|| line_text(file, line).to_string())
}

/// Builds a finding from its parts. Every rule constructs findings through this
/// or through `finding`, which is what keeps ids consistent across rules.
pub fn finding_at(rule: &dyn Rule, rel: &str, span: Span, anchor: &str, evidence: &str, fix: &str) -> Finding {
    Finding {
        id: make_id(rule.id(), rel, anchor),
        rule: rule.id().to_string(),
        category: rule.category(),
        severity: rule.default_severity(),
        confidence: rule.confidence(),
        file: rel.to_string(),
        span,
        evidence: evidence.to_string(),
        fix: fix.to_string(),
        related: vec![],
        owasp: None,
        cwe: None,
    }
}

/// Builds a finding for a file rule at a line, anchored on the enclosing symbol.
pub fn finding(rule: &dyn Rule, file: &ParsedFile, line: u32, evidence: &str, fix: &str) -> Finding {
    finding_at(rule, &file.rel, line_span(file, line), &anchor_for(file, line), evidence, fix)
}

/// Runs the given rules, dropping any finding that came from a file which failed
/// to parse or whose line carries the `locrin:allow` marker, and applying the
/// configured severity to every finding that survives.
///
/// A rule runs when the config's explicit `enabled` says so, and when the config
/// is silent, when the rule's own `enabled_by_default` says so. That is the only
/// place enablement is decided.
///
/// The spec 9 gate is enforced here rather than left to the rules. For a file
/// parsed this run the parse status and the line text are at hand; for any
/// other file (graph rules report on files the run did not parse) the index
/// remembers both from when the file was last indexed.
pub fn run_rules(rules: &[Box<dyn Rule>], ctx: &RuleContext) -> anyhow::Result<Vec<Finding>> {
    let by_rel: HashMap<&str, &ParsedFile> = ctx.files.iter().map(|f| (f.rel.as_str(), f)).collect();
    let dropped = |f: &Finding| -> anyhow::Result<bool> {
        if let Some(file) = by_rel.get(f.file.as_str()) {
            return Ok(file.has_error || line_text(file, f.span.start_line).contains(ALLOW_MARK));
        }
        let index = ctx.index()?;
        Ok(index.parse_status(&f.file)?.as_deref() == Some("error") || index.is_allowed(&f.file, f.span.start_line)?)
    };
    let mut out = Vec::new();
    for rule in rules {
        if !ctx.config.rule_enabled_or(rule.id(), rule.enabled_by_default()) {
            continue;
        }
        let severity = ctx.config.severity_for(rule.id(), rule.default_severity());
        for mut f in rule.run(ctx)? {
            if dropped(&f)? {
                continue;
            }
            f.severity = severity;
            out.push(f);
        }
    }
    Ok(out)
}

/// Runs file rules over every file, one file at a time, across the pool.
///
/// A file rule reads one file and nothing else, so a file is a unit of work: each
/// gets a context holding only itself and goes through `run_rules`, which keeps
/// the enablement decision, the spec 9 gate and the configured severity in the
/// one place that has always decided them. The per-file results are collected in
/// file order and flattened, so the output is the same on every run and on every
/// machine, whatever order the pool happened to finish in.
///
/// The contexts carry no index. Nothing in a file rule's path needs one: the gate
/// asks the index only about files the run did not parse, and a per-file context
/// always holds the file its findings are about.
pub fn run_file_rules(
    rules: &[Box<dyn Rule>],
    files: &[ParsedFile],
    config: &Config,
    entries: &EntryPoints,
) -> anyhow::Result<Vec<Finding>> {
    let per_file: Vec<Vec<Finding>> = files
        .par_iter()
        .map(|file| {
            let ctx = RuleContext { files: std::slice::from_ref(file), config, index: None, entries };
            run_rules(rules, &ctx)
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    Ok(per_file.into_iter().flatten().collect())
}

/// The registry: every rule the engine ships, in the order they are declared.
pub fn all_rules() -> Vec<Box<dyn Rule>> {
    vec![
        Box::new(leftover_debug::LeftoverDebug::default()),
        Box::new(leftover_commented::LeftoverCommented),
        Box::new(leftover_marker::LeftoverMarker),
        Box::new(unused_import::UnusedImport),
        Box::new(unreachable::Unreachable),
        Box::new(dead_export::DeadExport),
        Box::new(dead_file::DeadFile),
        Box::new(boundary::BoundaryViolation),
    ]
}

pub fn file_rules() -> Vec<Box<dyn Rule>> {
    all_rules().into_iter().filter(|r| r.scope() == Scope::File).collect()
}

pub fn graph_rules() -> Vec<Box<dyn Rule>> {
    all_rules().into_iter().filter(|r| r.scope() == Scope::Graph).collect()
}

pub fn run_all(ctx: &RuleContext) -> anyhow::Result<Vec<Finding>> {
    run_rules(&all_rules(), ctx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use locrin_core::config::Config;
    use locrin_core::entry::EntryPoints;
    use locrin_core::finding::{Category, Confidence, Severity, Span};
    use locrin_core::index::Index;
    use locrin_core::parse::{parse_source, ParsedFile};
    use std::path::Path;

    struct Always;
    impl Rule for Always {
        fn id(&self) -> &'static str {
            "always"
        }
        fn description(&self) -> &'static str {
            "test rule"
        }
        fn scope(&self) -> Scope {
            Scope::File
        }
        fn category(&self) -> Category {
            Category::Erosion
        }
        fn default_severity(&self) -> Severity {
            Severity::Medium
        }
        fn confidence(&self) -> Confidence {
            Confidence::Medium
        }
        fn run(&self, ctx: &RuleContext) -> anyhow::Result<Vec<Finding>> {
            Ok(clean_files(ctx).map(|f| finding(self, f, 1, "hit", "remove it")).collect())
        }
    }

    /// A rule that ignores `clean_files` and walks `ctx.files` directly, the way
    /// a careless third-party rule would. `run_rules` has to hold the spec 9
    /// gate on its own, not trust the rule to hold it.
    struct Careless;
    impl Rule for Careless {
        fn id(&self) -> &'static str {
            "careless"
        }
        fn description(&self) -> &'static str {
            "test rule"
        }
        fn scope(&self) -> Scope {
            Scope::File
        }
        fn category(&self) -> Category {
            Category::Erosion
        }
        fn default_severity(&self) -> Severity {
            Severity::Medium
        }
        fn confidence(&self) -> Confidence {
            Confidence::Medium
        }
        fn run(&self, ctx: &RuleContext) -> anyhow::Result<Vec<Finding>> {
            Ok(ctx.files.iter().map(|f| finding(self, f, 1, "hit", "remove it")).collect())
        }
    }

    /// A graph rule reports on files the run did not parse. The gate still holds
    /// for them, from what the index remembers.
    struct Ghost;
    impl Rule for Ghost {
        fn id(&self) -> &'static str {
            "ghost"
        }
        fn description(&self) -> &'static str {
            "test rule"
        }
        fn scope(&self) -> Scope {
            Scope::Graph
        }
        fn category(&self) -> Category {
            Category::Erosion
        }
        fn default_severity(&self) -> Severity {
            Severity::Medium
        }
        fn confidence(&self) -> Confidence {
            Confidence::Medium
        }
        fn run(&self, _ctx: &RuleContext) -> anyhow::Result<Vec<Finding>> {
            let span = Span { start_line: 1, start_col: 0, end_line: 1, end_col: 0 };
            Ok(vec![
                finding_at(self, "src/broken.ts", span.clone(), "a", "hit", "fix"),
                finding_at(self, "src/allowed.ts", span.clone(), "a", "hit", "fix"),
                finding_at(self, "src/plain.ts", span, "a", "hit", "fix"),
            ])
        }
    }

    #[test]
    fn run_rules_applies_config_overrides() {
        let file = parse_source(Path::new("src/a.ts"), "src/a.ts", "export const a = 1;\n".into()).unwrap();
        let files = vec![file];
        let mut config = Config::default();
        let ix = Index::open_in_memory().unwrap();
        let entries = EntryPoints::detect(Path::new("."), &[]).unwrap();
        let ctx = RuleContext { files: &files, config: &config, index: Some(&ix), entries: &entries };
        let out = run_rules(&[Box::new(Always)], &ctx).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].severity, Severity::Medium);
        assert_eq!(out[0].id.len(), 16);

        config.rules.insert(
            "always".into(),
            locrin_core::config::RuleOverride { enabled: None, severity: Some(Severity::Low) },
        );
        let ctx = RuleContext { files: &files, config: &config, index: Some(&ix), entries: &entries };
        assert_eq!(run_rules(&[Box::new(Always)], &ctx).unwrap()[0].severity, Severity::Low);

        config
            .rules
            .insert("always".into(), locrin_core::config::RuleOverride { enabled: Some(false), severity: None });
        let ctx = RuleContext { files: &files, config: &config, index: Some(&ix), entries: &entries };
        assert!(run_rules(&[Box::new(Always)], &ctx).unwrap().is_empty());
    }

    /// A rule that ships off. Nothing but `enabled_by_default` separates it from
    /// `Always`, so the test measures the enablement decision and nothing else.
    struct OffByDefault;
    impl Rule for OffByDefault {
        fn id(&self) -> &'static str {
            "off-by-default"
        }
        fn description(&self) -> &'static str {
            "test rule"
        }
        fn scope(&self) -> Scope {
            Scope::File
        }
        fn category(&self) -> Category {
            Category::Erosion
        }
        fn default_severity(&self) -> Severity {
            Severity::Medium
        }
        fn confidence(&self) -> Confidence {
            Confidence::Medium
        }
        fn enabled_by_default(&self) -> bool {
            false
        }
        fn run(&self, ctx: &RuleContext) -> anyhow::Result<Vec<Finding>> {
            Ok(clean_files(ctx).map(|f| finding(self, f, 1, "hit", "remove it")).collect())
        }
    }

    #[test]
    fn a_rule_that_ships_off_runs_only_when_the_config_turns_it_on() {
        let file = parse_source(Path::new("src/a.ts"), "src/a.ts", "export const a = 1;\n".into()).unwrap();
        let files = vec![file];
        let mut config = Config::default();
        let ix = Index::open_in_memory().unwrap();
        let entries = EntryPoints::detect(Path::new("."), &[]).unwrap();
        let ctx = RuleContext { files: &files, config: &config, index: Some(&ix), entries: &entries };
        assert!(run_rules(&[Box::new(OffByDefault)], &ctx).unwrap().is_empty(), "no config, no findings");

        config
            .rules
            .insert("off-by-default".into(), locrin_core::config::RuleOverride { enabled: Some(true), severity: None });
        let ctx = RuleContext { files: &files, config: &config, index: Some(&ix), entries: &entries };
        assert_eq!(run_rules(&[Box::new(OffByDefault)], &ctx).unwrap().len(), 1, "enabled = true turns it on");
    }

    #[test]
    fn anchor_prefers_enclosing_symbol() {
        let file = parse_source(
            Path::new("src/a.ts"),
            "src/a.ts",
            "function f() {\n  console.log(1);\n}\nconsole.log(2);\n".into(),
        )
        .unwrap();
        assert_eq!(anchor_for(&file, 2), "f");
        assert_eq!(anchor_for(&file, 4), "console.log(2);");
        assert_eq!(line_text(&file, 4), "console.log(2);");
    }

    #[test]
    fn a_file_with_a_parse_error_is_exempt_from_every_rule() {
        let clean = parse_source(Path::new("src/a.ts"), "src/a.ts", "export const a = 1;\n".into()).unwrap();
        let broken =
            parse_source(Path::new("src/b.ts"), "src/b.ts", "export function broken( { return 1;\n".into()).unwrap();
        assert!(broken.has_error, "the fixture source must not parse cleanly");
        let files = vec![clean, broken];
        let config = Config::default();
        let ix = Index::open_in_memory().unwrap();
        let entries = EntryPoints::detect(Path::new("."), &[]).unwrap();
        let ctx = RuleContext { files: &files, config: &config, index: Some(&ix), entries: &entries };
        let out = run_rules(&[Box::new(Careless)], &ctx).unwrap();
        assert_eq!(out.len(), 1, "only the clean file should survive run_rules, got {out:?}");
        assert_eq!(out[0].file, "src/a.ts");
    }

    #[test]
    fn allow_marker_suppresses_a_finding_from_any_rule() {
        let file =
            parse_source(Path::new("src/a.ts"), "src/a.ts", "export const a = 1; // locrin:allow\n".into()).unwrap();
        let files = vec![file];
        let config = Config::default();
        let ix = Index::open_in_memory().unwrap();
        let entries = EntryPoints::detect(Path::new("."), &[]).unwrap();
        let ctx = RuleContext { files: &files, config: &config, index: Some(&ix), entries: &entries };
        assert!(run_rules(&[Box::new(Always)], &ctx).unwrap().is_empty());
    }

    #[test]
    fn the_gate_uses_the_index_for_files_not_parsed_this_run() {
        let mut ix = Index::open_in_memory().unwrap();
        ix.upsert_file("src/broken.ts", "typescript", "h", "error").unwrap();
        ix.upsert_file("src/allowed.ts", "typescript", "h", "ok").unwrap();
        ix.replace_allow_lines("src/allowed.ts", &[1]).unwrap();
        ix.upsert_file("src/plain.ts", "typescript", "h", "ok").unwrap();
        let files: Vec<ParsedFile> = vec![];
        let config = Config::default();
        let entries = EntryPoints::detect(Path::new("."), &[]).unwrap();
        let ctx = RuleContext { files: &files, config: &config, index: Some(&ix), entries: &entries };
        let out = run_rules(&[Box::new(Ghost)], &ctx).unwrap();
        assert_eq!(out.iter().map(|f| f.file.as_str()).collect::<Vec<_>>(), vec!["src/plain.ts"]);
    }

    /// Splitting the file rules across the pool must not change what they say.
    /// The comparison is against the same rules run over every file in one
    /// context, which is exactly what the sequential pass used to do.
    #[test]
    fn run_file_rules_matches_one_context_over_every_file() {
        let a = parse_source(Path::new("src/a.ts"), "src/a.ts", "export const a = 1;\n".into()).unwrap();
        let b = parse_source(Path::new("src/b.ts"), "src/b.ts", "export const b = 2;\n".into()).unwrap();
        let c =
            parse_source(Path::new("src/c.ts"), "src/c.ts", "export const c = 3; // locrin:allow\n".into()).unwrap();
        let d =
            parse_source(Path::new("src/d.ts"), "src/d.ts", "export function broken( { return 1;\n".into()).unwrap();
        assert!(d.has_error, "the fixture source must not parse cleanly");
        let files = vec![a, b, c, d];
        let config = Config::default();
        let ix = Index::open_in_memory().unwrap();
        let entries = EntryPoints::detect(Path::new("."), &[]).unwrap();

        let rules: Vec<Box<dyn Rule>> = vec![Box::new(Always)];
        let ctx = RuleContext { files: &files, config: &config, index: Some(&ix), entries: &entries };
        let sequential = run_rules(&rules, &ctx).unwrap();
        assert_eq!(
            sequential.iter().map(|f| f.file.as_str()).collect::<Vec<_>>(),
            vec!["src/a.ts", "src/b.ts"],
            "the allow marker and the parse error still gate, so there is something to compare"
        );
        assert_eq!(run_file_rules(&rules, &files, &config, &entries).unwrap(), sequential);

        // Several rules interleave differently: one context runs rule by rule,
        // the parallel pass runs file by file. Both produce the same findings,
        // and the reporter sorts them, so the comparison is on the set.
        let rules: Vec<Box<dyn Rule>> = vec![Box::new(Always), Box::new(Careless)];
        let ctx = RuleContext { files: &files, config: &config, index: Some(&ix), entries: &entries };
        let mut sequential = run_rules(&rules, &ctx).unwrap();
        let mut parallel = run_file_rules(&rules, &files, &config, &entries).unwrap();
        assert_eq!(parallel.len(), 4, "two rules over the two files that survive the gate");
        let key = |f: &Finding| (f.file.clone(), f.rule.clone(), f.span.start_line);
        sequential.sort_by_key(key);
        parallel.sort_by_key(key);
        assert_eq!(parallel, sequential);
    }

    /// A file rule never needs the index, so `run_file_rules` gives it none. A
    /// graph rule reaching for one that is not there is a programming error, and
    /// it has to say so rather than take the process down.
    #[test]
    fn a_graph_rule_without_an_index_errors_rather_than_panicking() {
        let files: Vec<ParsedFile> = vec![];
        let config = Config::default();
        let entries = EntryPoints::detect(Path::new("."), &[]).unwrap();
        let ctx = RuleContext { files: &files, config: &config, index: None, entries: &entries };
        assert!(ctx.index().is_err(), "no index means no answer");
        let err = run_rules(&graph_rules(), &ctx).unwrap_err();
        assert!(err.to_string().contains("graph rule run without an index"), "unhelpful message: {err}");
    }

    #[test]
    fn registry_lists_every_shipped_rule() {
        let files: Vec<ParsedFile> = vec![];
        let config = Config::default();
        let ix = Index::open_in_memory().unwrap();
        let entries = EntryPoints::detect(Path::new("."), &[]).unwrap();
        let ctx = RuleContext { files: &files, config: &config, index: Some(&ix), entries: &entries };
        let ids: Vec<&str> = all_rules().iter().map(|r| r.id()).collect();
        assert_eq!(
            ids,
            vec![
                "leftover-debug",
                "leftover-commented-code",
                "leftover-agent-marker",
                "unused-import",
                "unreachable",
                "dead-export",
                "dead-file",
                "boundary-violation"
            ]
        );
        assert!(run_all(&ctx).unwrap().is_empty(), "no files means no findings");
        assert_eq!(file_rules().len(), 5, "five file rules and three graph rules");
        assert_eq!(
            graph_rules().iter().map(|r| r.id()).collect::<Vec<_>>(),
            vec!["dead-export", "dead-file", "boundary-violation"]
        );
    }
}
