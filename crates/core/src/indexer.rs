//! Puts one parsed file into every table the index keeps about a file.

use crate::edges;
use crate::imports;
use crate::index::Index;
use crate::parse::ParsedFile;
use crate::resolve::Resolver;
use crate::symbols;
use crate::ALLOW_MARK;

/// Records `file` under `hash`. The `files` row is written last on purpose: it
/// carries the content hash that later runs compare against, so anything that
/// fails before it leaves the file looking stale and it is redone next run.
pub fn record(ix: &mut Index, file: &ParsedFile, hash: &str, resolver: &Resolver) -> anyhow::Result<()> {
    let syms = symbols::extract(file);
    symbols::store(ix, file, &syms)?;
    let edges = edges::from_imports(&file.rel, &imports::extract(file), resolver);
    edges::store(ix, &file.rel, &edges)?;
    let allow: Vec<u32> =
        file.source.lines().enumerate().filter(|(_, l)| l.contains(ALLOW_MARK)).map(|(i, _)| i as u32 + 1).collect();
    ix.replace_allow_lines(&file.rel, &allow)?;
    let status = if file.has_error { "error" } else { "ok" };
    ix.upsert_file(&file.rel, file.language.as_str(), hash, status)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::content_hash;
    use crate::parse::parse_file;
    use crate::walk::canonical_root;
    use std::collections::HashSet;
    use std::path::PathBuf;

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
}
