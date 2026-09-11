//! Markdown summary for pull-request comments: one marker line the action
//! finds again, the verdict, and at most ten findings. The SARIF file is the
//! complete list; this is the glanceable one.

use locrin_core::finding::{Confidence, Status, Verdict};

pub const MARKER: &str = "<!-- locrin-report -->";
pub const CAP: usize = 10;

fn status_word(s: Status) -> &'static str {
    match s {
        Status::Pass => "PASS",
        Status::Advisory => "ADVISORY",
        Status::Block => "BLOCK",
    }
}

/// A table cell cannot carry a backtick or a pipe unescaped: the first would
/// close the code span the cell is wrapped in, the second would split the row.
fn cell(text: &str) -> String {
    text.replace('`', "'").replace('|', "\\|")
}

pub fn render(v: &Verdict, version: &str, run_url: Option<&str>) -> String {
    let capped = v.clone().capped(CAP);
    let total = v.high + v.medium + v.low;
    let mut out = format!("{MARKER}\n### Locrin: {}\n\n", status_word(v.status));
    out.push_str(&format!(
        "{total} finding(s): {} high, {} medium, {} low. {} blocking. {} ms. locrin {version}\n\n",
        v.high, v.medium, v.low, v.blocking, v.duration_ms
    ));
    if capped.findings.is_empty() {
        out.push_str("No findings.\n");
    } else {
        out.push_str("| | File | Rule | Evidence | Fix |\n|---|---|---|---|---|\n");
        for f in &capped.findings {
            let kind = if f.confidence == Confidence::High { "block" } else { "advise" };
            out.push_str(&format!(
                "| {kind} | `{}:{}` | `{}` | `{}` | {} |\n",
                cell(&f.file),
                f.span.start_line,
                cell(&f.rule),
                cell(&f.evidence),
                cell(&f.fix)
            ));
        }
        if capped.truncated > 0 {
            out.push_str(&format!("\n... and {} more in the SARIF upload.\n", capped.truncated));
        }
    }
    if let Some(url) = run_url {
        out.push_str(&format!("\nDetails: {url}\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use locrin_core::finding::*;

    fn f(rule: &str, sev: Severity, conf: Confidence, file: &str, line: u32, evidence: &str) -> Finding {
        Finding {
            id: format!("{line:016x}"),
            rule: rule.into(),
            category: Category::Erosion,
            severity: sev,
            confidence: conf,
            file: file.into(),
            span: Span { start_line: line, start_col: 0, end_line: line, end_col: 1 },
            evidence: evidence.into(),
            fix: "Fix it".into(),
            related: vec![],
            owasp: None,
            cwe: None,
        }
    }

    #[test]
    fn starts_with_the_marker_and_status() {
        let v = Verdict::from_findings(vec![], 3);
        let s = render(&v, "0.3.0", None);
        assert!(s.starts_with("<!-- locrin-report -->\n### Locrin: PASS\n"));
        assert!(s.contains("No findings."));
        assert!(!s.contains("Details:"));
    }

    #[test]
    fn table_marks_blocking_rows_and_escapes_cells() {
        let v = Verdict::from_findings(
            vec![
                f("leftover-debug", Severity::High, Confidence::High, "src/a.ts", 2, "console.log(`x|y`)"),
                f("leftover-agent-marker", Severity::Low, Confidence::Medium, "src/a.ts", 3, "// TODO x"),
            ],
            15,
        );
        let s = render(&v, "0.3.0", Some("https://example/run/1"));
        assert!(s.contains("### Locrin: BLOCK"));
        assert!(s.contains("2 finding(s): 1 high, 0 medium, 1 low. 1 blocking. 15 ms. locrin 0.3.0"));
        assert!(s.contains("| block | `src/a.ts:2` | `leftover-debug` | `console.log('x\\|y')` | Fix it |"));
        assert!(s.contains("| advise | `src/a.ts:3` | `leftover-agent-marker` |"));
        assert!(s.ends_with("Details: https://example/run/1\n"));
    }

    #[test]
    fn caps_at_ten_and_points_to_sarif() {
        let many: Vec<Finding> =
            (0..13).map(|i| f("r", Severity::Low, Confidence::Medium, "src/a.ts", i, "e")).collect();
        let v = Verdict::from_findings(many, 1);
        let s = render(&v, "0.3.0", None);
        assert_eq!(s.matches("| advise |").count(), 10);
        assert!(s.contains("... and 3 more in the SARIF upload."));
    }
}
