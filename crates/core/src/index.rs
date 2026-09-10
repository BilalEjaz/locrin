use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use rusqlite::{params, Connection, OptionalExtension};

pub const SCHEMA_VERSION: &str = "5";

/// How long a statement waits for another process holding the same index before
/// it gives up.
const BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS files (
  rel TEXT PRIMARY KEY,
  language TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  parse_status TEXT NOT NULL,
  indexed_at INTEGER NOT NULL,
  size INTEGER NOT NULL DEFAULT 0,
  mtime INTEGER NOT NULL DEFAULT 0
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
  exported INTEGER NOT NULL,
  -- How many parameters the symbol declares, NULL when it is not callable.
  -- This is the signature the symbol search ranks on, so a class or a plain
  -- const must be absent from that comparison rather than count as zero.
  params INTEGER
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
CREATE TABLE IF NOT EXISTS skipped_tests (
  rel TEXT NOT NULL,
  name TEXT NOT NULL,
  PRIMARY KEY (rel, name)
);
CREATE TABLE IF NOT EXISTS osv_batch (
  lock_hash TEXT PRIMARY KEY,
  fetched_at INTEGER NOT NULL,
  json TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS osv_vulns (
  id TEXT PRIMARY KEY,
  fetched_at INTEGER NOT NULL,
  json TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
"#;

/// Every table that holds rows keyed by a file's repo-relative path. `remove_missing`
/// walks this list so a file that leaves the repository leaves every table.
///
/// The two OSV tables are not here: they are keyed by a lockfile hash and by an
/// advisory id, and they are a snapshot of what a registry said, not a statement
/// about any one file in this repository.
const PER_FILE_TABLES: &[(&str, &str)] = &[
    ("symbols", "rel"),
    ("edges", "from_rel"),
    ("allow_lines", "rel"),
    ("skipped_tests", "rel"),
    ("findings_cache", "rel"),
    ("files", "rel"),
];

pub fn content_hash(source: &str) -> String {
    blake3::hash(source.as_bytes()).to_hex().to_string()
}

/// The size and modification time a stat-based change check compares against.
///
/// The modification time is in nanoseconds, as fine as the filesystem records.
/// Seconds would be too coarse to be safe: an editor hook that saves twice
/// inside one second, ending at the same byte length, would leave the second
/// save looking unchanged and its findings stale until the file is edited again.
///
/// Zero for either value means the filesystem did not answer, which
/// [`Index::unchanged_by_stat`] treats as "no answer" rather than as a match, so
/// a platform that cannot supply one simply never takes the shortcut.
pub fn file_stat(path: &Path) -> (i64, i64) {
    let Ok(meta) = std::fs::metadata(path) else { return (0, 0) };
    let mtime =
        meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_nanos() as i64).unwrap_or(0);
    (meta.len() as i64, mtime)
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
        Index::open_at(&cache_path(repo_root))
    }

    /// Opens the index database at an exact path, rebuilding it when its schema
    /// is not this build's. [`Index::open`] derives that path from a repository
    /// root; callers that already know where the database lives use this.
    pub fn open_at(path: &Path) -> anyhow::Result<Index> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        let conn = open_with_timeout(path)?;
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
            remove_database_files(path)?;
            ix = Index { conn: open_with_timeout(path)? };
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

    /// Runs `body` as one atomic unit, releasing the savepoint on success and
    /// rolling back to it on failure.
    ///
    /// A savepoint outside any transaction behaves like `BEGIN DEFERRED`, so a
    /// caller that opens no transaction of its own still gets one commit per
    /// call. Inside the run-wide transaction [`Index::begin`] opens it nests
    /// instead, which is what lets a whole run share a single commit.
    ///
    /// `name` is a SQL identifier and is only ever a literal from this crate.
    ///
    /// A failed rollback is not reported: the caller's error is the one that
    /// explains what went wrong, and replacing it with the cleanup's error would
    /// hide the cause.
    pub(crate) fn savepoint<T>(
        &self,
        name: &str,
        body: impl FnOnce(&Connection) -> anyhow::Result<T>,
    ) -> anyhow::Result<T> {
        self.conn.execute_batch(&format!("SAVEPOINT {name}"))?;
        match body(&self.conn) {
            Ok(value) => {
                self.conn.execute_batch(&format!("RELEASE {name}"))?;
                Ok(value)
            }
            Err(e) => {
                let _ = self.conn.execute_batch(&format!("ROLLBACK TO {name}; RELEASE {name}"));
                Err(e)
            }
        }
    }

    /// Opens a transaction meant to span a whole run.
    ///
    /// Every write the index does is a savepoint, so without this each one is
    /// its own commit: several thousand fsync-shaped units for a repository the
    /// size of a real app. Wrapping the run turns those into one. The caller
    /// owns the pair: call [`Index::commit`] once the run's writes are done, and
    /// drop the index instead if anything failed, which rolls the run back.
    ///
    /// The transaction is `IMMEDIATE`, so the write lock is taken here rather
    /// than at the run's first write. That is what makes a second locrin against
    /// the same cache wait (up to the busy timeout) instead of failing: in WAL
    /// mode a deferred transaction that has already read and then tries to write
    /// after another connection committed is refused with a snapshot conflict
    /// straight away, and the busy handler is never consulted for it. Taking the
    /// lock up front sends the second run through the busy handler, where
    /// waiting is what it is for. Readers are unaffected either way: the
    /// database is in WAL mode. The lock is then held for the whole run.
    pub fn begin(&mut self) -> anyhow::Result<()> {
        self.conn.execute_batch("BEGIN IMMEDIATE")?;
        Ok(())
    }

    /// Commits the transaction [`Index::begin`] opened.
    pub fn commit(&mut self) -> anyhow::Result<()> {
        self.conn.execute_batch("COMMIT")?;
        Ok(())
    }

    /// Records the file row, without any stat for a later run to compare against.
    ///
    /// A row written this way is never matched by [`Index::unchanged_by_stat`],
    /// so a run that reads it falls back to the content hash. Callers that hold
    /// the file's size and modification time use [`Index::upsert_file_stat`].
    pub fn upsert_file(
        &mut self,
        rel: &str,
        language: &str,
        content_hash: &str,
        parse_status: &str,
    ) -> anyhow::Result<()> {
        self.upsert_file_stat(rel, language, content_hash, parse_status, 0, 0)
    }

    /// Records the file row together with the size and modification time that
    /// [`Index::unchanged_by_stat`] compares against on a later run. Pass zero
    /// for either when the filesystem could not answer; a zero is "no answer",
    /// not a value that can match.
    pub fn upsert_file_stat(
        &mut self,
        rel: &str,
        language: &str,
        content_hash: &str,
        parse_status: &str,
        size: i64,
        mtime: i64,
    ) -> anyhow::Result<()> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
        self.conn.execute(
            "INSERT INTO files(rel, language, content_hash, parse_status, indexed_at, size, mtime)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(rel) DO UPDATE SET language=excluded.language, content_hash=excluded.content_hash,
             parse_status=excluded.parse_status, indexed_at=excluded.indexed_at, size=excluded.size,
             mtime=excluded.mtime",
            params![rel, language, content_hash, parse_status, now, size, mtime],
        )?;
        Ok(())
    }

    /// Brings the stored size and modification time for `rel` up to date without
    /// saying anything about its content.
    ///
    /// This is for the file that was touched but not edited: the stat disagreed,
    /// the run read and hashed the file, and the hash said unchanged. Nothing is
    /// re-recorded for such a file, so without this the stale stat would stay and
    /// the file would pay a read and a hash on every run from now on. A checkout,
    /// a formatter or a stash pop rewrites hundreds of unchanged files at once,
    /// so that adds up.
    ///
    /// The stat passed in must be the one taken before the read, the same rule
    /// [`indexer::record_with_stat`](crate::indexer::record_with_stat) explains.
    /// A stat that could not answer is not stored, and a file the index has never
    /// seen is not invented: only an existing row is updated, and only when the
    /// values actually differ.
    pub fn refresh_stat(&mut self, rel: &str, size: i64, mtime: i64) -> anyhow::Result<()> {
        if size == 0 || mtime == 0 {
            return Ok(());
        }
        // Prepared once and reused: a full run calls this for every file whose
        // stat moved without its bytes moving, which after a checkout is most of
        // the repository.
        self.conn
            .prepare_cached("UPDATE files SET size = ?1, mtime = ?2 WHERE rel = ?3 AND (size <> ?1 OR mtime <> ?2)")?
            .execute(params![size, mtime, rel])?;
        Ok(())
    }

    /// Whether `rel` is certainly the file this index already recorded, judged
    /// by size and modification time alone.
    ///
    /// This is a shortcut past reading and hashing a file, so it may only ever
    /// be wrong in the direction of more work: false means "read it and decide
    /// properly". It answers true only when a row exists and both values match
    /// and neither is zero, because zero is what an unavailable stat records and
    /// two unavailable stats must not compare equal.
    pub fn unchanged_by_stat(&self, rel: &str, size: i64, mtime: i64) -> anyhow::Result<bool> {
        if size == 0 || mtime == 0 {
            return Ok(false);
        }
        let stored: Option<(i64, i64)> = self
            .conn
            .prepare_cached("SELECT size, mtime FROM files WHERE rel = ?1")?
            .query_row(params![rel], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?;
        Ok(stored == Some((size, mtime)))
    }

    pub fn file_hash(&self, rel: &str) -> anyhow::Result<Option<String>> {
        Ok(self
            .conn
            .prepare_cached("SELECT content_hash FROM files WHERE rel = ?1")?
            .query_row(params![rel], |r| r.get(0))
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

    /// Reads one row of the key-value side table the schema version lives in.
    ///
    /// The table is the index's own scratch space, so anything stored here is as
    /// disposable as the index: a rebuild takes it with everything else, and a
    /// caller has to be able to answer without it.
    pub fn meta_get(&self, key: &str) -> anyhow::Result<Option<String>> {
        Ok(self
            .conn
            .prepare_cached("SELECT value FROM meta WHERE key = ?1")?
            .query_row(params![key], |r| r.get(0))
            .optional()?)
    }

    /// Writes one row of that table, replacing whatever the key held.
    pub fn meta_set(&mut self, key: &str, value: &str) -> anyhow::Result<()> {
        self.conn
            .prepare_cached("INSERT OR REPLACE INTO meta(key, value) VALUES (?1, ?2)")?
            .execute(params![key, value])?;
        Ok(())
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
        self.savepoint("allow_lines", |tx| {
            tx.execute("DELETE FROM allow_lines WHERE rel = ?1", params![rel])?;
            let mut stmt = tx.prepare("INSERT INTO allow_lines(rel, line) VALUES (?1, ?2)")?;
            for line in lines {
                stmt.execute(params![rel, line])?;
            }
            Ok(())
        })
    }

    /// Records which test cases in `rel` the runner will skip, replacing whatever
    /// was stored before. This is the memory that lets a later run tell a test
    /// skipped in this change from one that was already skipped: the run reads
    /// the stored set before it re-records the file, and what it reads is what
    /// the previous version of that file said.
    ///
    /// Names are a set, so two cases sharing a name store one row. A test file
    /// with no skipped case stores nothing, which is not the same as a file the
    /// index has never seen: the caller learns which of the two it has from the
    /// `files` row, not from this table.
    pub fn replace_skipped_tests(&mut self, rel: &str, names: &[String]) -> anyhow::Result<()> {
        self.savepoint("skipped_tests", |tx| {
            tx.execute("DELETE FROM skipped_tests WHERE rel = ?1", params![rel])?;
            let mut stmt = tx.prepare("INSERT OR IGNORE INTO skipped_tests(rel, name) VALUES (?1, ?2)")?;
            for name in names {
                stmt.execute(params![rel, name])?;
            }
            Ok(())
        })
    }

    /// The names of the test cases `rel` was last recorded as skipping. Empty for
    /// a file that skipped nothing and for a file the index has never seen.
    pub fn skipped_tests(&self, rel: &str) -> anyhow::Result<HashSet<String>> {
        let mut stmt = self.conn.prepare_cached("SELECT name FROM skipped_tests WHERE rel = ?1")?;
        let rows = stmt.query_map(params![rel], |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
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
        let keep: HashSet<&str> = present.iter().map(|s| s.as_str()).collect();
        self.savepoint("remove_missing", |tx| {
            let mut removed = 0;
            for rel in existing.iter().filter(|r| !keep.contains(r.as_str())) {
                for (table, column) in PER_FILE_TABLES {
                    tx.execute(&format!("DELETE FROM {table} WHERE {column} = ?1"), params![rel])?;
                }
                // Edges from files that stayed still point here. The target is gone, so
                // the edge is no longer resolved: it keeps its specifier and says so.
                tx.execute(
                    "UPDATE edges SET to_rel = NULL, resolution = 'unresolved' WHERE to_rel = ?1",
                    params![rel],
                )?;
                removed += 1;
            }
            Ok(removed)
        })
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

    /// The stat check is only ever allowed to say "definitely unchanged". A
    /// missing row, a value that differs, or a stat that could not answer all
    /// have to send the caller down the read-and-hash path.
    #[test]
    fn unchanged_by_stat_needs_a_row_and_two_real_values() {
        let mut ix = Index::open_in_memory().unwrap();
        assert!(!ix.unchanged_by_stat("src/a.ts", 10, 100).unwrap(), "a file with no row was never indexed");
        ix.upsert_file_stat("src/a.ts", "typescript", "h1", "ok", 10, 100).unwrap();
        assert!(ix.unchanged_by_stat("src/a.ts", 10, 100).unwrap());
        assert!(!ix.unchanged_by_stat("src/a.ts", 11, 100).unwrap(), "a different size is a change");
        assert!(!ix.unchanged_by_stat("src/a.ts", 10, 101).unwrap(), "a different mtime is a change");

        ix.upsert_file_stat("src/b.ts", "typescript", "h1", "ok", 0, 100).unwrap();
        assert!(!ix.unchanged_by_stat("src/b.ts", 0, 100).unwrap(), "an unknown size is not a match");
        ix.upsert_file_stat("src/c.ts", "typescript", "h1", "ok", 10, 0).unwrap();
        assert!(!ix.unchanged_by_stat("src/c.ts", 10, 0).unwrap(), "an unknown mtime is not a match");
    }

    /// A file that was touched but not edited has to have its stat brought up
    /// to date, or the run that read it to find that out pays that read on
    /// every later run too. Nothing about the content changed, so the hash is
    /// left exactly as it was.
    #[test]
    fn refresh_stat_updates_a_row_without_touching_its_hash() {
        let mut ix = Index::open_in_memory().unwrap();
        ix.upsert_file_stat("src/a.ts", "typescript", "h1", "ok", 10, 100).unwrap();
        ix.refresh_stat("src/a.ts", 10, 250).unwrap();
        assert!(ix.unchanged_by_stat("src/a.ts", 10, 250).unwrap());
        assert_eq!(ix.file_hash("src/a.ts").unwrap().as_deref(), Some("h1"), "a refresh says nothing about content");

        ix.refresh_stat("src/a.ts", 0, 300).unwrap();
        ix.refresh_stat("src/a.ts", 10, 0).unwrap();
        assert!(ix.unchanged_by_stat("src/a.ts", 10, 250).unwrap(), "a stat that could not answer is not one to store");

        ix.refresh_stat("src/gone.ts", 10, 250).unwrap();
        assert!(ix.file_hash("src/gone.ts").unwrap().is_none(), "a refresh never invents a file row");
    }

    #[test]
    fn file_stat_reads_a_real_file_and_gives_up_quietly() {
        let dir = unique_cache_dir("stat");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.ts");
        std::fs::write(&path, "export const a = 1;\n").unwrap();
        let (size, mtime) = file_stat(&path);
        assert_eq!(size, 20);
        assert!(mtime > 0, "a file on disk has a modification time");
        // Nanoseconds, not seconds. Two saves inside one wall clock second is an
        // ordinary editor-hook workflow, and if they end at the same length a
        // second-granularity mtime would call the second one unchanged and leave
        // the file's findings stale until it is edited again.
        let handle = std::fs::File::options().write(true).open(&path).unwrap();
        handle.set_modified(UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000)).unwrap();
        drop(handle);
        assert_eq!(file_stat(&path).1, 1_000_000_000_000_000_000, "the modification time is nanoseconds");
        assert_eq!(file_stat(&dir.join("nothing.ts")), (0, 0), "a path that is not there answers nothing");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The run-wide transaction takes the write lock when it opens, so a second
    /// run waits for the first through the busy handler and then says the
    /// database is busy. A deferred transaction would instead read happily and
    /// fail on its first write with a snapshot conflict, which the busy handler
    /// is never consulted for: the second run would fail immediately rather than
    /// wait its turn.
    #[test]
    fn begin_takes_the_write_lock_up_front() {
        let dir = unique_cache_dir("begin");
        let path = dir.join("index.db");
        let mut first = Index::open_at(&path).unwrap();
        let mut second = Index::open_at(&path).unwrap();
        // Short, so the test does not sit out the real five second timeout. The
        // point is that the wait happens at all.
        let wait = std::time::Duration::from_millis(50);
        second.conn().busy_timeout(wait).unwrap();

        first.begin().unwrap();
        assert!(first.begin().is_err(), "one transaction at a time on one connection");

        let started = std::time::Instant::now();
        let err = second.begin().expect_err("the first run holds the write lock");
        let waited = started.elapsed();
        let code = err.downcast_ref::<rusqlite::Error>().map(|e| match e {
            rusqlite::Error::SqliteFailure(f, _) => f.code,
            _ => rusqlite::ErrorCode::Unknown,
        });
        assert_eq!(code, Some(rusqlite::ErrorCode::DatabaseBusy), "{err:?}");
        assert!(waited >= wait / 2, "the busy handler has to be consulted, but it gave up after {waited:?}");

        first.commit().unwrap();
        second.begin().expect("the write lock is free once the first run commits");
        second.commit().unwrap();
        drop(first);
        drop(second);
        let _ = std::fs::remove_dir_all(&dir);
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

    /// The skipped set is per file and replaces wholesale, because it stands for
    /// "what this version of the file skips". A name that leaves the file has to
    /// leave the table, or re-skipping it later would look like an old decision.
    #[test]
    fn skipped_tests_round_trip_and_replace() {
        let mut ix = Index::open_in_memory().unwrap();
        assert!(ix.skipped_tests("src/a.test.ts").unwrap().is_empty(), "a file the index never saw skips nothing");

        ix.replace_skipped_tests("src/a.test.ts", &["one".to_string(), "two".to_string()]).unwrap();
        assert_eq!(
            ix.skipped_tests("src/a.test.ts").unwrap(),
            HashSet::from(["one".to_string(), "two".to_string()]),
            "both names come back"
        );
        assert!(ix.skipped_tests("src/b.test.ts").unwrap().is_empty(), "the set is per file");

        ix.replace_skipped_tests("src/a.test.ts", &["two".to_string()]).unwrap();
        assert_eq!(
            ix.skipped_tests("src/a.test.ts").unwrap(),
            HashSet::from(["two".to_string()]),
            "replace must drop the names that are no longer skipped"
        );

        // Two cases can share a name; the memory is a set, not a count.
        ix.replace_skipped_tests("src/c.test.ts", &["same".to_string(), "same".to_string()]).unwrap();
        assert_eq!(ix.skipped_tests("src/c.test.ts").unwrap(), HashSet::from(["same".to_string()]));

        ix.replace_skipped_tests("src/a.test.ts", &[]).unwrap();
        assert!(ix.skipped_tests("src/a.test.ts").unwrap().is_empty(), "a file that skips nothing stores nothing");
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
        ix.replace_skipped_tests("a.ts", &["one".to_string()]).unwrap();
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
        for table in ["files", "symbols", "edges", "allow_lines", "skipped_tests", "findings_cache"] {
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
    fn meta_round_trip() {
        let mut ix = Index::open_in_memory().unwrap();
        assert_eq!(ix.meta_get("last_verdict").unwrap(), None, "a key that was never written has no value");
        ix.meta_set("last_verdict", r#"{"status":"pass"}"#).unwrap();
        assert_eq!(ix.meta_get("last_verdict").unwrap().as_deref(), Some(r#"{"status":"pass"}"#));
        // The second write replaces the first: one row per key, so a run reading
        // this back sees the last verdict rather than a history of them.
        ix.meta_set("last_verdict", r#"{"status":"block"}"#).unwrap();
        assert_eq!(ix.meta_get("last_verdict").unwrap().as_deref(), Some(r#"{"status":"block"}"#));
        assert_eq!(ix.meta_get("never_written").unwrap(), None);
    }

    /// The `params` column arrives with schema 5, so every index written by a
    /// build that stamped 4 has to be thrown away rather than queried for a
    /// column it does not have.
    #[test]
    fn schema_five_rebuilds_a_schema_four_index() {
        let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = unique_cache_dir("schema-four");
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("LOCRIN_CACHE_DIR", &dir);
        let repo = Path::new("C:/repo/schema-four");

        let mut ix = Index::open(repo).unwrap();
        ix.upsert_file("marker.ts", "typescript", "h1", "ok").unwrap();
        ix.conn().execute("UPDATE meta SET value = '4' WHERE key = 'schema_version'", []).unwrap();
        drop(ix);

        let ix = Index::open(repo).unwrap();
        let version: String =
            ix.conn().query_row("SELECT value FROM meta WHERE key = 'schema_version'", [], |r| r.get(0)).unwrap();
        assert_eq!(version, "5");
        assert_eq!(version, SCHEMA_VERSION);
        assert!(ix.file_hash("marker.ts").unwrap().is_none(), "a schema 4 index must be rebuilt, not restamped");
        drop(ix);

        std::env::remove_var("LOCRIN_CACHE_DIR");
        let _ = std::fs::remove_dir_all(&dir);
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
