use locrin_core::finding::Verdict;

/// How many findings an agent gets. An agent acts on a handful of items at a
/// time, and the capped verdict still carries the full counts, so the cap costs
/// context rather than information.
pub const CAP: usize = 10;

/// Renders a verdict as one line of compact JSON for an agent.
///
/// Only the verdict is serialised, so the output never contains file contents:
/// a finding carries its own evidence line and nothing more.
pub fn render(v: &Verdict) -> String {
    let capped = v.clone().capped(CAP);
    serde_json::to_string(&capped).expect("verdict serialises")
}

#[cfg(test)]
mod tests {
    use super::*;
    use locrin_core::finding::*;

    fn f(line: u32) -> Finding {
        Finding {
            id: format!("{:016x}", line),
            rule: "leftover-debug".into(),
            category: Category::Erosion,
            severity: Severity::High,
            confidence: Confidence::High,
            file: "src/a.ts".into(),
            span: Span { start_line: line, start_col: 0, end_line: line, end_col: 1 },
            evidence: "e".into(),
            fix: "f".into(),
            related: vec![],
            owasp: None,
            cwe: None,
        }
    }

    #[test]
    fn caps_at_ten_and_keeps_counts() {
        let v = Verdict::from_findings((0..15).map(f).collect(), 5);
        let s = render(&v);
        let parsed: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed["status"], "block");
        assert_eq!(parsed["high"], 15);
        assert_eq!(parsed["findings"].as_array().unwrap().len(), 10);
        assert_eq!(parsed["truncated"], 5);
        assert!(!s.contains('\n'));
    }
}
