use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use rusqlite::{params, Connection, OptionalExtension};

pub const SCHEMA_VERSION: &str = "2";

/// How long a statement waits for another process holding the same index before
/// it gives up.
const BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

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
  export_name TEXT,
  start_line INTEGER NOT NULL,
  start_col INTEGER NOT NULL,
  end_line INTEGER NOT NULL,
  end_col INTEGER NOT NULL,
  exported INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS symbols_rel ON symbols(rel);
CREATE INDEX IF NOT EXISTS symbols_name ON symbols(name);
CREATE INDEX IF NOT EXISTS symbols_export ON symbols(export_name);
CREATE TABLE IF NOT EXISTS edges (
  id INTEGER PRIMARY KEY,
  from_rel TEXT NOT NULL,
  to_rel TEXT,
  specifier TEXT NOT NULL,
  name TEXT NOT NULL,
  kind TEXT NOT NULL,
  resolution TEXT NOT NULL,
  line INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS edges_from ON edges(from_rel);
CREATE INDEX IF NOT EXISTS edges_to ON edges(to_rel);
CREATE TABLE IF NOT EXISTS allow_lines (
  rel TEXT NOT NULL,
  line INTEGER NOT NULL,
  PRIMARY KEY (rel, line)
);
CREATE TABLE IF NOT EXISTS findings_cache (
  rel TEXT NOT NULL,
  rule TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  config_hash TEXT NOT NULL,
  findings TEXT NOT NULL,
  PRIMARY KEY (rel, rule)
);
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
"#;

/// Every table that holds rows keyed by a file's repo-relative path. `remove_missing`
/// walks this list so a file that leaves the repository leaves every table.
const PER_FILE_TABLES: &[(&str, &str)] =
    &[("symbols", "rel"), ("edges", "from_rel"), ("allow_lines", "rel"), ("findings_cache", "rel"), ("files", "rel")];

pub fn content_hash(source: &str) -> String {
    blake3::hash(source.as_bytes()).to_hex().to_string()
}

/// Where the index database for `repo_root` lives.
///
/// The cache sits outside the repository so a scan never dirties the working
/// tree, and it is keyed by a hash of the canonical root so two checkouts of the
/// same project do not share one database.
///
/// `LOCRIN_CACHE_DIR` overrides the base directory. Set it to give a run its own
/// cache: CI jobs that must not share state, a sandbox, or a test. Unset, the
/// base is the platform cache directory (`locrin` inside it), falling back to
/// `.locrin-cache` in the repository when the platform has no cache directory.
pub fn cache_path(repo_root: &Path) -> PathBuf {
    let canonical = std::fs::canonicalize(repo_root).unwrap_or_else(|_| repo_root.to_path_buf());
    let key = blake3::hash(canonical.to_string_lossy().as_bytes()).to_hex();
    let base = std::env::var_os("LOCRIN_CACHE_DIR")
        .map(PathBuf::from)
        .or_else(|| dirs::cache_dir().map(|d| d.join("locrin")))
        .unwrap_or_else(|| repo_root.join(".locrin-cache"));
    base.join(&key[..16]).join("index.db")
}

/// A sibling of the database file, such as the write-ahead log.
fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

/// Delete a stale database and its write-ahead log siblings.
///
/// A failure here must surface: silently keeping a mismatched database and
/// stamping it with the current schema version would hide the mismatch forever.
fn remove_database_files(path: &Path) -> anyhow::Result<()> {
    for target in [path.to_path_buf(), sibling(path, "-wal"), sibling(path, "-shm")] {
        match std::fs::remove_file(&target) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(anyhow::Error::new(e))
                    .with_context(|| format!("removing stale index file {}", target.display()));
            }
        }
    }
    Ok(())
}

/// Whether a failure to read the schema means the database is unusable and has
/// to be thrown away.
///
/// Only the two codes that say the bytes on disk are not a readable database
/// qualify. A busy or locked database is another process holding the same cache,
/// and an I/O error is the disk; deleting a shared index over either would turn a
/// transient failure into data loss for every run that shares it.
fn should_rebuild(err: &rusqlite::Error) -> bool {
    matches!(
        err,
        rusqlite::Error::SqliteFailure(e, _)
            if matches!(e.code, rusqlite::ErrorCode::NotADatabase | rusqlite::ErrorCode::DatabaseCorrupt)
    )
}

/// Open the index database with the busy timeout already in place.
///
/// The timeout has to be set before the first read, not in `init`: the schema
/// check is itself a read, and without a timeout a concurrent writer turns it
/// into an immediate `SQLITE_BUSY`.
fn open_with_timeout(path: &Path) -> anyhow::Result<Connection> {
    let conn = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
    conn.busy_timeout(BUSY_TIMEOUT).with_context(|| format!("setting the busy timeout on {}", path.display()))?;
    Ok(conn)
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
        let conn = open_with_timeout(&path)?;
        let mut ix = Index { conn };
        // A file that is not a database at all fails the same way a stale schema
        // does: it is a cache the engine cannot read, and spec 9 says rebuild it
        // and log once rather than fail the run over a disposable file. Anything
        // else the read can fail with is a live problem, not a broken cache, and
        // must propagate: the database is only ever deleted for the two codes
        // `should_rebuild` names.
        let rebuild = match ix.schema_matches() {
            Ok(true) => false,
            Ok(false) => {
                eprintln!("warning: rebuilding index at {} (schema changed)", path.display());
                true
            }
            Err(e) if should_rebuild(&e) => {
                eprintln!("warning: rebuilding corrupt index at {}", path.display());
                true
            }
            Err(e) => {
                return Err(anyhow::Error::new(e))
                    .with_context(|| format!("reading the index schema at {}", path.display()));
            }
        };
        if rebuild {
            drop(ix);
            remove_database_files(&path)?;
            ix = Index { conn: open_with_timeout(&path)? };
        }
        ix.init()?;
        Ok(ix)
    }

    pub fn open_in_memory() -> anyhow::Result<Index> {
        let mut ix = Index { conn: Connection::open_in_memory()? };
        ix.init()?;
        Ok(ix)
    }

    fn schema_matches(&mut self) -> rusqlite::Result<bool> {
        let has_meta: bool = self
            .conn
            .query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name='meta'", [], |r| {
                r.get::<_, i64>(0)
            })
            .map(|n| n > 0)?;
        if !has_meta {
            return Ok(true); // fresh database, init() will stamp it
        }
        let v: Option<String> =
            self.conn.query_row("SELECT value FROM meta WHERE key='schema_version'", [], |r| r.get(0)).optional()?;
        Ok(v.as_deref() == Some(SCHEMA_VERSION))
    }

    fn init(&mut self) -> anyhow::Result<()> {
        // Two runs against one repository (an editor hook and a terminal, say)
        // share a database. Waiting briefly for the other writer is the right
        // answer; failing the check with "database is locked" is not. `open` has
        // already set this on the connection it hands over; the call is repeated
        // here for the in-memory connection, which never goes through `open`.
        self.conn.busy_timeout(BUSY_TIMEOUT)?;
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

    pub fn parse_status(&self, rel: &str) -> anyhow::Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT parse_status FROM files WHERE rel = ?1", params![rel], |r| r.get(0))
            .optional()?)
    }

    /// Every indexed file, sorted, so callers iterate in a reproducible order.
    pub fn all_files(&self) -> anyhow::Result<Vec<String>> {
        let mut stmt = self.conn.prepare("SELECT rel FROM files ORDER BY rel")?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Records which lines of `rel` carry the allow marker, replacing whatever was
    /// stored before. The rule runner consults this for findings on files it did not
    /// parse this run, so suppression works for graph rules too.
    pub fn replace_allow_lines(&mut self, rel: &str, lines: &[u32]) -> anyhow::Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM allow_lines WHERE rel = ?1", params![rel])?;
        {
            let mut stmt = tx.prepare("INSERT INTO allow_lines(rel, line) VALUES (?1, ?2)")?;
            for line in lines {
                stmt.execute(params![rel, line])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn is_allowed(&self, rel: &str, line: u32) -> anyhow::Result<bool> {
        let n: i64 = self.conn.query_row(
            "SELECT count(*) FROM allow_lines WHERE rel = ?1 AND line = ?2",
            params![rel, line],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }

    /// Every file that still has at least one unresolved import edge, sorted.
    ///
    /// `remove_missing` marks edges into a departed file unresolved without
    /// touching the importer's own row, so when that file comes back the
    /// importer is unchanged and would never be re-indexed. This is how a run
    /// finds the importers whose edges are worth attempting again.
    pub fn files_with_unresolved_edges(&self) -> anyhow::Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT DISTINCT from_rel FROM edges WHERE resolution = 'unresolved' ORDER BY from_rel")?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn remove_missing(&mut self, present: &[String]) -> anyhow::Result<usize> {
        let mut stmt = self.conn.prepare("SELECT rel FROM files")?;
        let existing: Vec<String> = stmt.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
        drop(stmt);
        let keep: std::collections::HashSet<&str> = present.iter().map(|s| s.as_str()).collect();
        let mut removed = 0;
        let tx = self.conn.transaction()?;
        for rel in existing.iter().filter(|r| !keep.contains(r.as_str())) {
            for (table, column) in PER_FILE_TABLES {
                tx.execute(&format!("DELETE FROM {table} WHERE {column} = ?1"), params![rel])?;
            }
            // Edges from files that stayed still point here. The target is gone, so
            // the edge is no longer resolved: it keeps its specifier and says so.
            tx.execute("UPDATE edges SET to_rel = NULL, resolution = 'unresolved' WHERE to_rel = ?1", params![rel])?;
            removed += 1;
        }
        tx.commit()?;
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sqlite_failure(code: rusqlite::ErrorCode, extended_code: i32) -> rusqlite::Error {
        rusqlite::Error::SqliteFailure(rusqlite::ffi::Error { code, extended_code }, None)
    }

    #[test]
    fn only_corruption_rebuilds_the_database() {
        assert!(
            !should_rebuild(&sqlite_failure(rusqlite::ErrorCode::DatabaseBusy, 5)),
            "a busy database is another process, not corruption: rebuilding would destroy a shared index"
        );
        assert!(should_rebuild(&sqlite_failure(rusqlite::ErrorCode::NotADatabase, 26)));
        assert!(should_rebuild(&sqlite_failure(rusqlite::ErrorCode::DatabaseCorrupt, 11)));
    }

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
    fn allow_lines_round_trip_and_replace() {
        let mut ix = Index::open_in_memory().unwrap();
        ix.replace_allow_lines("src/a.ts", &[3, 9]).unwrap();
        assert!(ix.is_allowed("src/a.ts", 3).unwrap());
        assert!(!ix.is_allowed("src/a.ts", 4).unwrap());
        assert!(!ix.is_allowed("src/b.ts", 3).unwrap());
        ix.replace_allow_lines("src/a.ts", &[4]).unwrap();
        assert!(!ix.is_allowed("src/a.ts", 3).unwrap(), "replace must drop the old lines");
        assert!(ix.is_allowed("src/a.ts", 4).unwrap());
    }

    #[test]
    fn parse_status_and_file_list() {
        let mut ix = Index::open_in_memory().unwrap();
        assert_eq!(ix.parse_status("src/a.ts").unwrap(), None);
        ix.upsert_file("src/b.ts", "typescript", "h", "ok").unwrap();
        ix.upsert_file("src/a.ts", "typescript", "h", "error").unwrap();
        assert_eq!(ix.parse_status("src/a.ts").unwrap().as_deref(), Some("error"));
        assert_eq!(ix.all_files().unwrap(), vec!["src/a.ts".to_string(), "src/b.ts".to_string()]);
    }

    /// A file that left the repository must leave every table, or a graph rule
    /// would keep seeing edges from a file that no longer exists.
    #[test]
    fn remove_missing_cascades_to_every_table() {
        let mut ix = Index::open_in_memory().unwrap();
        ix.upsert_file("a.ts", "typescript", "1", "ok").unwrap();
        ix.replace_allow_lines("a.ts", &[1]).unwrap();
        let c = ix.conn();
        c.execute(
            "INSERT INTO symbols(rel, kind, name, start_line, start_col, end_line, end_col, exported)
             VALUES ('a.ts','function','f',1,0,1,1,0)",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO edges(from_rel, to_rel, specifier, name, kind, resolution, line)
             VALUES ('a.ts','b.ts','./b','x','import','resolved',1)",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO findings_cache(rel, rule, content_hash, config_hash, findings) VALUES ('a.ts','r','1','c','[]')",
            [],
        )
        .unwrap();
        assert_eq!(ix.remove_missing(&[]).unwrap(), 1);
        for table in ["files", "symbols", "edges", "allow_lines", "findings_cache"] {
            let n: i64 = ix.conn().query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0)).unwrap();
            assert_eq!(n, 0, "{table} still has rows for a removed file");
        }
    }

    /// The other half of the cascade: an edge from a file that stayed into a file
    /// that left must stop claiming it resolved, or a graph rule would follow it
    /// to a node that is no longer in the index.
    #[test]
    fn remove_missing_unresolves_edges_into_a_departed_file() {
        let mut ix = Index::open_in_memory().unwrap();
        ix.upsert_file("a.ts", "typescript", "1", "ok").unwrap();
        ix.upsert_file("b.ts", "typescript", "1", "ok").unwrap();
        ix.conn()
            .execute(
                "INSERT INTO edges(from_rel, to_rel, specifier, name, kind, resolution, line)
                 VALUES ('a.ts','b.ts','./b','x','import','resolved',1)",
                [],
            )
            .unwrap();
        assert_eq!(ix.remove_missing(&["a.ts".to_string()]).unwrap(), 1);
        let (to_rel, resolution): (Option<String>, String) = ix
            .conn()
            .query_row("SELECT to_rel, resolution FROM edges WHERE from_rel = 'a.ts'", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(to_rel, None, "the edge must stop pointing at a file that left the repository");
        assert_eq!(resolution, "unresolved");
    }

    #[test]
    fn files_with_unresolved_edges_lists_each_importer_once() {
        let ix = Index::open_in_memory().unwrap();
        ix.conn()
            .execute_batch(
                "INSERT INTO edges(from_rel, to_rel, specifier, name, kind, resolution, line)
                 VALUES ('b.ts',NULL,'./gone','x','import','unresolved',1),
                        ('a.ts',NULL,'./gone','x','import','unresolved',1),
                        ('a.ts',NULL,'./gone','y','import','unresolved',1),
                        ('a.ts','b.ts','./b','z','import','resolved',2),
                        ('c.ts',NULL,'react','d','import','external',1)",
            )
            .unwrap();
        assert_eq!(ix.files_with_unresolved_edges().unwrap(), vec!["a.ts".to_string(), "b.ts".to_string()]);
    }

    #[test]
    fn cache_path_is_outside_repo_and_keyed_by_root() {
        let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let p = cache_path(std::path::Path::new("C:/repo/one"));
        let q = cache_path(std::path::Path::new("C:/repo/two"));
        assert_ne!(p, q);
        assert!(p.ends_with("index.db"));
        assert!(!p.starts_with("C:/repo/one"), "the cache must not live inside the repository: {p:?}");
    }

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn unique_cache_dir(tag: &str) -> PathBuf {
        static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        std::env::temp_dir().join(format!("locrin-test-{tag}-{}-{nanos}-{n}", std::process::id()))
    }

    #[test]
    fn schema_mismatch_rebuilds_the_database() {
        let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = unique_cache_dir("mismatch");
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("LOCRIN_CACHE_DIR", &dir);
        let repo = Path::new("C:/repo/mismatch");

        let mut ix = Index::open(repo).unwrap();
        ix.upsert_file("marker.ts", "typescript", "h1", "ok").unwrap();
        ix.conn().execute("UPDATE meta SET value = '0' WHERE key = 'schema_version'", []).unwrap();
        drop(ix);

        let ix = Index::open(repo).unwrap();
        let version: String =
            ix.conn().query_row("SELECT value FROM meta WHERE key = 'schema_version'", [], |r| r.get(0)).unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        assert!(
            ix.file_hash("marker.ts").unwrap().is_none(),
            "a schema mismatch must rebuild the database, not restamp the old one"
        );
        drop(ix);

        std::env::remove_var("LOCRIN_CACHE_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_corrupt_database_is_rebuilt() {
        let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = unique_cache_dir("corrupt");
        let repo = Path::new("C:/repo/corrupt");
        std::env::set_var("LOCRIN_CACHE_DIR", &dir);
        let path = cache_path(repo);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "this is not a database, it is a text file\n").unwrap();

        let ix = Index::open(repo).expect("a corrupt index must be rebuilt, not reported as an error");
        let version: String =
            ix.conn().query_row("SELECT value FROM meta WHERE key = 'schema_version'", [], |r| r.get(0)).unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        drop(ix);

        std::env::remove_var("LOCRIN_CACHE_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn schema_mismatch_reports_a_rebuild_that_cannot_delete() {
        let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = unique_cache_dir("locked");
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("LOCRIN_CACHE_DIR", &dir);
        let repo = Path::new("C:/repo/locked");
        let path = cache_path(repo);

        let mut ix = Index::open(repo).unwrap();
        ix.upsert_file("marker.ts", "typescript", "h1", "ok").unwrap();
        ix.conn().execute("UPDATE meta SET value = '0' WHERE key = 'schema_version'", []).unwrap();
        drop(ix);

        // Stand in for a second process holding the database: Windows refuses the
        // delete while SQLite has the file open.
        let holder = Connection::open(&path).unwrap();
        let err = match Index::open(repo) {
            Ok(_) => panic!("open must fail while the stale database cannot be removed"),
            Err(e) => e,
        };
        let text = format!("{err:#}");
        assert!(text.contains("index.db"), "the error must name the file it could not remove: {text}");
        drop(holder);

        std::env::remove_var("LOCRIN_CACHE_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
