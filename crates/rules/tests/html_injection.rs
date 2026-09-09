mod common;

use common::{fixture, hits, run_on};
use locrin_core::config::Config;
use locrin_core::finding::{Category, Confidence, Severity};
use locrin_rules::html_injection::HtmlInjection;

const FIX: &str =
    "Render text through the framework (textContent, JSX children) or sanitise with DOMPurify before injecting";

#[test]
fn every_sink_flags_its_line_in_the_fixture() {
    let out = run_on(Box::new(HtmlInjection), &fixture("html_injection", "flag"), &Config::default());
    assert_eq!(
        hits(&out),
        vec![
            ("a.tsx".to_string(), 2),
            ("a.tsx".to_string(), 6),
            ("a.tsx".to_string(), 10),
            ("a.tsx".to_string(), 14),
            ("a.tsx".to_string(), 18),
            ("a.tsx".to_string(), 22),
            ("a.tsx".to_string(), 26),
        ]
    );
    let evidence: Vec<&str> = out.iter().map(|f| f.evidence.as_str()).collect();
    assert_eq!(
        evidence,
        vec![
            "dangerouslySetInnerHTML receives a property",
            "innerHTML receives a variable",
            "outerHTML receives a template with substitutions",
            "insertAdjacentHTML receives a concatenation",
            "document.write receives a variable",
            "$().html receives a variable",
            "innerHTML receives a call",
        ]
    );
    assert!(out.iter().all(|f| f.fix == FIX), "{:?}", out.first());
}

/// Spec 7.1 metadata: every finding is a High-severity, Medium-confidence
/// Security finding under A03:2021 and CWE-79. See the plan's global
/// constraints.
#[test]
fn every_finding_carries_the_security_metadata() {
    let out = run_on(Box::new(HtmlInjection), &fixture("html_injection", "flag"), &Config::default());
    assert!(
        out.iter().all(|f| f.severity == Severity::High
            && f.category == Category::Security
            && f.confidence == Confidence::Medium
            && f.owasp.as_deref() == Some("A03:2021")
            && f.cwe.as_deref() == Some("CWE-79")),
        "{:?}",
        out.first()
    );
}

/// Two sinks in one file do not share an id: the anchor carries what the
/// finding says as well as the symbol it sits in.
#[test]
fn findings_have_distinct_ids() {
    let out = run_on(Box::new(HtmlInjection), &fixture("html_injection", "flag"), &Config::default());
    let mut ids: Vec<&str> = out.iter().map(|f| f.id.as_str()).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), out.len(), "seven findings, seven ids");
}

/// The safe spellings: markup written here as a literal, a value put through
/// DOMPurify or a sanitiser named like one (`escapeHtml` included, which the
/// `unescape` exclusion must not catch), and text set through `textContent`
/// rather than as markup.
#[test]
fn literals_sanitiser_calls_and_text_content_are_left_alone() {
    let out = run_on(Box::new(HtmlInjection), &fixture("html_injection", "clean"), &Config::default());
    assert!(out.is_empty(), "{:?}", hits(&out));
}

/// The two judgement calls. A template with nothing interpolated is a fixed
/// string however it is quoted, so the `__html` on line 2 is not a finding. The
/// `innerHTML` on line 6 is one, and it is the engine rather than the rule that
/// drops it for the marker on its line.
#[test]
fn a_template_without_substitutions_is_a_literal_and_the_marker_is_honoured() {
    let out = run_on(Box::new(HtmlInjection), &fixture("html_injection", "edge"), &Config::default());
    assert!(out.is_empty(), "{:?}", hits(&out));
}
