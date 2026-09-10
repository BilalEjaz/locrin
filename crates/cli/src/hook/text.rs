use locrin_core::finding::{Status, Verdict};

/// The agent payload as plain lines: the verdict, then one line per finding,
/// then how many findings the cap left out.
///
/// Lines rather than JSON because an agent reads this as the prose of a
/// `reason` field, where the punctuation of a nested object is noise it has to
/// parse before it can act. The counts come from the verdict rather than from
/// the findings still attached to it, so a capped verdict still states the true
/// totals and the last line says how many it is not showing.
pub fn feedback(v: &Verdict) -> String {
    let total = v.high + v.medium + v.low;
    let mut lines = Vec::with_capacity(v.findings.len() + 2);
    lines.push(match v.status {
        // A pass has no blocking and no advisory count worth splitting, so it
        // says the one number it has.
        Status::Pass => format!("PASS  0 finding(s) in {} ms", v.duration_ms),
        Status::Advisory | Status::Block => format!(
            "{}  {} blocking, {} advisory, {} finding(s) in {} ms",
            if v.status == Status::Block { "BLOCK" } else { "ADVISORY" },
            v.blocking,
            total - v.blocking,
            total,
            v.duration_ms
        ),
    });
    for f in &v.findings {
        lines.push(format!("{}  {}:{}  {}  ->  {}", f.rule, f.file, f.span.start_line, f.evidence, f.fix));
    }
    if v.truncated > 0 {
        lines.push(format!("+{} more", v.truncated));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use locrin_core::finding::*;

    fn f(
        rule: &str,
        file: &str,
        line: u32,
        evidence: &str,
        fix: &str,
        severity: Severity,
        confidence: Confidence,
    ) -> Finding {
        Finding {
            id: make_id(rule, file, evidence),
            rule: rule.into(),
            category: Category::Erosion,
            severity,
            confidence,
            file: file.into(),
            span: Span { start_line: line, start_col: 0, end_line: line, end_col: 1 },
            evidence: evidence.into(),
            fix: fix.into(),
            related: vec![],
            owasp: None,
            cwe: None,
        }
    }

    #[test]
    fn renders_verdict_line_then_one_line_per_finding() {
        let findings = vec![
            f(
                "leftover-debug",
                "src/a.ts",
                12,
                "console.log(\"x\")",
                "Remove the debug statement or mark the line locrin:allow",
                Severity::High,
                Confidence::High,
            ),
            f(
                "secret-exposed",
                "src/k.ts",
                3,
                "AWS access key id, AKIA... (20 chars)",
                "Move the credential to an environment variable and rotate it",
                Severity::High,
                Confidence::High,
            ),
            f(
                "unused-import",
                "src/a.ts",
                1,
                "import { x } from \"./x\"",
                "Delete the import",
                Severity::Low,
                Confidence::Medium,
            ),
        ];
        let v = Verdict::from_findings(findings, 143);
        assert_eq!(
            feedback(&v),
            "BLOCK  2 blocking, 1 advisory, 3 finding(s) in 143 ms\n\
             leftover-debug  src/a.ts:12  console.log(\"x\")  ->  Remove the debug statement or mark the line locrin:allow\n\
             secret-exposed  src/k.ts:3  AWS access key id, AKIA... (20 chars)  ->  Move the credential to an environment variable and rotate it\n\
             unused-import  src/a.ts:1  import { x } from \"./x\"  ->  Delete the import"
        );
    }

    #[test]
    fn says_how_many_more_when_truncated() {
        let findings: Vec<Finding> = (1..=15)
            .map(|i| {
                f("leftover-debug", "src/a.ts", i, "console.log(1)", "Remove it", Severity::High, Confidence::High)
            })
            .collect();
        let v = Verdict::from_findings(findings, 12).capped(10);
        let text = feedback(&v);
        assert!(text.ends_with("\n+5 more"), "{text}");
        // The cap costs lines, not counts: the verdict line still says fifteen.
        assert!(text.starts_with("BLOCK  15 blocking, 0 advisory, 15 finding(s) in 12 ms\n"), "{text}");
        assert_eq!(text.lines().count(), 12);
    }

    #[test]
    fn pass_is_one_line() {
        let v = Verdict::from_findings(vec![], 40);
        assert_eq!(feedback(&v), "PASS  0 finding(s) in 40 ms");
    }

    #[test]
    fn advisory_counts_the_findings_that_do_not_block() {
        let findings = vec![
            f("todo-marker", "src/a.ts", 4, "TODO fix", "Remove it", Severity::Low, Confidence::Medium),
            f("unused-import", "src/a.ts", 1, "import { x }", "Delete it", Severity::Low, Confidence::Medium),
        ];
        let v = Verdict::from_findings(findings, 90);
        assert!(
            feedback(&v).starts_with("ADVISORY  0 blocking, 2 advisory, 2 finding(s) in 90 ms\n"),
            "{}",
            feedback(&v)
        );
    }
}
