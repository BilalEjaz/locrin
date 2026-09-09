//! Flags a test case this change stopped running. A skip is a decision, and the
//! decision that matters is the one somebody makes today: a suite that has
//! carried the same three `it.skip`s for two years does not need telling, while
//! the skip added an hour ago to get a branch green is the one nobody will
//! remember to undo. So the rule reports a change rather than a state, and what
//! it compares against is [`locrin_core::previous::Previous`]: for each skipped
//! case, flagged unless the previous version of the file already skipped a case
//! by that name.
//!
//! ## The three sources of "previous"
//!
//! The rule reads the snapshot; the CLI decides what goes in it, from one of
//! three places:
//!
//! - **The index.** An ordinary run captures what the index remembered about
//!   each file it is about to re-record, before the record overwrites it. That
//!   is the version the last run saw, so on a developer's machine "previous"
//!   means "since I last ran locrin", which is the edit in front of them.
//! - **Git.** A `--base` or `--since` run overrides the index for every file in
//!   the diff with what the file held at the base revision. That is the "before"
//!   a pull request is actually judged against, and on a fresh CI clone it is the
//!   only witness there is. A base version that will not parse says nothing and
//!   records nothing, so a recovered tree cannot call an old skip new.
//! - **Nothing.** A first run, or a file the index has never seen, has no
//!   previous version. Every skip in it is reported once and the baseline
//!   absorbs the legacy, which is what makes the rule adoptable on a repository
//!   that already has a hundred of them. The evidence says so: `(newly)` is
//!   appended only when a previous version was known, so a finding from a file
//!   with no history does not claim a change nobody can see.
//!
//! ## The finding outlives the run that made it
//!
//! Findings for a file rule are cached per file, and the cache key does not
//! include the previous snapshot: it cannot, because the snapshot describes the
//! run rather than the file. So a cached row carries the `previous` of the run
//! that wrote it. A skip reported once therefore keeps being served on every
//! later run until the file is next edited, at which point the file is
//! re-parsed, the index's memory of it says the skip was already there, and the
//! finding stops. This is deliberate: a finding that vanished on the next run
//! before anybody looked at it would be worse than useless in a hook, and the
//! repository-wide answer to a skip somebody has decided to live with is the
//! baseline, not a silent expiry.

use std::collections::HashSet;

use locrin_core::finding::{Category, Confidence, Finding, Severity};
use locrin_core::parse::ParsedFile;
use locrin_core::previous::Previous;
use locrin_core::testcases::{extract, is_test_file};

use crate::{clean_files, finding_at, line_span, Rule, RuleContext, Scope};

pub struct TestNewlySkipped;

const FIX: &str = "Re-enable the test or delete it; a skipped test is a decision that needs an owner";

fn scan(rule: &TestNewlySkipped, file: &ParsedFile, previous: &Previous) -> Vec<Finding> {
    if !is_test_file(&file.rel) {
        return Vec::new();
    }
    // None is "no previous version of this file", which is not the same as one
    // that skipped nothing: the first means every skip is unjudgeable and gets
    // reported once, the second means every skip here is genuinely new.
    let was: Option<&HashSet<String>> = previous.skipped_tests.get(&file.rel);
    extract(file)
        .into_iter()
        .filter(|case| case.skipped && !was.is_some_and(|s| s.contains(&case.name)))
        .map(|case| {
            // The name, not the line: a skip keeps its identity when the cases
            // above it grow or move.
            let anchor = format!("skip\x1f{}", case.name);
            let newly = if was.is_some() { " (newly)" } else { "" };
            let evidence = format!("test \"{}\" is skipped{}", case.name, newly);
            finding_at(rule, &file.rel, line_span(file, case.line), &anchor, &evidence, FIX)
        })
        .collect()
}

impl Rule for TestNewlySkipped {
    fn id(&self) -> &'static str {
        "test-newly-skipped"
    }
    fn description(&self) -> &'static str {
        "A test case this change stopped running, so the suite still passes with the case's ground uncovered"
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
    /// High: whether a case is skipped is written in the source in one of a few
    /// fixed forms, and whether the previous version skipped it is a set lookup.
    /// Neither step guesses. The rule under-reports where the extractor does not
    /// recognise a case form, but a case it does report is skipped.
    fn confidence(&self) -> Confidence {
        Confidence::High
    }

    fn run(&self, ctx: &RuleContext) -> anyhow::Result<Vec<Finding>> {
        Ok(clean_files(ctx).flat_map(|file| scan(self, file, ctx.previous)).collect())
    }
}
