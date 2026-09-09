mod common;

use common::{fixture, hits, rule_on, run_on};
use locrin_core::finding::{Category, Confidence, Severity};
use locrin_rules::injection_sink::InjectionSink;

const CODE_FIX: &str = "Do not build code from data; use a lookup table or JSON.parse";
const COMMAND_FIX: &str =
    "Pass arguments as an array to execFile or spawn without a shell, and validate or allow-list every piece";
const SQL_FIX: &str =
    "Use parameter placeholders ($1, ?) or a tagged template that parameterises, and pass values separately";

#[test]
fn every_sink_family_flags_its_lines_in_the_fixture() {
    let out = run_on(Box::new(InjectionSink), &fixture("injection_sink", "flag"), &rule_on("injection-sink"));
    assert_eq!(
        hits(&out),
        vec![
            ("a.ts".to_string(), 4),
            ("a.ts".to_string(), 8),
            ("a.ts".to_string(), 12),
            ("a.ts".to_string(), 16),
            ("a.ts".to_string(), 20),
            ("a.ts".to_string(), 24),
            ("a.ts".to_string(), 29),
            ("a.ts".to_string(), 34),
            ("a.ts".to_string(), 38),
            ("a.ts".to_string(), 42),
        ]
    );
    let evidence: Vec<&str> = out.iter().map(|f| f.evidence.as_str()).collect();
    assert_eq!(
        evidence,
        vec![
            "eval receives a variable",
            "new Function receives a concatenation",
            "shell command built from a template with substitutions",
            "shell command built from a concatenation",
            "SQL built from a template with substitutions reaches db.query",
            "SQL built from a concatenation reaches knex.raw",
            "SQL built from a variable reaches prisma.$queryRawUnsafe",
            "setTimeout receives a variable",
            "shell command built from a template with substitutions",
            "shell command built from a template with substitutions",
        ]
    );
    let fixes: Vec<&str> = out.iter().map(|f| f.fix.as_str()).collect();
    assert_eq!(
        fixes,
        vec![
            CODE_FIX,
            CODE_FIX,
            COMMAND_FIX,
            COMMAND_FIX,
            SQL_FIX,
            SQL_FIX,
            SQL_FIX,
            CODE_FIX,
            COMMAND_FIX,
            COMMAND_FIX
        ]
    );
}

/// Spec 7.1 metadata: every finding is a High-severity Security finding under
/// A03:2021, and the CWE says which sink family spoke. See the plan's global
/// constraints.
#[test]
fn every_finding_carries_the_security_metadata_for_its_family() {
    let out = run_on(Box::new(InjectionSink), &fixture("injection_sink", "flag"), &rule_on("injection-sink"));
    assert!(
        out.iter().all(|f| f.severity == Severity::High
            && f.category == Category::Security
            && f.confidence == Confidence::High
            && f.owasp.as_deref() == Some("A03:2021")),
        "{:?}",
        out.first()
    );
    let cwes: Vec<Option<&str>> = out.iter().map(|f| f.cwe.as_deref()).collect();
    assert_eq!(
        cwes,
        vec![
            Some("CWE-95"),
            Some("CWE-95"),
            Some("CWE-78"),
            Some("CWE-78"),
            Some("CWE-89"),
            Some("CWE-89"),
            Some("CWE-89"),
            Some("CWE-95"),
            Some("CWE-78"),
            Some("CWE-78"),
        ]
    );
}

/// Two findings in one file do not share an id: the anchor carries what the
/// finding says as well as the symbol it sits in.
#[test]
fn findings_have_distinct_ids() {
    let out = run_on(Box::new(InjectionSink), &fixture("injection_sink", "flag"), &rule_on("injection-sink"));
    let mut ids: Vec<&str> = out.iter().map(|f| f.id.as_str()).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), out.len(), "ten findings, ten ids");
}

/// A literal argument, a tagged template, a parameterised query, an argv-array
/// spawn and a function passed to `setTimeout` are the safe spellings of every
/// sink the rule knows.
#[test]
fn literals_tagged_templates_and_parameterised_calls_are_left_alone() {
    let out = run_on(Box::new(InjectionSink), &fixture("injection_sink", "clean"), &rule_on("injection-sink"));
    assert!(out.is_empty(), "{:?}", hits(&out));
}

/// The four judgement calls. Two string literals joined with `+` are a
/// constant, so the `exec` on line 4 is not a finding. `db.query(q)` where `q`
/// is a plain string stands at Medium: the identifier reaches a SQL sink, but
/// its declaration says nothing was interpolated. The allowed line is dropped
/// by the engine, not by the rule. Interpolated SQL inside a test file is
/// advisory rather than blocking, so it stands at Medium however it was built:
/// see the module doc.
#[test]
fn a_constant_is_not_a_finding_and_a_plain_variable_is_only_a_medium_one() {
    let out = run_on(Box::new(InjectionSink), &fixture("injection_sink", "edge"), &rule_on("injection-sink"));
    assert_eq!(hits(&out), vec![("c.ts".to_string(), 9), ("seed.test.ts".to_string(), 2)]);
    assert_eq!(out[0].confidence, Confidence::Medium, "{}", out[0].evidence);
    assert_eq!(out[0].evidence, "SQL built from a variable reaches db.query");
    assert_eq!(out[1].confidence, Confidence::Medium, "{}", out[1].evidence);
    assert_eq!(out[1].evidence, "SQL built from a template with substitutions reaches db.query");
}
