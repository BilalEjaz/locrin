//! Puts one parsed file into every table the index keeps about a file.

use std::collections::HashSet;

use crate::edges;
use crate::imports;
use crate::index::Index;
use crate::parse::ParsedFile;
use crate::resolve::Resolver;
use crate::symbols;
use crate::ALLOW_MARK;

/// Every import the file has, syntactic ones first. A file whose tree came back
/// with errors also gets the text scan merged in, because tree-sitter's recovery
/// drops whole statements and the leftover edge set would otherwise let a broken
/// file make the modules it really imports look dead. Syntactic entries win where
/// both found the same specifier: they carry the names and bindings.
fn file_imports(file: &ParsedFile) -> Vec<imports::Import> {
    let mut found = imports::extract(file);
    if file.has_error {
        let known: HashSet<String> = found.iter().map(|i| i.specifier.clone()).collect();
        found
            .extend(imports::extract_text_fallback(&file.source).into_iter().filter(|i| !known.contains(&i.specifier)));
    }
    found
}

/// Records `file` under `hash`. The `files` row is written last on purpose: it
/// carries the content hash that later runs compare against, so anything that
/// fails before it leaves the file looking stale and it is redone next run.
///
/// That row also carries the file's size and modification time, which is what
/// lets a later run skip reading the file at all. A path that cannot be stat'ed
/// records zeroes, and a zero never matches, so such a file is simply always
/// read and hashed.
pub fn record(ix: &mut Index, file: &ParsedFile, hash: &str, resolver: &Resolver) -> anyhow::Result<()> {
    let syms = symbols::extract(file);
    symbols::store(ix, file, &syms)?;
    let edges = edges::from_imports(&file.rel, &file_imports(file), resolver);
    edges::store(ix, &file.rel, &edges)?;
    let allow: Vec<u32> =
        file.source.lines().enumerate().filter(|(_, l)| l.contains(ALLOW_MARK)).map(|(i, _)| i as u32 + 1).collect();
    ix.replace_allow_lines(&file.rel, &allow)?;
    let status = if file.has_error { "error" } else { "ok" };
    let (size, mtime) = crate::index::file_stat(&file.path);
    ix.upsert_file_stat(&file.rel, file.language.as_str(), hash, status, size, mtime)
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
