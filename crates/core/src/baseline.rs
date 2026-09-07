use std::collections::HashSet;
use std::path::Path;

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::finding::{Finding, Severity};

pub const BASELINE_FILE: &str = "locrin-baseline.json";

/// One accepted finding. `reason` and `author` are recorded so a reviewer can
/// see later why the debt was signed off and by whom.
///
/// `severity` is the severity the finding carried when it was signed off. Spec
/// 3.5 reports a baselined finding again once its severity rises, and that
/// comparison needs a recorded severity to compare against. It is optional and
/// defaulted so a baseline file written before the field existed still loads.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    pub rule: String,
    pub file: String,
    pub reason: String,
    pub author: String,
    pub date: String,
    #[serde(default)]
    pub severity: Option<Severity>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Baseline {
    pub entries: Vec<Entry>,
}

/// Today as `YYYY-MM-DD` in UTC, via Howard Hinnant's civil-from-days, so the
/// baseline file does not drag in a date library for one field.
fn today() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = (secs / 86_400) as i64;
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{:04}-{:02}-{:02}", y, m, d)
}

impl Baseline {
    /// Reads `locrin-baseline.json` from the repository root. An absent file is
    /// an empty baseline, not an error.
    pub fn load(repo_root: &Path) -> anyhow::Result<Baseline> {
        let path = repo_root.join(BASELINE_FILE);
        if !path.exists() {
            return Ok(Baseline::default());
        }
        let text = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        serde_json::from_str(&text).with_context(|| format!("invalid {}", path.display()))
    }

    /// Writes pretty JSON with the entries sorted by id, so the file is a stable
    /// diff no matter what order findings were accepted in.
    ///
    /// The write goes to a sibling temporary file and is then renamed over the
    /// target. The baseline is a committed file: a run interrupted mid-write
    /// must leave the previous baseline intact rather than a truncated one that
    /// suppresses nothing and no longer parses.
    pub fn save(&self, repo_root: &Path) -> anyhow::Result<()> {
        let mut copy = self.clone();
        copy.entries.sort_by(|a, b| a.id.cmp(&b.id));
        let path = repo_root.join(BASELINE_FILE);
        let tmp = repo_root.join(format!("{BASELINE_FILE}.tmp"));
        std::fs::write(&tmp, serde_json::to_string_pretty(&copy)?)
            .with_context(|| format!("writing {}", tmp.display()))?;
        std::fs::rename(&tmp, &path).with_context(|| format!("writing {}", path.display()))
    }

    pub fn contains(&self, id: &str) -> bool {
        self.entries.iter().any(|e| e.id == id)
    }

    /// Accepts a finding into the baseline, ignoring one that is already there.
    ///
    /// A finding id identifies a finding *class* within a symbol rather than a
    /// single occurrence (see the `make_id` doc in `finding`), so accepting one
    /// id suppresses every finding of that class inside that symbol. That is by
    /// design: signing off "this function may keep its debug logging" should not
    /// have to be repeated line by line.
    pub fn accept(&mut self, f: &Finding, reason: &str, author: &str) {
        if self.contains(&f.id) {
            return;
        }
        self.entries.push(Entry {
            id: f.id.clone(),
            rule: f.rule.clone(),
            file: f.file.clone(),
            reason: reason.to_string(),
            author: author.to_string(),
            date: today(),
            severity: Some(f.severity),
        });
    }

    /// Drops every finding whose id has been accepted.
    pub fn filter(&self, findings: Vec<Finding>) -> Vec<Finding> {
        let ids: HashSet<&str> = self.entries.iter().map(|e| e.id.as_str()).collect();
        findings.into_iter().filter(|f| !ids.contains(f.id.as_str())).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::finding::{Category, Confidence, Finding, Severity, Span};
    use std::path::{Path, PathBuf};

    /// Removes the temp directory on the way out of the test, including when an
    /// assertion panics part way through.
    struct Cleanup(PathBuf);

    impl Drop for Cleanup {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).ok();
        }
    }

    /// A fresh directory: a leftover from an earlier failed run would otherwise
    /// leave a stale baseline file behind and decide the test for us.
    fn fresh(tag: &str) -> Cleanup {
        let dir = std::env::temp_dir().join(format!("locrin-baseline-{tag}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        Cleanup(dir)
    }

    fn path(c: &Cleanup) -> &Path {
        c.0.as_path()
    }

    fn f(id: &str) -> Finding {
        Finding {
            id: id.into(),
            rule: "leftover-debug".into(),
            category: Category::Erosion,
            severity: Severity::High,
            confidence: Confidence::High,
            file: "src/a.ts".into(),
            span: Span { start_line: 1, start_col: 0, end_line: 1, end_col: 1 },
            evidence: "e".into(),
            fix: "f".into(),
            related: vec![],
            owasp: None,
            cwe: None,
        }
    }

    #[test]
    fn round_trip_and_filter() {
        let dir = fresh("baseline");
        let mut b = Baseline::load(path(&dir)).unwrap();
        assert!(b.entries.is_empty());
        b.accept(&f("b"), "legacy script", "tester");
        b.accept(&f("a"), "generated", "tester");
        b.save(path(&dir)).unwrap();
        let b2 = Baseline::load(path(&dir)).unwrap();
        assert_eq!(b2.entries.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), vec!["a", "b"]);
        assert!(b2.contains("a"));
        assert_eq!(b2.entries[0].date.len(), 10);
        let kept = b2.filter(vec![f("a"), f("c")]);
        assert_eq!(kept.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), vec!["c"]);
    }

    /// Spec 3.5 reports a baselined finding again once its severity rises, which
    /// is only decidable if the entry records the severity it was accepted at.
    #[test]
    fn accept_records_the_severity_it_was_signed_off_at() {
        let dir = fresh("baseline-severity");
        let mut b = Baseline::default();
        b.accept(&f("a"), "legacy script", "tester");
        b.save(path(&dir)).unwrap();
        let b2 = Baseline::load(path(&dir)).unwrap();
        assert_eq!(b2.entries[0].severity, Some(Severity::High));
    }

    /// A baseline written before the severity field existed must still load: the
    /// file is committed, and an older entry is not a corrupt entry.
    #[test]
    fn an_entry_without_a_severity_still_loads() {
        let dir = fresh("baseline-legacy");
        std::fs::write(
            path(&dir).join(BASELINE_FILE),
            r#"{"entries":[{"id":"a","rule":"leftover-debug","file":"src/a.ts","reason":"legacy","author":"t","date":"2026-09-05"}]}"#,
        )
        .unwrap();
        let b = Baseline::load(path(&dir)).unwrap();
        assert_eq!(b.entries.len(), 1);
        assert_eq!(b.entries[0].severity, None);
        assert!(b.contains("a"));
    }
}
