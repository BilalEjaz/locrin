//! The per-file findings cache (spec 3.2). Keyed by file content and by the
//! config, because a severity override or an exclude changes what a rule
//! produces without changing a single source byte.

use std::collections::HashMap;

use rusqlite::params;

use crate::config::Config;
use crate::finding::Finding;
use crate::index::Index;

pub fn config_hash(config: &Config) -> String {
    let mut h = blake3::Hasher::new();
    h.update(env!("CARGO_PKG_VERSION").as_bytes());
    h.update(b"\x1f");
    h.update(toml::to_string(config).unwrap_or_default().as_bytes());
    h.finalize().to_hex()[..16].to_string()
}

#[derive(Debug, Default, Clone)]
pub struct CachedFile {
    pub content_hash: String,
    pub config_hash: String,
    pub by_rule: HashMap<String, Vec<Finding>>,
}

/// Every cached row, grouped by file, in one query. A file whose rows disagree on
/// their hashes (a write that was interrupted between rules) is reported under the
/// hashes of its first row and will simply miss for the others.
pub fn load_all(ix: &Index) -> anyhow::Result<HashMap<String, CachedFile>> {
    let mut stmt = ix.conn().prepare("SELECT rel, rule, content_hash, config_hash, findings FROM findings_cache")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
        ))
    })?;
    let mut out: HashMap<String, CachedFile> = HashMap::new();
    for row in rows {
        let (rel, rule, content_hash, config_hash, json) = row?;
        let findings: Vec<Finding> = serde_json::from_str(&json)?;
        let entry = out.entry(rel).or_default();
        if entry.by_rule.is_empty() {
            entry.content_hash = content_hash;
            entry.config_hash = config_hash;
        } else if entry.content_hash != content_hash || entry.config_hash != config_hash {
            continue;
        }
        entry.by_rule.insert(rule, findings);
    }
    Ok(out)
}

pub fn put(
    ix: &mut Index,
    rel: &str,
    content_hash: &str,
    config_hash: &str,
    rule: &str,
    findings: &[Finding],
) -> anyhow::Result<()> {
    ix.conn().execute(
        "INSERT INTO findings_cache(rel, rule, content_hash, config_hash, findings) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(rel, rule) DO UPDATE SET content_hash=excluded.content_hash, config_hash=excluded.config_hash,
         findings=excluded.findings",
        params![rel, rule, content_hash, config_hash, serde_json::to_string(findings)?],
    )?;
    Ok(())
}

pub fn clear(ix: &mut Index, rel: &str) -> anyhow::Result<()> {
    ix.conn().execute("DELETE FROM findings_cache WHERE rel = ?1", params![rel])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RuleOverride;
    use crate::finding::{Category, Confidence, Severity, Span};

    fn f(rule: &str) -> Finding {
        Finding {
            id: "0123456789abcdef".into(),
            rule: rule.into(),
            category: Category::Erosion,
            severity: Severity::Low,
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
    fn config_hash_moves_with_the_config() {
        let a = config_hash(&Config::default());
        let mut c = Config::default();
        c.excludes.push("gen/**".into());
        assert_ne!(a, config_hash(&c));
        assert_eq!(a, config_hash(&Config::default()));
        assert_eq!(a.len(), 16);
    }

    /// The override that costs the most to get wrong: a severity the operator
    /// lowered changes every finding of that rule, and a cache that kept serving
    /// the old severity would ignore the config until somebody edited the file.
    /// It is also the one shape of config that carries a `None` field, so this
    /// pins that the hash is built from a serialisation that survives it.
    #[test]
    fn config_hash_moves_with_a_rule_override() {
        let a = config_hash(&Config::default());
        let mut c = Config::default();
        c.rules.insert("leftover-debug".into(), RuleOverride { enabled: None, severity: Some(Severity::Low) });
        assert_ne!(a, config_hash(&c));
        let mut off = Config::default();
        off.rules.insert("leftover-debug".into(), RuleOverride { enabled: Some(false), severity: None });
        assert_ne!(config_hash(&c), config_hash(&off));
    }

    #[test]
    fn put_load_replace_and_clear() {
        let mut ix = Index::open_in_memory().unwrap();
        put(&mut ix, "src/a.ts", "h1", "c1", "r1", &[f("r1")]).unwrap();
        put(&mut ix, "src/a.ts", "h1", "c1", "r2", &[]).unwrap();
        let all = load_all(&ix).unwrap();
        let a = &all["src/a.ts"];
        assert_eq!((a.content_hash.as_str(), a.config_hash.as_str()), ("h1", "c1"));
        assert_eq!(a.by_rule["r1"].len(), 1);
        assert!(a.by_rule["r2"].is_empty(), "an empty result is cached too: it means 'checked, clean'");

        put(&mut ix, "src/a.ts", "h2", "c1", "r1", &[]).unwrap();
        let n: i64 = ix.conn().query_row("SELECT count(*) FROM findings_cache", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 2, "put replaces the (rel, rule) row");

        clear(&mut ix, "src/a.ts").unwrap();
        assert!(load_all(&ix).unwrap().is_empty());
    }
}
