//! Flags a test case that runs code and checks nothing. A case with no
//! assertion passes as long as its body does not throw, so it reports green
//! while proving almost nothing; the suite's coverage number counts it all the
//! same.
//!
//! What counts as an assertion is [`locrin_core::testcases`]: a call to
//! `expect` or `assert` in any form, a chai `.should` chain, or a call to a
//! same-file function whose own body asserts. That is one file's syntax and
//! nothing more, which is why the rule ships at Medium confidence and where its
//! false positives come from:
//!
//! - **Helpers are followed one level, not two.** A case calling a helper that
//!   asserts is clean; a case calling a helper that calls a second helper that
//!   asserts is reported, because the first helper's own body holds no
//!   assertion. Chasing further needs the call graph, which is release two.
//! - **An assertion in another file is invisible.** A shared `expectRowShape`
//!   imported from a test-utils module is an ordinary call as far as this file
//!   is concerned, so a case that delegates all of its checks across a file
//!   boundary is reported. Same cause, same fix.
//! - **A custom matcher named neither `expect` nor `assert` is invisible.** A
//!   project whose house assertion is `verify(x).equals(1)` or `t.is(a, b)`
//!   (ava, tap) reads as assertion-free throughout. Matching on every call that
//!   might be a matcher would flag nothing at all, so the rule holds to the
//!   three dialects it can name.
//! - **Some case forms are not recognised.** `it.skip.each(table)(...)` is not
//!   read as a case, because its callee is a member of a member rather than of
//!   an identifier. It is skipped, so the rule would not report it either way,
//!   but a `describe.each`-generated case or a runner-specific wrapper is
//!   likewise unseen: the rule under-reports there rather than guessing.
//!
//! One thing the rule reports on purpose, which is not a false positive: a case
//! whose only check is that its body did not throw. A smoke test is a real
//! thing to want, so the fix does not say to delete the case; it says to write
//! the intent down with `expect.assertions(0)`, which is what lets the next
//! reader tell a deliberate smoke test from a case someone left unfinished.

use locrin_core::finding::{Category, Confidence, Finding, Severity};
use locrin_core::parse::ParsedFile;
use locrin_core::testcases::{extract, is_test_file};

use crate::{clean_files, finding_at, line_span, Rule, RuleContext, Scope};

pub struct TestNoAssert;

const FIX: &str =
    "Assert on the outcome, or mark the case as a smoke test with expect.assertions(0) so the intent is explicit";

fn scan(rule: &TestNoAssert, file: &ParsedFile) -> Vec<Finding> {
    if !is_test_file(&file.rel) {
        return Vec::new();
    }
    extract(file)
        .into_iter()
        .filter(|case| case.assertions == 0 && !case.skipped)
        .map(|case| {
            // The name, not the line: a case keeps its identity when the cases
            // above it grow or move.
            let anchor = format!("case\x1f{}", case.name);
            let evidence = format!("test \"{}\" has no assertion", case.name);
            finding_at(rule, &file.rel, line_span(file, case.line), &anchor, &evidence, FIX)
        })
        .collect()
}

impl Rule for TestNoAssert {
    fn id(&self) -> &'static str {
        "test-no-assert"
    }
    fn description(&self) -> &'static str {
        "A test case that runs code without asserting anything, so it passes unless the body throws"
    }
    fn scope(&self) -> Scope {
        Scope::File
    }
    fn category(&self) -> Category {
        Category::Erosion
    }
    fn default_severity(&self) -> Severity {
        Severity::Low
    }
    /// Medium: the three assertion dialects and the single level of helper the
    /// extractor follows cover how most suites are written, but every blind
    /// spot in the module doc turns a case that does assert into a finding.
    fn confidence(&self) -> Confidence {
        Confidence::Medium
    }

    fn run(&self, ctx: &RuleContext) -> anyhow::Result<Vec<Finding>> {
        Ok(clean_files(ctx).flat_map(|file| scan(self, file)).collect())
    }
}
