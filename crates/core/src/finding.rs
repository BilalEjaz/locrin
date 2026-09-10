use serde::{Deserialize, Serialize};

/// Ordering matters: `Verdict::from_findings` sorts ascending on this as its
/// second key, so the most severe variant must come first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    High,
    Medium,
    Low,
}

/// Ordering matters, same as `Severity`, and this is the *primary* sort key:
/// `Verdict::from_findings` sorts ascending and confidence is the gate, so
/// `High` must come first and survive any later cap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    High,
    Medium,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Erosion,
    Security,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Pass,
    Advisory,
    Block,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub id: String,
    pub rule: String,
    pub category: Category,
    pub severity: Severity,
    pub confidence: Confidence,
    pub file: String,
    pub span: Span,
    pub evidence: String,
    pub fix: String,
    pub related: Vec<String>,
    pub owasp: Option<String>,
    pub cwe: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verdict {
    pub status: Status,
    pub high: usize,
    pub medium: usize,
    pub low: usize,
    pub blocking: usize,
    pub duration_ms: u128,
    pub findings: Vec<Finding>,
    pub truncated: usize,
}

/// Stable 16 hex character identity for a finding. The unit separator keeps the
/// three parts from running together, so ("ab", "c") and ("a", "bc") differ.
///
/// What the id identifies is whatever the caller put in the anchor. A rule that
/// reports a line anchors on the enclosing symbol, the line's text and the
/// ordinal of that line among the identical ones inside the symbol, so the
/// caller's anchor gives every occurrence its own id. A rule that reports a
/// class anchors on the class instead: the same secret value twice in one file
/// is one id, and accepting it accepts both.
pub fn make_id(rule: &str, rel: &str, anchor: &str) -> String {
    let mut h = blake3::Hasher::new();
    h.update(rule.as_bytes());
    h.update(b"\x1f");
    h.update(rel.as_bytes());
    h.update(b"\x1f");
    h.update(anchor.trim().as_bytes());
    h.finalize().to_hex()[..16].to_string()
}

impl Verdict {
    /// Sorts by confidence (High first), then severity (High first), then rule,
    /// file, start line, start column, id.
    ///
    /// Confidence ranks *above* severity because confidence alone decides
    /// `blocking` and therefore the status: a `Low`/High-confidence finding
    /// blocks the build, so it must not fall off a later `capped(n)` behind
    /// higher-severity advisories that block nothing. Severity then orders
    /// within a confidence band, and the rest of the chain is pure tie-breaking,
    /// so the order is total and reproducible run to run.
    pub fn from_findings(mut findings: Vec<Finding>, duration_ms: u128) -> Verdict {
        findings.sort_by(|a, b| {
            a.confidence
                .cmp(&b.confidence)
                .then_with(|| a.severity.cmp(&b.severity))
                .then_with(|| a.rule.cmp(&b.rule))
                .then_with(|| a.file.cmp(&b.file))
                .then_with(|| a.span.start_line.cmp(&b.span.start_line))
                .then_with(|| a.span.start_col.cmp(&b.span.start_col))
                .then_with(|| a.id.cmp(&b.id))
        });
        let high = findings.iter().filter(|f| f.severity == Severity::High).count();
        let medium = findings.iter().filter(|f| f.severity == Severity::Medium).count();
        let low = findings.iter().filter(|f| f.severity == Severity::Low).count();
        let blocking = findings.iter().filter(|f| f.confidence == Confidence::High).count();
        let status = if blocking > 0 {
            Status::Block
        } else if !findings.is_empty() {
            Status::Advisory
        } else {
            Status::Pass
        };
        Verdict { status, high, medium, low, blocking, duration_ms, findings, truncated: 0 }
    }

    /// Keeps the `n` most important findings. The counts stay as they were, so a
    /// capped verdict still reports the true totals.
    pub fn capped(mut self, n: usize) -> Verdict {
        if self.findings.len() > n {
            self.truncated = self.findings.len() - n;
            self.findings.truncate(n);
        }
        self
    }

    pub fn exit_code(&self) -> i32 {
        match self.status {
            Status::Block => 1,
            _ => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(rule: &str, sev: Severity, conf: Confidence, line: u32) -> Finding {
        Finding {
            id: make_id(rule, "src/a.ts", "anchor"),
            rule: rule.into(),
            category: Category::Erosion,
            severity: sev,
            confidence: conf,
            file: "src/a.ts".into(),
            span: Span { start_line: line, start_col: 0, end_line: line, end_col: 5 },
            evidence: "e".into(),
            fix: "f".into(),
            related: vec![],
            owasp: None,
            cwe: None,
        }
    }

    #[test]
    fn id_is_stable_and_short() {
        let a = make_id("r", "src/a.ts", "main");
        assert_eq!(a.len(), 16);
        assert_eq!(a, make_id("r", "src/a.ts", "main"));
        assert_ne!(a, make_id("r", "src/a.ts", "other"));
        assert_ne!(a, make_id("r2", "src/a.ts", "main"));
    }

    #[test]
    fn verdict_status_follows_confidence() {
        let v = Verdict::from_findings(vec![], 1);
        assert_eq!(v.status, Status::Pass);
        assert_eq!(v.exit_code(), 0);
        let v = Verdict::from_findings(vec![f("r", Severity::Low, Confidence::Medium, 1)], 1);
        assert_eq!(v.status, Status::Advisory);
        assert_eq!(v.exit_code(), 0);
        let v = Verdict::from_findings(vec![f("r", Severity::High, Confidence::High, 1)], 1);
        assert_eq!(v.status, Status::Block);
        assert_eq!(v.exit_code(), 1);
        assert_eq!(v.blocking, 1);
    }

    #[test]
    fn capped_keeps_highest_severity_and_counts_truncated() {
        let mut fs = Vec::new();
        for i in 0..12 {
            fs.push(f("r", if i % 3 == 0 { Severity::High } else { Severity::Low }, Confidence::Medium, i));
        }
        let v = Verdict::from_findings(fs, 1).capped(10);
        assert_eq!(v.findings.len(), 10);
        assert_eq!(v.truncated, 2);
        assert_eq!(v.findings[0].severity, Severity::High);
        assert_eq!(v.high, 4); // counts reflect the full set, not the cap
    }

    #[test]
    fn capped_keeps_high_confidence_findings_first() {
        let mut fs = Vec::new();
        for i in 0..10 {
            fs.push(f("a", Severity::High, Confidence::Medium, i));
        }
        // Low severity, but High confidence: confidence alone decides blocking,
        // so these two must outrank ten High-severity advisories under the cap.
        for i in 0..2 {
            fs.push(f("z", Severity::Low, Confidence::High, i));
        }
        let v = Verdict::from_findings(fs, 1).capped(10);
        assert_eq!(v.findings.len(), 10);
        assert_eq!(v.status, Status::Block);
        assert_eq!(v.blocking, 2);
        assert_eq!(v.findings[0].confidence, Confidence::High);
        assert_eq!(v.findings[1].confidence, Confidence::High);
        assert_eq!(
            v.findings.iter().filter(|x| x.confidence == Confidence::High).count(),
            2,
            "capping must not drop the blocking findings the status is based on"
        );
    }

    #[test]
    fn declaration_order_is_sort_order() {
        assert!(Severity::High < Severity::Medium && Severity::Medium < Severity::Low);
        assert!(Confidence::High < Confidence::Medium);
    }

    #[test]
    fn ties_break_on_start_col_then_id() {
        let mut wide = f("r", Severity::High, Confidence::High, 4);
        wide.span.start_col = 9;
        let mut narrow = f("r", Severity::High, Confidence::High, 4);
        narrow.span.start_col = 2;
        let v = Verdict::from_findings(vec![wide, narrow], 1);
        assert_eq!(v.findings.iter().map(|x| x.span.start_col).collect::<Vec<_>>(), vec![2, 9]);

        let mut later = f("r", Severity::High, Confidence::High, 4);
        later.id = "ffffffffffffffff".into();
        let mut earlier = f("r", Severity::High, Confidence::High, 4);
        earlier.id = "0000000000000000".into();
        let v = Verdict::from_findings(vec![later, earlier], 1);
        assert_eq!(
            v.findings.iter().map(|x| x.id.clone()).collect::<Vec<_>>(),
            vec!["0000000000000000".to_string(), "ffffffffffffffff".to_string()]
        );
    }

    #[test]
    fn serialises_enums_lowercase() {
        let v = Verdict::from_findings(vec![f("r", Severity::Medium, Confidence::High, 3)], 7);
        let s = serde_json::to_string(&v).unwrap();
        assert!(s.contains("\"status\":\"block\""));
        assert!(s.contains("\"severity\":\"medium\""));
        assert!(s.contains("\"confidence\":\"high\""));
        assert!(s.contains("\"category\":\"erosion\""));
    }
}
