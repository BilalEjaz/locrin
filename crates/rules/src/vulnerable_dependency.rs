//! Flags an installed npm package that a published advisory says is vulnerable.
//!
//! The rule is a thin reading of two core modules. `lockfile::read` says what an
//! install of this repository puts on disk and which line of the lockfile
//! declares each package; `osv::check` says which of those versions osv.dev has
//! an advisory for, from the network when the run allows it and from the index's
//! snapshot otherwise. Nothing here parses a package or resolves a version
//! range: the answer belongs to the advisory database, and this file's whole job
//! is to turn it into findings a reader can act on.
//!
//! Graph scope, so it runs on every whole-repository pass. A file rule would
//! never fire, because the lockfile is not a source file the engine parses, and
//! the answer can change without any file changing at all: an advisory published
//! this morning affects a repository nobody has touched.
//!
//! Which is also the scoping contract, and the CLI enforces it before the rule
//! is asked to run (see `lock_in_scope` in `crates/cli/src/run.rs`). A run
//! narrowed to a scope answers for the lockfile only when the scope names the
//! lockfile itself: a diff whose git file list includes it, or a path argument
//! naming it. Any other scope skips the rule outright, reading neither the
//! lockfile nor the snapshot and making no request, because nothing else can put
//! the lockfile in scope: it is in no walk, no index and no import
//! neighbourhood, so every finding would have been discarded after being paid
//! for. `--changed` is therefore never a run that reports advisories, whatever
//! was done to the lockfile: that scope is the index's watermark and the index
//! holds source files only.
//!
//! Severity comes from the advisory rather than from the rule, which is why
//! [`crate::run_rules`] overwrites a finding's severity only when the config
//! asks it to. A repository that disagrees with the ratings can still set one
//! level for all of them with `[rules.vulnerable-dependency] severity = "..."`.
//!
//! Confidence says how much of the advisory the run actually has. A High-rated
//! advisory that names the version to upgrade to is a complete instruction and
//! reads High; anything else, including a rating this engine had to guess at
//! because the detail document was unavailable, reads Medium.

use locrin_core::finding::{Category, Confidence, Finding, Severity, Span};
use locrin_core::{lockfile, osv};

use crate::{finding_at, Rule, RuleContext, Scope};

/// How much of an advisory summary the evidence carries. Summaries are usually
/// one line; a few are a paragraph, and a finding is a pointer to the advisory,
/// not a copy of it.
const SUMMARY_MAX: usize = 120;

pub struct VulnerableDependency;

/// The fetch an offline run is handed. `osv::check` never calls it when
/// `offline` is set, so this is a statement rather than a fallback: the one
/// place in the engine that opens a socket is unreachable from a run that was
/// told not to, and every test in the rules crate runs offline.
fn no_network(url: &str, _body: Option<&str>) -> anyhow::Result<String> {
    anyhow::bail!("offline: {url} was not requested")
}

/// The advisory's own rating, read as a severity. An advisory that does not rate
/// itself, or rates itself in words this engine does not know, is Low: it is
/// still worth reporting and it is not worth blocking a build over.
fn severity_of(rating: &str) -> Severity {
    match rating {
        "CRITICAL" | "HIGH" => Severity::High,
        "MODERATE" => Severity::Medium,
        _ => Severity::Low,
    }
}

/// The summary, cut to [`SUMMARY_MAX`] characters including the ellipsis. Cut by
/// characters and not by bytes: advisory summaries carry names and quotation
/// marks that are not ASCII, and slicing one mid-character would panic.
fn short(summary: &str) -> String {
    if summary.chars().count() <= SUMMARY_MAX {
        return summary.to_string();
    }
    let head: String = summary.chars().take(SUMMARY_MAX - 3).collect();
    format!("{head}...")
}

impl Rule for VulnerableDependency {
    fn id(&self) -> &'static str {
        "vulnerable-dependency"
    }
    fn description(&self) -> &'static str {
        "Installed dependency with a published security advisory"
    }
    fn scope(&self) -> Scope {
        Scope::Graph
    }
    fn category(&self) -> Category {
        Category::Security
    }

    /// The level a finding carries when nothing else decides it. Every finding
    /// this rule produces replaces it with the advisory's own rating, so this is
    /// what the rule is worth in the abstract, for the documentation and the
    /// SARIF rule entry rather than for any one finding.
    fn default_severity(&self) -> Severity {
        Severity::High
    }
    fn confidence(&self) -> Confidence {
        Confidence::Medium
    }

    fn run(&self, ctx: &RuleContext) -> anyhow::Result<Vec<Finding>> {
        let Some(lock) = lockfile::read(ctx.root)? else { return Ok(Vec::new()) };
        let fetch: &dyn Fn(&str, Option<&str>) -> anyhow::Result<String> =
            if ctx.offline { &no_network } else { &osv::http_fetch };
        let outcome = osv::check(ctx.index()?, &lock, ctx.offline, fetch)?;
        // The rule runs once per run, so each warning is printed once. They go to
        // stderr and not into the findings: a stale snapshot or an unreachable
        // registry is something the reader should know about the run, not
        // something the repository has to fix (spec 9).
        for warning in &outcome.warnings {
            eprintln!("warning: {warning}");
        }

        let mut out = Vec::new();
        for hit in outcome.hits {
            let (package, advisory) = (hit.package, hit.advisory);
            let severity = severity_of(&advisory.severity);
            let confidence = if advisory.fixed.is_some() && severity == Severity::High {
                Confidence::High
            } else {
                Confidence::Medium
            };
            // Columns are zero: the finding is about the whole entry, and a
            // lockfile entry's name and version sit on different lines in two of
            // the three formats.
            let span = Span { start_line: package.line, start_col: 0, end_line: package.line, end_col: 0 };
            // The advisory id joins the package name in the anchor so that a
            // package with two advisories is two findings, and so that a finding
            // keeps its id when the lockfile is regenerated and the entry moves.
            // The version is there for the same reason: an install tree holding
            // three copies of one package at three versions is three findings
            // with three upgrades to make, and without the version they shared
            // one id, so accepting one of them accepted all three unread.
            let anchor = format!("{}\x1f{}\x1f{}", package.name, package.version, advisory.id);
            let evidence = format!(
                "{} {}: {} ({}) {}",
                package.name,
                package.version,
                advisory.id,
                advisory.severity,
                short(&advisory.summary)
            );
            // Three sentences, because the reader has to act on three different
            // situations. There is a version to move to; or the advisory covers
            // this version and has published no fix for the branch it is on; or
            // the advisory's own ranges do not cover this version at all, which
            // is the batch endpoint and the detail document disagreeing and is
            // not the same claim as "no fix exists".
            let fix = match &advisory.fixed {
                Some(fixed) => format!("Upgrade {} to {fixed}", package.name),
                None if advisory.outside_every_range => {
                    format!(
                        "No fixed version applies to this version; review {} and pin or replace the package",
                        advisory.id
                    )
                }
                None => format!("No fixed version published; review {} and pin or replace the package", advisory.id),
            };
            // An advisory whose detail document was unavailable has no summary,
            // which would otherwise leave the evidence ending in a space.
            let mut finding = finding_at(self, &lock.rel, span, &anchor, evidence.trim_end(), &fix);
            finding.severity = severity;
            finding.confidence = confidence;
            finding.owasp = Some("A06:2021".to_string());
            finding.cwe = Some("CWE-1395".to_string());
            out.push(finding);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_advisorys_rating_decides_the_severity_and_an_unknown_one_is_low() {
        assert_eq!(severity_of("CRITICAL"), Severity::High);
        assert_eq!(severity_of("HIGH"), Severity::High);
        assert_eq!(severity_of("MODERATE"), Severity::Medium);
        assert_eq!(severity_of("LOW"), Severity::Low);
        assert_eq!(severity_of("UNKNOWN"), Severity::Low);
        assert_eq!(severity_of(""), Severity::Low, "a rating this engine cannot read is not a High finding");
    }

    #[test]
    fn a_long_summary_is_cut_to_the_limit_including_the_ellipsis() {
        let short_enough = "a".repeat(SUMMARY_MAX);
        assert_eq!(short(&short_enough), short_enough, "exactly the limit is not cut");

        let long = "b".repeat(SUMMARY_MAX + 1);
        let cut = short(&long);
        assert_eq!(cut.chars().count(), SUMMARY_MAX);
        assert!(cut.ends_with("..."), "{cut}");

        // Cutting by bytes here would panic rather than shorten.
        let accented = "é".repeat(SUMMARY_MAX + 40);
        assert_eq!(short(&accented).chars().count(), SUMMARY_MAX);
    }
}
