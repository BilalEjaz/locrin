use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use rusqlite::{params, Connection, OptionalExtension};

pub const SCHEMA_VERSION: &str = "1";

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS files (
  rel TEXT PRIMARY KEY,
  language TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  parse_status TEXT NOT NULL,
  indexed_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS symbols (
  id INTEGER PRIMARY KEY,
  rel TEXT NOT NULL,
  kind TEXT NOT NULL,
  name TEXT NOT NULL,
  start_line INTEGER NOT NULL,
  start_col INTEGER NOT NULL,
  end_line INTEGER NOT NULL,
  end_col INTEGER NOT NULL,
  exported INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS symbols_rel ON symbols(rel);
CREATE INDEX IF NOT EXISTS symbols_name ON symbols(name);
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
"#;

pub fn content_hash(source: &str) -> String {
    blake3::hash(source.as_bytes()).to_hex().to_string()
}

/// Where the index database for `repo_root` lives.
///
/// The cache sits outside the repository so a scan never dirties the working
/// tree, and it is keyed by a hash of the canonical root so two checkouts of the
/// same project do not share one database.
pub fn cache_path(repo_root: &Path) -> PathBuf {
    let canonical = std::fs::canonicalize(repo_root).unwrap_or_else(|_| repo_root.to_path_buf());
    let key = blake3::hash(canonical.to_string_lossy().as_bytes()).to_hex();
    let base = std::env::var_os("LOCRIN_CACHE_DIR")
        .map(PathBuf::from)
        .or_else(|| dirs::cache_dir().map(|d| d.join("locrin")))
        .unwrap_or_else(|| repo_root.join(".locrin-cache"));
    base.join(&key[..16]).join("index.db")
}

pub struct Index {
    conn: Connection,
}

impl Index {
    pub fn open(repo_root: &Path) -> anyhow::Result<Index> {
        let path = cache_path(repo_root);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        let conn = Connection::open(&path).with_context(|| format!("opening {}", path.display()))?;
        let mut ix = Index { conn };
        if !ix.schema_matches()? {
            drop(ix);
            let _ = std::fs::remove_file(&path);
            let conn = Connection::open(&path).with_context(|| format!("opening {}", path.display()))?;
            ix = Index { conn };
        }
        ix.init()?;
        Ok(ix)
    }

    pub fn open_in_memory() -> anyhow::Result<Index> {
        let mut ix = Index { conn: Connection::open_in_memory()? };
        ix.init()?;
        Ok(ix)
    }

    fn schema_matches(&mut self) -> anyhow::Result<bool> {
        let has_meta: bool = self
            .conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='meta'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .map(|n| n > 0)?;
        if !has_meta {
            return Ok(true); // fresh database, init() will stamp it
        }
        let v: Option<String> = self
            .conn
            .query_row("SELECT value FROM meta WHERE key='schema_version'", [], |r| r.get(0))
            .optional()?;
        Ok(v.as_deref() == Some(SCHEMA_VERSION))
    }

    fn init(&mut self) -> anyhow::Result<()> {
        self.conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
        self.conn.execute_batch(SCHEMA)?;
        self.conn.execute(
            "INSERT OR REPLACE INTO meta(key, value) VALUES ('schema_version', ?1)",
            params![SCHEMA_VERSION],
        )?;
        Ok(())
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    pub fn upsert_file(
        &mut self,
        rel: &str,
        language: &str,
        content_hash: &str,
        parse_status: &str,
    ) -> anyhow::Result<()> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
        self.conn.execute(
            "INSERT INTO files(rel, language, content_hash, parse_status, indexed_at) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(rel) DO UPDATE SET language=excluded.language, content_hash=excluded.content_hash,
             parse_status=excluded.parse_status, indexed_at=excluded.indexed_at",
            params![rel, language, content_hash, parse_status, now],
        )?;
        Ok(())
    }

    pub fn file_hash(&self, rel: &str) -> anyhow::Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT content_hash FROM files WHERE rel = ?1", params![rel], |r| r.get(0))
            .optional()?)
    }

    pub fn changed(&self, rel: &str, content_hash: &str) -> anyhow::Result<bool> {
        Ok(self.file_hash(rel)?.as_deref() != Some(content_hash))
    }

    pub fn remove_missing(&mut self, present: &[String]) -> anyhow::Result<usize> {
        let mut stmt = self.conn.prepare("SELECT rel FROM files")?;
        let existing: Vec<String> = stmt.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
        drop(stmt);
        let keep: std::collections::HashSet<&str> = present.iter().map(|s| s.as_str()).collect();
        let mut removed = 0;
        let tx = self.conn.transaction()?;
        for rel in existing.iter().filter(|r| !keep.contains(r.as_str())) {
            tx.execute("DELETE FROM symbols WHERE rel = ?1", params![rel])?;
            tx.execute("DELETE FROM files WHERE rel = ?1", params![rel])?;
            removed += 1;
        }
        tx.commit()?;
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_stable_and_hex() {
        let h = content_hash("abc");
        assert_eq!(h.len(), 64);
        assert_eq!(h, content_hash("abc"));
        assert_ne!(h, content_hash("abd"));
    }

    #[test]
    fn upsert_and_change_detection() {
        let mut ix = Index::open_in_memory().unwrap();
        assert!(ix.changed("src/a.ts", "h1").unwrap());
        ix.upsert_file("src/a.ts", "typescript", "h1", "ok").unwrap();
        assert!(!ix.changed("src/a.ts", "h1").unwrap());
        assert!(ix.changed("src/a.ts", "h2").unwrap());
        assert_eq!(ix.file_hash("src/a.ts").unwrap().as_deref(), Some("h1"));
        ix.upsert_file("src/a.ts", "typescript", "h2", "ok").unwrap();
        assert_eq!(ix.file_hash("src/a.ts").unwrap().as_deref(), Some("h2"));
    }

    #[test]
    fn remove_missing_drops_files_not_present() {
        let mut ix = Index::open_in_memory().unwrap();
        ix.upsert_file("a.ts", "typescript", "1", "ok").unwrap();
        ix.upsert_file("b.ts", "typescript", "1", "ok").unwrap();
        let removed = ix.remove_missing(&["a.ts".to_string()]).unwrap();
        assert_eq!(removed, 1);
        assert!(ix.file_hash("b.ts").unwrap().is_none());
        assert!(ix.file_hash("a.ts").unwrap().is_some());
    }

    #[test]
    fn cache_path_is_outside_repo_and_keyed_by_root() {
        std::env::remove_var("LOCRIN_CACHE_DIR");
        let p = cache_path(std::path::Path::new("C:/repo/one"));
        let q = cache_path(std::path::Path::new("C:/repo/two"));
        assert_ne!(p, q);
        assert!(p.ends_with("index.db"));
    }
}
