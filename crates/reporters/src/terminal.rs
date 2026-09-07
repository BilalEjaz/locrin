use std::collections::BTreeMap;

use locrin_core::finding::{Finding, Status, Verdict};

fn status_word(s: Status) -> &'static str {
    match s {
        Status::Pass => "PASS",
        Status::Advisory => "ADVISORY",
        Status::Block => "BLOCK",
    }
}

/// The `severity/confidence` pair a finding line carries, lower cased so it
/// reads as prose rather than as Rust enum names.
fn sev(f: &Finding) -> String {
    format!("{:?}/{:?}", f.severity, f.confidence).to_lowercase()
}

/// Renders a verdict for a human terminal: one header line, then the findings
/// grouped by file in path order, then a footer when the verdict was capped.
///
/// The counts in the header come from the verdict, not from the findings it
/// still carries, so a capped verdict reports the true totals and the footer
/// says how many lines are not shown.
///
/// Each finding line ends with its id, because `locrin baseline accept` takes an
/// id and the terminal is where an operator reads the finding they want to
/// accept. Without it the only way to get the id is to re-run with `--json`.
pub fn render(v: &Verdict) -> String {
    let total = v.high + v.medium + v.low;
    let mut out = format!(
        "{}  {} finding(s): {} high, {} medium, {} low  ({} ms)\n",
        status_word(v.status),
        total,
        v.high,
        v.medium,
        v.low,
        v.duration_ms
    );
    let mut by_file: BTreeMap<&str, Vec<&Finding>> = BTreeMap::new();
    for f in &v.findings {
        by_file.entry(f.file.as_str()).or_default().push(f);
    }
    for (file, fs) in by_file {
        out.push_str(file);
        out.push('\n');
        for f in fs {
            out.push_str(&format!(
                "  L{}  {}  {}  {}  id={}\n",
                f.span.start_line,
                f.rule,
                sev(f),
                f.evidence,
                f.id
            ));
            out.push_str(&format!("        fix: {}\n", f.fix));
        }
    }
    if v.truncated > 0 {
        out.push_str(&format!("... and {} more\n", v.truncated));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use locrin_core::finding::*;

    fn f(file: &str, line: u32) -> Finding {
        Finding {
            id: "0123456789abcdef".into(),
            rule: "leftover-debug".into(),
            category: Category::Erosion,
            severity: Severity::High,
            confidence: Confidence::High,
            file: file.into(),
            span: Span { start_line: line, start_col: 0, end_line: line, end_col: 10 },
            evidence: "console.log(1)".into(),
            fix: "Remove it".into(),
            related: vec![],
            owasp: None,
            cwe: None,
        }
    }

    #[test]
    fn renders_header_groups_and_fix() {
        let v = Verdict::from_findings(vec![f("src/b.ts", 4), f("src/a.ts", 9)], 12);
        let s = render(&v);
        assert!(s.starts_with("BLOCK  2 finding(s): 2 high, 0 medium, 0 low  (12 ms)"));
        let a = s.find("src/a.ts").unwrap();
        let b = s.find("src/b.ts").unwrap();
        assert!(a < b);
        assert!(s.contains("  L9  leftover-debug  high/high  console.log(1)  id=0123456789abcdef"), "{s}");
        assert!(s.contains("        fix: Remove it"));
    }

    #[test]
    fn renders_pass_and_truncation() {
        assert!(render(&Verdict::from_findings(vec![], 3)).starts_with("PASS  0 finding(s)"));
        let many: Vec<Finding> = (0..12).map(|i| f("src/a.ts", i)).collect();
        let s = render(&Verdict::from_findings(many, 3).capped(10));
        assert!(s.contains("... and 2 more"));
    }
}
