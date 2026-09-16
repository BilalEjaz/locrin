pub mod boundary;
pub mod dead_export;
pub mod dead_file;
pub mod express;
pub mod html_injection;
pub mod injection_sink;
pub mod leftover_commented;
pub mod leftover_debug;
pub mod leftover_marker;
pub mod secrets;
pub mod supabase;
pub mod swallowed_error;
pub mod test_newly_skipped;
pub mod test_no_assert;
pub mod unreachable;
pub mod unused_import;
pub mod vulnerable_dependency;
pub mod weak_crypto;

use std::collections::HashMap;
use std::path::Path;

use locrin_core::config::Config;
use locrin_core::entry::EntryPoints;
use locrin_core::finding::{make_id, Category, Confidence, Finding, Severity, Span};
use locrin_core::index::Index;
use locrin_core::parse::ParsedFile;
use locrin_core::previous::Previous;
use rayon::prelude::*;

/// Re-exported so a rule declaring [`Rule::languages`] names its set from the
/// crate it already imports the trait from.
pub use locrin_core::lang::{Language, ALL, JS_FAMILY};
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
/// repository config, the index, the entry points, the repository root, whether
/// the run may touch the network, and what the previous version of the
/// repository said.
///
/// Rules do not touch the filesystem. `root` is the one documented exception:
/// the two rules that must read a file the engine does not parse
/// (`supabase-table-without-rls` reads SQL migrations, `vulnerable-dependency`
/// reads a lockfile) resolve it from here rather than from the process's working
/// directory, which is not the repository root in a hook or an editor.
///
/// The index is optional because a file rule never reads it: the parallel file
/// pass hands each file a context of its own, and a per-file context cannot
/// carry a database connection that is neither `Sync` nor cheap to clone. A
/// graph rule takes it through [`RuleContext::index`], which says so plainly
/// when it is missing instead of leaving the rule to unwrap nothing.
///
/// `Copy` so the file pass can build a per-file context from the base with one
/// field changed; every field is a reference or a flag, so the copy is free.
#[derive(Clone, Copy)]
pub struct RuleContext<'a> {
    pub files: &'a [ParsedFile],
    pub config: &'a Config,
    pub index: Option<&'a Index>,
    pub entries: &'a EntryPoints,
    /// The canonical repository root. See the note above on filesystem access.
    pub root: &'a Path,
    /// Set when the run may not touch the network (spec 9). A rule that would
    /// have fetched serves its cache or warns and reports nothing.
    pub offline: bool,
    /// What the previous version of the repository said, for the rules whose
    /// answer is a change rather than a state.
    pub previous: &'a Previous,
    /// The languages of the rule that is running, which is what
    /// [`clean_files`] filters the file list down to. Not to be confused with
    /// `config.languages`, which is what the repository asked the walker to
    /// read: this is what the one rule holding the context was written for.
    ///
    /// [`run_rules`] sets it from [`languages_of`] before each rule's `run`, so
    /// a rule never sees a file in a language it was not taught or in one the
    /// repository has taken it off. A context built by hand carries [`ALL`],
    /// because a caller that hands a rule a file list directly has already
    /// chosen the files.
    pub rule_languages: &'a [Language],
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
    /// The languages this rule was written against. The runner hands it only
    /// the files in those languages, so a rule whose patterns are node kinds of
    /// the TypeScript grammar cannot report on a PHP or Python file by
    /// accident, and a rule taught a language says so once here rather than
    /// checking `file.language` in its own loop.
    ///
    /// It is a guarantee about findings and not only about the file list:
    /// [`run_rules`] drops a finding whose path names a language the rule did
    /// not declare, so a graph rule reading the index (which holds every
    /// indexed file) is held to its declaration too.
    ///
    /// The default is the JavaScript family, because that is what every rule
    /// the engine shipped before PHP and Python was written against. A rule
    /// that reads something every language has (a comment, a string literal, a
    /// lockfile) declares [`ALL`] instead.
    fn languages(&self) -> &'static [Language] {
        JS_FAMILY
    }
    /// Whether the rule runs when the config says nothing about it. Almost every
    /// rule ships on; one that cannot yet meet the spec 10.2 precision gate on a
    /// repository it knows nothing about ships off and says so in its own doc.
    fn enabled_by_default(&self) -> bool {
        true
    }
    /// Whether the rule reports on files of one language. The per-language half
    /// of `enabled_by_default`: a rule is measured against the spec 10.2
    /// precision gate once per language it declares, and a pair that fails
    /// ships off for that language alone while the rule stays on for every
    /// language it passed. [`run_rules`] drops a finding whose file is in a
    /// language the rule answers `false` for, so the answer holds for a graph
    /// rule reading the index as much as for a file rule, and a file whose
    /// extension names no language (a lockfile, a `.sql` migration) is never
    /// asked, because no language was measured for it.
    ///
    /// A config `[rules.<id>] enabled = true` does not override a per-language
    /// off: that key says whether the rule runs at all, and the languages a rule
    /// failed on are the engine's own measurement rather than the repository's
    /// choice. The knob for that is `[rules.<id>] languages = [...]`, which
    /// replaces this answer outright for the languages it names (see
    /// [`languages_of`]). The answer itself must be a constant of the binary,
    /// because it reaches the findings cache's key through
    /// [`rules_fingerprint`], which is computed once per run: an answer that
    /// varied with the repository would be a key changing under the run using
    /// it, and the override reaches that key as a config the fingerprint reads
    /// rather than as a rule that answers differently.
    ///
    /// The default is `true` for every language a rule declares. A rule that
    /// fails the PHP and Python precision gate
    /// (`docs/superpowers/plans/2026-09-11-php-and-python-precision.md`) for
    /// one language overrides this with a doc comment citing the report.
    /// `leftover-agent-marker` on PHP went off after round two and came back
    /// on in round three; `leftover-commented-code` on Python is recorded in
    /// its own override.
    fn enabled_for(&self, lang: Language) -> bool {
        let _ = lang;
        true
    }
    /// A locked rule ignores config overrides: it cannot be disabled and its
    /// severity cannot be lowered (spec 4.3, `secret-exposed`). A repository
    /// that wants a locked finding to stop failing the build accepts it into the
    /// baseline, where the acceptance is written down with a reason.
    fn locked(&self) -> bool {
        false
    }
    fn run(&self, ctx: &RuleContext) -> anyhow::Result<Vec<Finding>>;
}

/// Whether a rule runs under a config: a locked rule always, otherwise the
/// config's explicit `enabled` when it sets one and the rule's own
/// `enabled_by_default` when it does not.
///
/// This is the only place the question is answered. The CLI's findings cache
/// keys on the set of rules a run produces findings for, so it has to ask
/// exactly what [`run_rules`] asks or a cached file would be served without a
/// locked rule's findings. What the config cannot say is covered by
/// [`rules_fingerprint`], which is the other half of that key: the rule set's
/// own declarations, which no config mentions.
pub fn rule_runs(rule: &dyn Rule, config: &Config) -> bool {
    rule.locked() || config.rule_enabled_or(rule.id(), rule.enabled_by_default())
}

/// Whether a rule that declared `languages` may report on this file.
///
/// The path is the only thing every finding carries, so it is what the
/// guarantee is enforced on: a graph rule reports on files the run never parsed,
/// and [`clean_files`] cannot reach those. A path whose extension names no
/// language the engine parses (a `.sql` migration, a lockfile, a `.d.ts` stub)
/// is kept whatever the rule declared, because the declaration says nothing
/// about it and the rules reading those files off disk are precisely the ones
/// that would be silenced.
fn language_allowed(rel: &str, languages: &[Language]) -> bool {
    match Language::from_path(Path::new(rel)) {
        Some(language) => languages.contains(&language),
        None => true,
    }
}

/// Whether a rule reports on this file under its own per-language default
/// ([`Rule::enabled_for`]). A path naming no language is kept, for the reason
/// [`language_allowed`] keeps it.
///
/// Asked only when the config has named no languages for the rule. A config
/// that has is the repository overruling exactly this answer, and
/// [`languages_of`] has already reduced the rule to what it named.
fn enabled_for_file(rule: &dyn Rule, rel: &str) -> bool {
    match Language::from_path(Path::new(rel)) {
        Some(language) => rule.enabled_for(language),
        None => true,
    }
}

/// The languages a rule reports on under this config.
///
/// Without a `[rules.<id>] languages` key it is what the rule declares, and
/// each of those is kept or dropped by the rule's own per-language default
/// separately, in [`enabled_for_file`]. With one it is exactly the languages
/// the config named: the key replaces the default rather than adding to it, so
/// a pair the engine ships off is turned on by the repository that has measured
/// it for itself, and a rule can equally be narrowed to one language it was
/// measured on. Whether the rule can read each named language is checked once,
/// by [`validate_config`], so a name that reaches here is one the rule declared.
///
/// A locked rule has no answer but its own declaration, for the reason
/// [`language_override`] gives.
pub fn languages_of(rule: &dyn Rule, config: &Config) -> Vec<Language> {
    match language_override(rule, config) {
        Some(listed) => listed,
        None => rule.languages().to_vec(),
    }
}

/// The languages a config puts a rule on, or `None` when it names none and when
/// the rule is locked.
///
/// A locked rule ignores the `rules` table (spec 4.3, `secret-exposed`), and
/// `languages` is part of that table: a config that could narrow where a locked
/// rule reports could silence it on a language, which is the disabling the lock
/// exists to prevent. So the key is ignored here, exactly as `enabled` and
/// `severity` are, and [`validate_config`] additionally refuses it out loud,
/// because unlike those two it can only have been written to narrow the lock.
/// Every reading of the override goes through this, so the runner, the
/// fingerprint and the SARIF report cannot disagree about what a rule runs on.
fn language_override(rule: &dyn Rule, config: &Config) -> Option<Vec<Language>> {
    if rule.locked() {
        return None;
    }
    config.rule_languages(rule.id())
}

/// Checks every `[rules.<id>] languages` list against the rule it names.
///
/// The names themselves are checked by `Config::load`, which is in the crate
/// that owns the config; whether a rule can read a language is a question only
/// the rule set answers, and the rule set is here. A rule this engine does not
/// ship is left alone, like every other key on an unknown rule.
///
/// A locked rule refuses the key outright, which is stricter than the silent
/// ignore `enabled` and `severity` get: those two describe a run that still
/// happens, while a `languages` list on a locked rule can only have been written
/// to narrow a lock, so saying nothing would leave its author believing it took.
pub fn validate_config(config: &Config) -> anyhow::Result<()> {
    for rule in all_rules() {
        // Asked of the config rather than through `language_override`, which is
        // the reading that ignores a locked rule's list. The point here is to
        // find the list that reading ignores and say so.
        let Some(listed) = config.rule_languages(rule.id()) else {
            continue;
        };
        anyhow::ensure!(
            !rule.locked(),
            "rules.{}.languages is set, but {} is locked: a locked rule reports on every language it reads, \
             and a finding that should not fail the build is accepted into the baseline with a reason",
            rule.id(),
            rule.id()
        );
        let declared = rule.languages();
        for language in listed {
            anyhow::ensure!(
                declared.contains(&language),
                "rules.{}.languages names {}, which {} does not read (it reads {})",
                rule.id(),
                language.as_str(),
                rule.id(),
                declared.iter().map(|l| l.as_str()).collect::<Vec<_>>().join(", ")
            );
        }
    }
    Ok(())
}

/// The files a file rule is allowed to look at: every parsed file written in one
/// of the rule's own languages whose tree came back without a syntax error the
/// engine cannot see past (see `parse::has_blocking_error`). Rules iterate this
/// instead of `ctx.files`, so a file that failed to parse is exempt from every
/// rule rather than from whichever rules happened to check `has_error`, and a
/// file in a language a rule was not taught is never handed to it at all.
///
/// The language half of the filter is the whole of [`Rule::languages`]'s
/// enforcement, and it is why the languages live on the context rather than on
/// a filtered file list: `ctx.files` is a slice of owned `ParsedFile`s, so
/// narrowing it would mean either cloning trees or changing the field to a
/// slice of references and reallocating it once per rule per file in the
/// parallel pass. A borrowed `&'a [Language]` field on the context costs one
/// slice the runner already built for the rule it is about to call, and leaves
/// every rule's `clean_files(ctx)` loop exactly as it was.
pub fn clean_files<'a>(ctx: &'a RuleContext) -> impl Iterator<Item = &'a ParsedFile> {
    let files: &'a [ParsedFile] = ctx.files;
    let languages: &'a [Language] = ctx.rule_languages;
    files.iter().filter(move |f| !f.has_error && languages.contains(&f.language))
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

/// The part of a finding's identity that survives a line shift: the enclosing
/// symbol name if there is one, the line's trimmed text, and the ordinal of
/// this line among the identical lines inside that symbol.
///
/// Anchoring on the symbol is what keeps a finding's id stable when unrelated
/// lines move around it. The text and the ordinal are what keep two findings
/// apart: two identical `console.log` lines in one function are two lines a
/// reader has to delete, and on the symbol alone they had one id between them,
/// so accepting either into a baseline silently accepted the other. Neither
/// part is a line number, so moving the whole function down the file leaves
/// both ids alone (spec 7.1).
pub fn anchor_for(file: &ParsedFile, line: u32) -> String {
    // The symbol table is extracted once per file and then asked many times.
    // `enclosing_symbol` is not a lookup: extracting walks the whole tree and
    // allocates the symbol list again, so asking it once per identical earlier
    // line made a file of byte-identical flagged lines cost tree walks
    // quadratically, and extracting here made a file with many findings pay for
    // one walk each. `symbols_of` extracts on the first call and hands back the
    // same table after it, so a file costs one walk however many findings it
    // has. Resolving goes through the same `enclosing` the report uses, so an
    // anchor and the symbol a finding names can never be two different answers.
    let symbols = locrin_core::symbols::symbols_of(file);
    let enclosing = |at: u32| locrin_core::symbols::enclosing(symbols, at).map(|s| s.name.as_str());
    let symbol = enclosing(line);
    let text = line_text(file, line);
    // Only a line that reads the same can be an earlier occurrence, and reading
    // the same is a string compare where sharing a symbol is a scan of the
    // symbol list, so the cheap half is asked first: a file whose lines are all
    // different pays for one pass over the text and no symbol scan at all.
    let ordinal = file
        .source
        .lines()
        .take(line.saturating_sub(1) as usize)
        .enumerate()
        .filter(|(_, earlier)| earlier.trim() == text)
        .filter(|(i, _)| enclosing(*i as u32 + 1) == symbol)
        .count();
    format!("{}\x1f{text}\x1f{ordinal}", symbol.unwrap_or_default())
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
/// A rule runs when [`rule_runs`] says so, which is the only place enablement is
/// decided. A locked rule also keeps its own `default_severity`: the config can
/// neither turn it off nor lower it.
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
        if !rule_runs(rule.as_ref(), ctx.config) {
            continue;
        }
        // A finding arrives carrying a severity: `finding_at` gives it the rule's
        // own default, and one rule (`vulnerable-dependency`) then replaces it
        // with the severity of the advisory that finding reports. So the runner
        // overwrites only when it has something to say. A locked rule's severity
        // is its own by definition, and the config may not lower it.
        let severity =
            if rule.locked() { Some(rule.default_severity()) } else { ctx.config.severity_override(rule.id()) };
        // The per-rule language filter, applied once here rather than in each
        // rule: the rule's own context says which languages it was taught and
        // which of them this repository has left it on, and `clean_files` hands
        // it those files and no others. Both entry points come through this
        // loop, so the parallel file pass gets the same filter without knowing
        // there is one.
        let languages = languages_of(rule.as_ref(), ctx.config);
        // The per-language default is the engine's own measurement, so it
        // applies only where the repository has not replaced it outright.
        let overridden = language_override(rule.as_ref(), ctx.config).is_some();
        let ctx = &RuleContext { rule_languages: &languages, ..*ctx };
        for mut f in rule.run(ctx)? {
            // The other half of the guarantee, and the half that holds for the
            // rules `clean_files` cannot reach: a graph rule queries the index,
            // which remembers every file the walk indexed whatever language it
            // was written in, so the declaration is enforced on what comes back
            // rather than on what went in. Asked before the spec 9 gate, which
            // is the half that may have to go to the index for an answer.
            if !language_allowed(&f.file, &languages) {
                continue;
            }
            // The per-language default, applied to findings for the same reason
            // the declaration is: it is the one place both kinds of rule pass
            // through. A file rule is still handed the files of a language it
            // ships off for and its findings there are dropped here, which
            // costs the rule's work on those files and nothing else; no rule
            // ships off for a language today, so nothing pays it.
            if !overridden && !enabled_for_file(rule.as_ref(), &f.file) {
                continue;
            }
            if dropped(&f)? {
                continue;
            }
            if let Some(severity) = severity {
                f.severity = severity;
            }
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
/// Each per-file context is `base` with one file in it, so everything else a
/// rule can read (the config, the entry points, the root, the offline flag, the
/// previous state) is the same in the parallel pass as it would be in one
/// context over every file. The contexts carry no index: nothing in a file
/// rule's path needs one, because the gate asks the index only about files the
/// run did not parse and a per-file context always holds the file its findings
/// are about.
pub fn run_file_rules(
    rules: &[Box<dyn Rule>],
    files: &[ParsedFile],
    base: &RuleContext,
) -> anyhow::Result<Vec<Finding>> {
    // The base is unpacked before the pool rather than captured whole: it can
    // carry an index, an index holds a `Connection`, and a `Connection` is not
    // `Sync`, so a closure holding one would not compile even though the
    // per-file contexts drop it. Unpacking says which fields cross the pool.
    let RuleContext { config, entries, root, offline, previous, .. } = *base;
    let per_file: Vec<Vec<Finding>> = files
        .par_iter()
        .map(|file| {
            let ctx = RuleContext {
                files: std::slice::from_ref(file),
                config,
                index: None,
                entries,
                root,
                offline,
                previous,
                // `run_rules` narrows this to each rule's own languages, so the
                // per-file context starts from the widest set rather than
                // deciding anything the shared runner has not decided.
                rule_languages: ALL,
            };
            run_rules(rules, &ctx)
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    Ok(per_file.into_iter().flatten().collect())
}

/// The revision of the rule set's *logic*, which [`rules_fingerprint`] cannot
/// read for itself.
///
/// The fingerprint hashes what every rule declares about itself. It cannot hash
/// the body of `run`, so a precision fix inside one rule (a pattern narrowed, a
/// false positive removed) leaves every declaration exactly where it was and a
/// warm findings cache would keep serving the rows the old rule wrote. Bump this
/// when a rule's logic changes without a version bump; a release bumps the crate
/// version, which the cache key already carries.
pub const RULES_REVISION: u32 = 1;

/// A hash of everything the rule set declares about itself, for the findings
/// cache's key.
///
/// The key used to be the file's content hash plus the crate version and the
/// config. Inside one version that left a whole class of change invisible: a
/// rule turned off for a language, a severity or confidence moved, a rule added
/// or removed, a precision fix in a rule's body. A repository nobody had edited
/// would go on being answered from the rows the old rules wrote, and the only
/// way out was a release or a deleted cache directory. This is the missing half
/// of the key: every rule in [`all_rules`] order, its id, the languages it
/// declares, its [`Rule::enabled_for`] answer for every language the engine
/// parses, its severity, its confidence and whether it ships on, plus
/// [`RULES_REVISION`] for what none of that can see.
///
/// The config is read for one thing only: a `[rules.<id>] languages` override
/// replaces the per-language answers hashed here, so without it a repository
/// that turned a pair on would go on being served the rows written while it was
/// off. Everything else the config says already reaches the key through
/// `cache::config_hash`, which hashes the config itself.
pub fn rules_fingerprint(config: &Config) -> String {
    fingerprint_of(&all_rules(), config)
}

/// [`rules_fingerprint`] over a rule set named by the caller, so a test can hash
/// two sets it controls and see that a difference between them reaches the hash.
fn fingerprint_of(rules: &[Box<dyn Rule>], config: &Config) -> String {
    let mut h = blake3::Hasher::new();
    h.update(&RULES_REVISION.to_le_bytes());
    for rule in rules {
        // A record separator between rules and a unit separator between the
        // fields of one, so no two different rule sets can flatten to the same
        // byte string by running their fields together.
        h.update(b"\x1e");
        h.update(rule.id().as_bytes());
        for lang in rule.languages() {
            h.update(b"\x1f");
            h.update(lang.as_str().as_bytes());
        }
        // The languages this rule reports on under this config: its
        // per-language defaults, or the override that replaced them.
        let effective = languages_of(rule.as_ref(), config);
        let overridden = language_override(rule.as_ref(), config).is_some();
        for lang in ALL {
            h.update(b"\x1f");
            h.update(&[u8::from(effective.contains(lang) && (overridden || rule.enabled_for(*lang)))]);
        }
        h.update(b"\x1f");
        h.update(format!("{:?}", rule.default_severity()).as_bytes());
        h.update(b"\x1f");
        h.update(format!("{:?}", rule.confidence()).as_bytes());
        h.update(b"\x1f");
        h.update(&[u8::from(rule.enabled_by_default())]);
    }
    h.finalize().to_hex()[..16].to_string()
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
        Box::new(swallowed_error::SwallowedError),
        Box::new(test_no_assert::TestNoAssert),
        Box::new(test_newly_skipped::TestNewlySkipped),
        Box::new(secrets::SecretExposed),
        Box::new(weak_crypto::WeakCrypto),
        Box::new(injection_sink::InjectionSink),
        Box::new(html_injection::HtmlInjection),
        Box::new(vulnerable_dependency::VulnerableDependency),
        Box::new(supabase::service_role::SupabaseServiceRoleInClient),
        Box::new(supabase::rls::SupabaseTableWithoutRls),
        Box::new(express::route_auth::ExpressRouteWithoutAuth),
        Box::new(express::cors::ExpressCorsWildcardOnAuthenticated),
        Box::new(express::cookie::ExpressCookieInsecure),
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
    use locrin_core::previous::Previous;
    use std::collections::HashSet;
    use std::path::Path;

    /// An empty previous state, borrowed for the whole test run. Most tests say
    /// nothing about the previous version of the repository, and a context needs
    /// a reference rather than a value.
    fn no_previous() -> &'static Previous {
        static P: std::sync::OnceLock<Previous> = std::sync::OnceLock::new();
        P.get_or_init(Previous::default)
    }

    /// A context for the parts of it a test is about. The root, the offline flag
    /// and the previous state are fixed here so a test that does not care about
    /// them does not have to spell them out.
    fn test_ctx<'a>(
        files: &'a [ParsedFile],
        config: &'a Config,
        index: Option<&'a Index>,
        entries: &'a EntryPoints,
    ) -> RuleContext<'a> {
        RuleContext {
            files,
            config,
            index,
            entries,
            root: Path::new("."),
            offline: true,
            previous: no_previous(),
            rule_languages: ALL,
        }
    }

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

    /// A rule taught every language the engine parses, the way
    /// `leftover-agent-marker` is: it reads a comment, and every language has
    /// one.
    struct EveryLanguage;
    impl Rule for EveryLanguage {
        fn id(&self) -> &'static str {
            "every-language"
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
        fn languages(&self) -> &'static [Language] {
            ALL
        }
        fn run(&self, ctx: &RuleContext) -> anyhow::Result<Vec<Finding>> {
            Ok(clean_files(ctx).map(|f| finding(self, f, 1, "hit", "remove it")).collect())
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

    /// A rule is handed the files of the languages it declared and no others.
    /// The default is the JavaScript family, so the rules written against the
    /// TypeScript grammar's node kinds never see a Python file however the
    /// repository set `[languages]`; a rule that declared every language sees
    /// both files.
    #[test]
    fn a_rule_is_handed_only_the_languages_it_declares() {
        let ts = parse_source(Path::new("src/a.ts"), "src/a.ts", "export const a = 1;\n".into()).unwrap();
        let py = parse_source(Path::new("src/b.py"), "src/b.py", "b = 1\n".into()).unwrap();
        let files = vec![ts, py];
        let config = Config::default();
        let ix = Index::open_in_memory().unwrap();
        let entries = EntryPoints::detect(Path::new("."), &[]).unwrap();
        let ctx = test_ctx(&files, &config, Some(&ix), &entries);

        let js_only = run_rules(&[Box::new(Always)], &ctx).unwrap();
        assert_eq!(js_only.iter().map(|f| f.file.as_str()).collect::<Vec<_>>(), vec!["src/a.ts"]);

        let every = run_rules(&[Box::new(EveryLanguage)], &ctx).unwrap();
        assert_eq!(every.iter().map(|f| f.file.as_str()).collect::<Vec<_>>(), vec!["src/a.ts", "src/b.py"]);
    }

    /// The filter lives in the shared runner, so the parallel file pass applies
    /// it too: a Python file is its own unit of work there, and a JavaScript
    /// rule handed that unit must still report nothing.
    #[test]
    fn the_language_filter_holds_in_the_parallel_file_pass() {
        let ts = parse_source(Path::new("src/a.ts"), "src/a.ts", "export const a = 1;\n".into()).unwrap();
        let py = parse_source(Path::new("src/b.py"), "src/b.py", "b = 1\n".into()).unwrap();
        let files = vec![ts, py];
        let config = Config::default();
        let entries = EntryPoints::detect(Path::new("."), &[]).unwrap();
        let base = RuleContext {
            files: &files,
            config: &config,
            index: None,
            entries: &entries,
            root: Path::new("."),
            offline: true,
            previous: no_previous(),
            rule_languages: ALL,
        };
        let out = run_file_rules(&[Box::new(Always), Box::new(EveryLanguage)], &files, &base).unwrap();
        let hits: Vec<(&str, &str)> = out.iter().map(|f| (f.rule.as_str(), f.file.as_str())).collect();
        assert_eq!(hits, vec![("always", "src/a.ts"), ("every-language", "src/a.ts"), ("every-language", "src/b.py")]);
    }

    /// A rule that reports on files the run did not parse, in the languages it
    /// is constructed with. A graph rule reaches the index rather than
    /// `clean_files`, so nothing on its own path narrows what it reports: the
    /// runner has to hold the declaration for it.
    struct Reporting(&'static [Language]);
    impl Rule for Reporting {
        fn id(&self) -> &'static str {
            "reporting"
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
        fn languages(&self) -> &'static [Language] {
            self.0
        }
        fn run(&self, _ctx: &RuleContext) -> anyhow::Result<Vec<Finding>> {
            let span = Span { start_line: 1, start_col: 0, end_line: 1, end_col: 0 };
            Ok(["src/a.ts", "src/x.php", "db/schema.sql"]
                .iter()
                .map(|rel| finding_at(self, rel, span.clone(), "a", "hit", "fix"))
                .collect())
        }
    }

    /// [`Rule::languages`] is a guarantee about findings, not only about the
    /// files a rule is handed: a graph rule that never calls `clean_files` may
    /// not report on a language it did not declare. A file whose extension names
    /// no language the engine parses (a `.sql` migration, a lockfile) is kept,
    /// because the declaration says nothing about it and the rules that report
    /// on those files are exactly the ones reading them off disk.
    /// A rule on for every language it declares except PHP, which is what a
    /// rule that failed the precision gate on one language looks like.
    struct OffForPhp;
    impl Rule for OffForPhp {
        fn id(&self) -> &'static str {
            "off-for-php"
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
        fn languages(&self) -> &'static [Language] {
            ALL
        }
        fn enabled_for(&self, lang: Language) -> bool {
            lang != Language::Php
        }
        fn run(&self, _ctx: &RuleContext) -> anyhow::Result<Vec<Finding>> {
            let span = Span { start_line: 1, start_col: 0, end_line: 1, end_col: 0 };
            Ok(["src/a.ts", "src/x.php", "src/y.py", "db/schema.sql"]
                .iter()
                .map(|rel| finding_at(self, rel, span.clone(), "a", "hit", "fix"))
                .collect())
        }
    }

    /// [`Rule::enabled_for`] drops the findings of the language it is off for
    /// and nothing else: the other declared languages and the files naming no
    /// language stay. A config `enabled = true` turns the rule on, which it
    /// already is, and does not reach the per-language default.
    #[test]
    fn a_rule_off_for_one_language_keeps_its_findings_elsewhere() {
        let files: Vec<ParsedFile> = vec![];
        let mut config = Config::default();
        let ix = Index::open_in_memory().unwrap();
        let entries = EntryPoints::detect(Path::new("."), &[]).unwrap();
        let ctx = test_ctx(&files, &config, Some(&ix), &entries);

        let out = run_rules(&[Box::new(OffForPhp)], &ctx).unwrap();
        assert_eq!(
            out.iter().map(|f| f.file.as_str()).collect::<Vec<_>>(),
            vec!["src/a.ts", "src/y.py", "db/schema.sql"],
            "the PHP finding is dropped and the rest are kept"
        );

        config.rules.insert(
            "off-for-php".into(),
            locrin_core::config::RuleOverride { enabled: Some(true), severity: None, languages: None },
        );
        let ctx = test_ctx(&files, &config, Some(&ix), &entries);
        let out = run_rules(&[Box::new(OffForPhp)], &ctx).unwrap();
        assert_eq!(
            out.iter().map(|f| f.file.as_str()).collect::<Vec<_>>(),
            vec!["src/a.ts", "src/y.py", "db/schema.sql"],
            "enabled = true does not override a per-language off"
        );
    }

    /// The intended escape from a per-language default. The list replaces the
    /// default outright: a language the rule ships off for runs when the
    /// repository names it, and a language it ships on for stops when the
    /// repository does not.
    #[test]
    fn a_languages_override_replaces_the_per_language_default() {
        let files: Vec<ParsedFile> = vec![];
        let mut config = Config::default();
        let ix = Index::open_in_memory().unwrap();
        let entries = EntryPoints::detect(Path::new("."), &[]).unwrap();
        let over = |enabled| locrin_core::config::RuleOverride {
            enabled,
            severity: None,
            languages: Some(vec!["php".to_string()]),
        };

        config.rules.insert("off-for-php".into(), over(None));
        let ctx = test_ctx(&files, &config, Some(&ix), &entries);
        let out = run_rules(&[Box::new(OffForPhp)], &ctx).unwrap();
        assert_eq!(
            out.iter().map(|f| f.file.as_str()).collect::<Vec<_>>(),
            vec!["src/x.php", "db/schema.sql"],
            "PHP is on because the config named it, and the languages it did not name are off"
        );

        // `enabled = false` still wins. The key says which languages a rule
        // reports on, not whether the rule runs at all.
        config.rules.insert("off-for-php".into(), over(Some(false)));
        let ctx = test_ctx(&files, &config, Some(&ix), &entries);
        assert!(run_rules(&[Box::new(OffForPhp)], &ctx).unwrap().is_empty());
    }

    /// A rule reports only on the languages it was written against, so naming
    /// one it cannot read is a mistake in the config rather than a line that
    /// does nothing, and the message says which languages it does read.
    #[test]
    fn a_language_a_rule_cannot_read_is_a_config_error() {
        let listed = |names: &[&str]| locrin_core::config::RuleOverride {
            enabled: None,
            severity: None,
            languages: Some(names.iter().map(|n| n.to_string()).collect()),
        };
        let mut config = Config::default();
        config.rules.insert("dead-file".into(), listed(&["python"]));
        let err = format!("{:#}", validate_config(&config).unwrap_err());
        assert!(err.contains("rules.dead-file.languages"), "{err}");
        assert!(err.contains("python"), "{err}");
        assert!(err.contains("typescript"), "the languages the rule does read are named: {err}");

        // The override the README documents is accepted, and so is a config
        // naming a rule this engine does not ship, which is what every other
        // key already does.
        let mut ok = Config::default();
        ok.rules.insert("leftover-commented-code".into(), listed(&["typescript", "tsx", "javascript", "python"]));
        ok.rules.insert("no-such-rule".into(), listed(&["python"]));
        validate_config(&ok).unwrap();
    }

    #[test]
    fn a_finding_in_a_language_the_rule_did_not_declare_is_dropped() {
        let files: Vec<ParsedFile> = vec![];
        let config = Config::default();
        let ix = Index::open_in_memory().unwrap();
        let entries = EntryPoints::detect(Path::new("."), &[]).unwrap();
        let ctx = test_ctx(&files, &config, Some(&ix), &entries);

        let js = run_rules(&[Box::new(Reporting(JS_FAMILY))], &ctx).unwrap();
        assert_eq!(
            js.iter().map(|f| f.file.as_str()).collect::<Vec<_>>(),
            vec!["src/a.ts", "db/schema.sql"],
            "the PHP finding is not the JavaScript rule's to report"
        );

        let every = run_rules(&[Box::new(Reporting(ALL))], &ctx).unwrap();
        assert_eq!(
            every.iter().map(|f| f.file.as_str()).collect::<Vec<_>>(),
            vec!["src/a.ts", "src/x.php", "db/schema.sql"]
        );
    }

    #[test]
    fn run_rules_applies_config_overrides() {
        let file = parse_source(Path::new("src/a.ts"), "src/a.ts", "export const a = 1;\n".into()).unwrap();
        let files = vec![file];
        let mut config = Config::default();
        let ix = Index::open_in_memory().unwrap();
        let entries = EntryPoints::detect(Path::new("."), &[]).unwrap();
        let ctx = test_ctx(&files, &config, Some(&ix), &entries);
        let out = run_rules(&[Box::new(Always)], &ctx).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].severity, Severity::Medium);
        assert_eq!(out[0].id.len(), 16);

        config.rules.insert(
            "always".into(),
            locrin_core::config::RuleOverride { enabled: None, severity: Some(Severity::Low), languages: None },
        );
        let ctx = test_ctx(&files, &config, Some(&ix), &entries);
        assert_eq!(run_rules(&[Box::new(Always)], &ctx).unwrap()[0].severity, Severity::Low);

        config.rules.insert(
            "always".into(),
            locrin_core::config::RuleOverride { enabled: Some(false), severity: None, languages: None },
        );
        let ctx = test_ctx(&files, &config, Some(&ix), &entries);
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
        let ctx = test_ctx(&files, &config, Some(&ix), &entries);
        assert!(run_rules(&[Box::new(OffByDefault)], &ctx).unwrap().is_empty(), "no config, no findings");

        config.rules.insert(
            "off-by-default".into(),
            locrin_core::config::RuleOverride { enabled: Some(true), severity: None, languages: None },
        );
        let ctx = test_ctx(&files, &config, Some(&ix), &entries);
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
        assert_eq!(anchor_for(&file, 2), "f\u{1f}console.log(1);\u{1f}0");
        assert_eq!(anchor_for(&file, 4), "\u{1f}console.log(2);\u{1f}0", "a line outside every symbol has no name");
        assert_eq!(line_text(&file, 4), "console.log(2);");
    }

    /// The source two of these tests share: one function holding the same debug
    /// line twice. Before the ordinal was part of the anchor these two lines had
    /// one id between them, so accepting either into a baseline accepted both.
    const TWICE: &str = "function f() {\n  console.log(\"a\");\n  console.log(\"a\");\n}\n";

    fn parsed(source: &str) -> ParsedFile {
        parse_source(Path::new("src/a.ts"), "src/a.ts", source.into()).unwrap()
    }

    #[test]
    fn two_identical_lines_in_one_function_get_two_ids() {
        let file = parsed(TWICE);
        assert_ne!(anchor_for(&file, 2), anchor_for(&file, 3), "the second occurrence is a finding of its own");
    }

    /// The reason the ordinal counts occurrences rather than naming the line: an
    /// edit above the function moves both lines and neither finding may be
    /// retired and re-reported for it (spec 7.1).
    #[test]
    fn an_anchor_survives_a_line_shift() {
        let file = parsed(TWICE);
        let shifted = parsed(&format!("\n\n{TWICE}"));
        assert_eq!(anchor_for(&shifted, 4), anchor_for(&file, 2));
        assert_eq!(anchor_for(&shifted, 5), anchor_for(&file, 3));
    }

    /// The shape of the ordinal's cost, pinned on the file that used to be the
    /// bad case: 200 byte-identical lines in one function. The scan reads every
    /// earlier line once, and the symbol table behind it is extracted once for
    /// the whole call rather than once per matching line, so this is one tree
    /// walk and not two hundred.
    #[test]
    fn the_ordinal_counts_every_identical_earlier_line() {
        let body = "  console.log(\"x\");\n".repeat(200);
        let file = parsed(&format!("function f() {{\n{body}}}\n"));
        assert_eq!(anchor_for(&file, 201), "f\u{1f}console.log(\"x\");\u{1f}199");
    }

    #[test]
    fn identical_lines_in_different_functions_differ() {
        let file = parsed("function f() {\n  console.log(\"a\");\n}\nfunction g() {\n  console.log(\"a\");\n}\n");
        assert_ne!(anchor_for(&file, 2), anchor_for(&file, 5), "the enclosing symbol still separates them");
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
        let ctx = test_ctx(&files, &config, Some(&ix), &entries);
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
        let ctx = test_ctx(&files, &config, Some(&ix), &entries);
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
        let ctx = test_ctx(&files, &config, Some(&ix), &entries);
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
        let ctx = test_ctx(&files, &config, Some(&ix), &entries);
        let sequential = run_rules(&rules, &ctx).unwrap();
        assert_eq!(
            sequential.iter().map(|f| f.file.as_str()).collect::<Vec<_>>(),
            vec!["src/a.ts", "src/b.ts"],
            "the allow marker and the parse error still gate, so there is something to compare"
        );
        assert_eq!(run_file_rules(&rules, &files, &test_ctx(&files, &config, None, &entries)).unwrap(), sequential);

        // Several rules interleave differently: one context runs rule by rule,
        // the parallel pass runs file by file. Both produce the same findings,
        // and the reporter sorts them, so the comparison is on the set.
        let rules: Vec<Box<dyn Rule>> = vec![Box::new(Always), Box::new(Careless)];
        let ctx = test_ctx(&files, &config, Some(&ix), &entries);
        let mut sequential = run_rules(&rules, &ctx).unwrap();
        let mut parallel = run_file_rules(&rules, &files, &test_ctx(&files, &config, None, &entries)).unwrap();
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
        let ctx = test_ctx(&files, &config, None, &entries);
        assert!(ctx.index().is_err(), "no index means no answer");
        let err = run_rules(&graph_rules(), &ctx).unwrap_err();
        assert!(err.to_string().contains("graph rule run without an index"), "unhelpful message: {err}");
    }

    /// Two rules alike in everything [`fingerprint_of`] reads except the
    /// per-language answer this one is constructed with, so a difference in
    /// their hashes can only be that answer.
    struct PerLanguage(bool);
    impl Rule for PerLanguage {
        fn id(&self) -> &'static str {
            "per-language"
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
        fn languages(&self) -> &'static [Language] {
            ALL
        }
        fn enabled_for(&self, lang: Language) -> bool {
            self.0 || lang != Language::Php
        }
        fn run(&self, _ctx: &RuleContext) -> anyhow::Result<Vec<Finding>> {
            Ok(vec![])
        }
    }

    /// The findings cache keys on the fingerprint, so what it notices is the
    /// contract: every declaration a rule makes, and the shape of the set. What
    /// it cannot notice is a rule's body, which is what [`RULES_REVISION`] is
    /// for.
    #[test]
    fn the_fingerprint_moves_with_every_declaration_it_carries() {
        fn of(rules: Vec<Box<dyn Rule>>) -> String {
            fingerprint_of(&rules, &Config::default())
        }
        let a = of(vec![Box::new(Always)]);
        assert_eq!(a, of(vec![Box::new(Always)]), "nothing moved, so the hash may not");
        assert_eq!(a.len(), 16);
        assert_eq!(rules_fingerprint(&Config::default()).len(), 16);

        // A config that moves which languages a rule reports on moves the hash
        // too: the cache keys on it, so without that a warm cache would go on
        // serving the rows written before the override.
        let mut config = Config::default();
        config.rules.insert(
            "off-for-php".into(),
            locrin_core::config::RuleOverride {
                enabled: None,
                severity: None,
                languages: Some(vec!["php".to_string()]),
            },
        );
        let rules: Vec<Box<dyn Rule>> = vec![Box::new(OffForPhp)];
        assert_ne!(fingerprint_of(&rules, &config), fingerprint_of(&rules, &Config::default()));

        assert_ne!(a, of(vec![Box::new(EveryLanguage)]), "a different id and a different language set");
        assert_ne!(a, of(vec![Box::new(OffByDefault)]), "a rule that ships off");
        assert_ne!(a, of(vec![Box::new(Locked)]), "a different severity and confidence");
        assert_ne!(a, of(vec![Box::new(Always), Box::new(EveryLanguage)]), "a rule added to the set");
        assert_ne!(
            of(vec![Box::new(PerLanguage(true))]),
            of(vec![Box::new(PerLanguage(false))]),
            "the per-language default is the one declaration no config can say"
        );
    }

    #[test]
    fn registry_lists_every_shipped_rule() {
        let files: Vec<ParsedFile> = vec![];
        let config = Config::default();
        let ix = Index::open_in_memory().unwrap();
        let entries = EntryPoints::detect(Path::new("."), &[]).unwrap();
        let ctx = test_ctx(&files, &config, Some(&ix), &entries);
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
                "boundary-violation",
                "swallowed-error",
                "test-no-assert",
                "test-newly-skipped",
                "secret-exposed",
                "weak-crypto",
                "injection-sink",
                "html-injection",
                "vulnerable-dependency",
                "supabase-service-role-in-client",
                "supabase-table-without-rls",
                "express-route-without-auth",
                "express-cors-wildcard-on-authenticated",
                "express-cookie-insecure"
            ]
        );
        assert!(run_all(&ctx).unwrap().is_empty(), "no files means no findings");
        assert_eq!(file_rules().len(), 16, "sixteen file rules and five graph rules");
        assert_eq!(
            graph_rules().iter().map(|r| r.id()).collect::<Vec<_>>(),
            vec![
                "dead-export",
                "dead-file",
                "boundary-violation",
                "vulnerable-dependency",
                "supabase-table-without-rls"
            ]
        );
    }

    /// The three rules whose default is a measurement and not a preference, in
    /// one test so a flip has a single place to argue with. The public
    /// benchmark (github.com/BilalEjaz/locrin-benchmark, 217 agent-written
    /// diffs, two-model labels) scored `swallowed-error` at 92 percent
    /// precision, above the spec 10.2 gate of 85, so it ships on from 0.6.0;
    /// `dead-file` (0 of 16 true) and `injection-sink` (0 of 17) stay off. Each
    /// rule's own doc carries the reason.
    #[test]
    fn the_measured_defaults_are_what_the_benchmark_scored() {
        let default_of = |id: &str| {
            all_rules().into_iter().find(|r| r.id() == id).expect("rule is in the registry").enabled_by_default()
        };
        assert!(default_of("swallowed-error"), "92 percent precision clears the gate");
        assert!(!default_of("dead-file"), "0 of 16 true");
        assert!(!default_of("injection-sink"), "0 of 17 true");
        assert!(!default_of("leftover-commented-code"), "0 of 5 true in public, 0 of 99 and then 0 of 8 on FastLift");
    }

    /// A locked rule, the way `secret-exposed` is locked (spec 4.3). Nothing but
    /// `locked` separates it from `Always`, so the test measures the locking and
    /// nothing else.
    struct Locked;
    impl Rule for Locked {
        fn id(&self) -> &'static str {
            "locked"
        }
        fn description(&self) -> &'static str {
            "test rule"
        }
        fn scope(&self) -> Scope {
            Scope::File
        }
        fn category(&self) -> Category {
            Category::Security
        }
        fn default_severity(&self) -> Severity {
            Severity::High
        }
        fn confidence(&self) -> Confidence {
            Confidence::High
        }
        fn locked(&self) -> bool {
            true
        }
        fn run(&self, ctx: &RuleContext) -> anyhow::Result<Vec<Finding>> {
            Ok(clean_files(ctx).map(|f| finding(self, f, 1, "hit", "remove it")).collect())
        }
    }

    #[test]
    fn a_locked_rule_ignores_config_overrides() {
        let file = parse_source(Path::new("src/a.ts"), "src/a.ts", "export const a = 1;\n".into()).unwrap();
        let files = vec![file];
        let mut config = Config::default();
        let off = locrin_core::config::RuleOverride {
            enabled: Some(false),
            severity: Some(Severity::Low),
            // The third `rules` key, and the one a locked rule refuses outright:
            // PHP is not a language `Locked` declares, and TypeScript is, so a
            // config carrying either must fail for being locked rather than for
            // naming a language the rule cannot read.
            languages: Some(vec!["php".into()]),
        };
        config.rules.insert("locked".into(), off.clone());
        config.rules.insert("always".into(), off);
        let ix = Index::open_in_memory().unwrap();
        let entries = EntryPoints::detect(Path::new("."), &[]).unwrap();
        let ctx = test_ctx(&files, &config, Some(&ix), &entries);

        // Built by hand rather than loaded, so the runner is measured on a
        // config that never went through the check below.
        let out = run_rules(&[Box::new(Locked)], &ctx).unwrap();
        assert_eq!(out.len(), 1, "a locked rule cannot be turned off or narrowed: {out:?}");
        assert_eq!(out[0].severity, Severity::High, "a locked rule keeps its own severity");
        assert_eq!(
            languages_of(&Locked, &config),
            Locked.languages().to_vec(),
            "a locked rule reports on every language it reads, whatever the config names"
        );
        assert!(rule_runs(&Locked, &config), "the cache asks the same question the runner does");

        assert!(run_rules(&[Box::new(Always)], &ctx).unwrap().is_empty(), "the same config turns an unlocked rule off");
        assert!(!rule_runs(&Always, &config));
    }

    /// `enabled` and `severity` on a locked rule are ignored silently, because a
    /// config that sets them still describes the run that happens. `languages`
    /// cannot be: it says where a rule reports, and a locked rule reports
    /// everywhere it reads, so a config naming a list is asking for a run the
    /// engine will not give it and is told so.
    #[test]
    fn a_languages_key_on_a_locked_rule_is_a_config_error() {
        for named in [vec!["php".to_string()], vec!["typescript".to_string()]] {
            let mut config = Config::default();
            config.rules.insert(
                "secret-exposed".into(),
                locrin_core::config::RuleOverride { enabled: None, severity: None, languages: Some(named.clone()) },
            );
            let err = validate_config(&config).unwrap_err().to_string();
            assert!(err.contains("rules.secret-exposed.languages"), "{err}");
            assert!(err.contains("locked"), "the reason is the lock, not the language: {err}");
        }
    }

    /// A rule that reports what its context carries, so a per-file context can be
    /// checked against the base it was built from.
    struct Reporter;
    impl Rule for Reporter {
        fn id(&self) -> &'static str {
            "reporter"
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
            let evidence = format!("{}|{}|{}", ctx.root.display(), ctx.offline, ctx.previous.skipped_tests.len());
            Ok(clean_files(ctx).map(|f| finding(self, f, 1, &evidence, "fix")).collect())
        }
    }

    /// The per-file contexts are the base with one file in them: a rule that
    /// reads the root, the offline flag or the previous state sees in the
    /// parallel pass exactly what it would see in one context over every file.
    #[test]
    fn run_file_rules_carries_root_offline_and_previous_from_the_base() {
        let a = parse_source(Path::new("src/a.ts"), "src/a.ts", "export const a = 1;\n".into()).unwrap();
        let b = parse_source(Path::new("src/b.ts"), "src/b.ts", "export const b = 2;\n".into()).unwrap();
        let files = vec![a, b];
        let config = Config::default();
        let entries = EntryPoints::detect(Path::new("."), &[]).unwrap();
        let mut previous = Previous::default();
        previous.skipped_tests.insert("src/a.test.ts".into(), HashSet::from(["is slow".to_string()]));
        let root = Path::new("some").join("repo");
        let base = RuleContext {
            files: &files,
            config: &config,
            index: None,
            entries: &entries,
            root: &root,
            offline: true,
            previous: &previous,
            rule_languages: ALL,
        };

        let out = run_file_rules(&[Box::new(Reporter)], &files, &base).unwrap();
        assert_eq!(out.len(), 2);
        let expected = format!("{}|true|1", root.display());
        assert!(out.iter().all(|f| f.evidence == expected), "got {out:?}, wanted {expected}");
    }
}
