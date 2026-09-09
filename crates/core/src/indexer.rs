//! Puts one parsed file into every table the index keeps about a file.

use std::collections::HashSet;

use crate::edges;
use crate::imports;
use crate::index::Index;
use crate::parse::ParsedFile;
use crate::resolve::Resolver;
use crate::symbols;
use crate::testcases;
use crate::ALLOW_MARK;

/// Every import the file has, syntactic ones first. A file whose tree came back
/// with errors also gets the text scan merged in, because tree-sitter's recovery
/// drops whole statements and the leftover edge set would otherwise let a broken
/// file make the modules it really imports look dead. Syntactic entries win where
/// both found the same specifier: they carry the names and bindings.
///
/// Every import of such a file also takes the whole module. A clause recovery did
/// keep may still be a truncated one, and a name missing from it would make a
/// live export look dead; more edges is the safe direction (spec 3.2).
fn file_imports(file: &ParsedFile) -> Vec<imports::Import> {
    let mut found = imports::extract(file);
    if file.has_error {
        found.iter_mut().filter(|i| !i.names.iter().any(|n| n == "*")).for_each(|i| i.names.push("*".into()));
        let known: HashSet<String> = found.iter().map(|i| i.specifier.clone()).collect();
        found
            .extend(imports::extract_text_fallback(&file.source).into_iter().filter(|i| !known.contains(&i.specifier)));
    }
    found
}

/// Records `file` under `hash`, without a stat for a later run to compare
/// against, so that run reads and hashes the file rather than skipping it.
/// Callers that took the file's size and modification time before reading it
/// pass them to [`record_with_stat`] instead.
pub fn record(ix: &mut Index, file: &ParsedFile, hash: &str, resolver: &Resolver) -> anyhow::Result<()> {
    record_with_stat(ix, file, hash, resolver, (0, 0))
}

/// Records `file` under `hash` and `stat`. The `files` row is written last on
/// purpose: it carries the content hash that later runs compare against, so
/// anything that fails before it leaves the file looking stale and it is redone
/// next run.
///
/// That row also carries the file's size and modification time, which is what
/// lets a later run skip reading the file at all. `stat` must be the one the
/// caller took BEFORE it read the bytes it is recording. A stat taken here would
/// belong to whatever is on disk now, which is not necessarily what was read: a
/// save landing in between would be stored as the new stat beside the old hash,
/// and every later narrowed run would match that stat and skip the file, serving
/// stale symbols, edges and findings until it was edited again.
///
/// Zero for either value is "no stat", which never matches, so the file is
/// simply always read and hashed.
pub fn record_with_stat(
    ix: &mut Index,
    file: &ParsedFile,
    hash: &str,
    resolver: &Resolver,
    stat: (i64, i64),
) -> anyhow::Result<()> {
    let syms = symbols::extract(file);
    symbols::store(ix, file, &syms)?;
    let edges = edges::from_imports(&file.rel, &file_imports(file), resolver);
    edges::store(ix, &file.rel, &edges)?;
    let allow: Vec<u32> =
        file.source.lines().enumerate().filter(|(_, l)| l.contains(ALLOW_MARK)).map(|(i, _)| i as u32 + 1).collect();
    ix.replace_allow_lines(&file.rel, &allow)?;
    // What this version of the file skips, so the next run can tell a test
    // skipped in that change from one that was already skipped. Non-test files
    // are replaced with nothing rather than left alone: a file that stops being
    // a test file (renamed out of `__tests__`, say) must not keep the set the
    // version before it stored.
    let skipped: Vec<String> = if testcases::is_test_file(&file.rel) {
        testcases::extract(file).into_iter().filter(|c| c.skipped).map(|c| c.name).collect()
    } else {
        Vec::new()
    };
    ix.replace_skipped_tests(&file.rel, &skipped)?;
    let status = if file.has_error { "error" } else { "ok" };
    ix.upsert_file_stat(&file.rel, file.language.as_str(), hash, status, stat.0, stat.1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::content_hash;
    use crate::parse::parse_file;
    use crate::walk::canonical_root;
    use std::collections::HashSet;
    use std::path::PathBuf;

    /// A database path of its own, so two tests never share one file.
    fn scratch_db(tag: &str) -> PathBuf {
        let nanos =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("locrin-{tag}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("index.db")
    }

    /// A file the parser could not read is an unknown importer of every module it
    /// names, not only of the ones recovery dropped. A clause tree-sitter did
    /// recover may be a truncated one, so the names on it cannot be trusted to be
    /// all of them, and a missing name would make a live export look dead.
    #[test]
    fn every_import_of_a_parse_failed_file_takes_the_whole_module() {
        use crate::parse::parse_source;
        use std::path::Path;

        let clean =
            parse_source(Path::new("src/ok.tsx"), "src/ok.tsx", "import { a } from \"./a\";\n".to_string()).unwrap();
        assert!(!clean.has_error);
        assert_eq!(file_imports(&clean)[0].names, vec!["a".to_string()], "a clean file keeps the names it read");

        let source = "import { a } from \"./a\";\nconst shell = <View>;\nimport { b } from \"./b\";\n";
        let broken = parse_source(Path::new("src/broken.tsx"), "src/broken.tsx", source.to_string()).unwrap();
        assert!(broken.has_error, "the fixture must be a file that failed to parse");
        let found = file_imports(&broken);
        let mut specifiers: Vec<&str> = found.iter().map(|i| i.specifier.as_str()).collect();
        specifiers.sort();
        assert_eq!(specifiers, vec!["./a", "./b"], "both the recovered and the dropped import are edges");
        assert!(found.iter().all(|i| i.names.iter().any(|n| n == "*")), "{found:?}");
    }

    #[test]
    fn record_fills_every_table_and_writes_the_file_row_last() {
        let root = canonical_root(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mini"));
        let indexed: HashSet<String> = ["src/index.ts", "src/util.ts"].iter().map(|s| s.to_string()).collect();
        let resolver = Resolver::new(&root, indexed);
        let mut ix = Index::open_in_memory().unwrap();
        let mut file = parse_file(&root, &root.join("src/index.ts")).unwrap().unwrap();
        file.source.push_str("// locrin:allow\n");
        record(&mut ix, &file, &content_hash(&file.source), &resolver).unwrap();

        assert_eq!(ix.parse_status("src/index.ts").unwrap().as_deref(), Some("ok"));
        assert_eq!(symbols::exported(&ix).unwrap().len(), 1);
        assert_eq!(edges::dependents(&ix, "src/util.ts").unwrap(), vec!["src/index.ts".to_string()]);
        assert!(ix.is_allowed("src/index.ts", 6).unwrap());

        // Break the symbols table: record must fail before it stamps the file row.
        let mut broken = Index::open_in_memory().unwrap();
        broken.conn().execute_batch("DROP TABLE symbols").unwrap();
        assert!(record(&mut broken, &file, "h2", &resolver).is_err());
        assert_eq!(broken.parse_status("src/index.ts").unwrap(), None, "a failed record must not look indexed");
    }

    /// A test file's skipped cases are remembered so a later run can tell a new
    /// skip from an old one. A file that is not a test file has nothing to
    /// remember, and recording it must say so rather than leaving whatever an
    /// earlier version stored.
    #[test]
    fn record_remembers_what_a_test_file_skips_and_nothing_for_other_files() {
        use crate::parse::parse_source;
        use std::path::Path;

        let resolver = Resolver::new(Path::new("/repo"), HashSet::new());
        let mut ix = Index::open_in_memory().unwrap();

        let source = "it.skip(\"one\", () => {});\nit(\"two\", () => {});\nxit(\"three\", () => {});\n".to_string();
        let test_file = parse_source(Path::new("src/a.test.ts"), "src/a.test.ts", source).unwrap();
        record(&mut ix, &test_file, &content_hash(&test_file.source), &resolver).unwrap();
        assert_eq!(
            ix.skipped_tests("src/a.test.ts").unwrap(),
            HashSet::from(["one".to_string(), "three".to_string()]),
            "both skipped forms are remembered and the active case is not"
        );

        // The same source under a name that is not a test file: nothing is stored.
        let plain = parse_source(Path::new("src/a.ts"), "src/a.ts", test_file.source.clone()).unwrap();
        record(&mut ix, &plain, &content_hash(&plain.source), &resolver).unwrap();
        assert!(ix.skipped_tests("src/a.ts").unwrap().is_empty(), "only test files carry a skipped set");

        // Un-skipping is a change like any other: the old name has to go.
        let fixed =
            parse_source(Path::new("src/a.test.ts"), "src/a.test.ts", "it(\"one\", () => {});\n".to_string()).unwrap();
        record(&mut ix, &fixed, &content_hash(&fixed.source), &resolver).unwrap();
        assert!(ix.skipped_tests("src/a.test.ts").unwrap().is_empty(), "re-recording replaces the whole set");
    }

    /// The stat stored with a file is the one the caller took before it read the
    /// file, never one taken while recording. A save landing between the read
    /// and the record would otherwise be stored as the stat of the new bytes
    /// beside the hash of the old ones, and every later narrowed run would match
    /// that stat, skip the file, and keep serving stale symbols, edges and
    /// findings until somebody edited it again.
    #[test]
    fn record_stores_the_stat_it_was_given_not_one_it_takes_itself() {
        let root = canonical_root(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mini"));
        let indexed: HashSet<String> = ["src/index.ts", "src/util.ts"].iter().map(|s| s.to_string()).collect();
        let resolver = Resolver::new(&root, indexed);
        let file = parse_file(&root, &root.join("src/index.ts")).unwrap().unwrap();
        let on_disk = crate::index::file_stat(&file.path);
        // What a run holds after a save lands between its stat and its record:
        // the bytes on disk are newer than the stat it took.
        let taken_before_the_read = (on_disk.0, on_disk.1 - 1);

        let mut ix = Index::open_in_memory().unwrap();
        let hash = content_hash(&file.source);
        record_with_stat(&mut ix, &file, &hash, &resolver, taken_before_the_read).unwrap();
        assert!(ix.unchanged_by_stat("src/index.ts", taken_before_the_read.0, taken_before_the_read.1).unwrap());
        assert!(
            !ix.unchanged_by_stat("src/index.ts", on_disk.0, on_disk.1).unwrap(),
            "the file on disk is newer than the stat that was recorded, so the next run has to read it"
        );

        let mut plain = Index::open_in_memory().unwrap();
        record(&mut plain, &file, &hash, &resolver).unwrap();
        assert!(
            !plain.unchanged_by_stat("src/index.ts", on_disk.0, on_disk.1).unwrap(),
            "a caller with no stat to offer records none, so the file is always read again"
        );
    }

    /// A whole run is one unit: `begin` opens the transaction, every `record`
    /// nests a savepoint inside it, and only `commit` makes the run visible to
    /// the next one. A run that ends without committing leaves the index exactly
    /// as it found it, so a half-written run is never mistaken for a complete one.
    #[test]
    fn a_run_commits_once_and_an_abandoned_run_leaves_nothing() {
        let root = canonical_root(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mini"));
        let indexed: HashSet<String> = ["src/index.ts", "src/util.ts"].iter().map(|s| s.to_string()).collect();
        let resolver = Resolver::new(&root, indexed);
        let a = parse_file(&root, &root.join("src/index.ts")).unwrap().unwrap();
        let b = parse_file(&root, &root.join("src/util.ts")).unwrap().unwrap();

        let committed = scratch_db("indexer-commit");
        let mut ix = Index::open_at(&committed).unwrap();
        ix.begin().unwrap();
        record(&mut ix, &a, &content_hash(&a.source), &resolver).unwrap();
        record(&mut ix, &b, &content_hash(&b.source), &resolver).unwrap();
        ix.commit().unwrap();
        drop(ix);
        let ix = Index::open_at(&committed).unwrap();
        assert_eq!(ix.all_files().unwrap(), vec!["src/index.ts".to_string(), "src/util.ts".to_string()]);
        drop(ix);

        let abandoned = scratch_db("indexer-abandoned");
        let mut ix = Index::open_at(&abandoned).unwrap();
        ix.begin().unwrap();
        record(&mut ix, &a, &content_hash(&a.source), &resolver).unwrap();
        drop(ix);
        let ix = Index::open_at(&abandoned).unwrap();
        assert!(ix.all_files().unwrap().is_empty(), "a run that never committed must leave no trace");
        drop(ix);

        for db in [committed, abandoned] {
            std::fs::remove_dir_all(db.parent().unwrap()).ok();
        }
    }
}
