//! Import edges: one row per name a file takes from another. `to_rel` is set only
//! for resolved edges; external and unresolved edges keep the specifier so a
//! later rule (or a human) can see what was attempted.

use rusqlite::params;

use crate::imports::Import;
use crate::index::Index;
use crate::resolve::{Resolution, Resolver};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edge {
    pub from_rel: String,
    pub to_rel: Option<String>,
    pub specifier: String,
    /// `default`, `*`, an exported name, or `""` for a side-effect import.
    pub name: String,
    pub kind: String,
    pub resolution: String,
    pub line: u32,
}

pub fn from_imports(from_rel: &str, imports: &[Import], resolver: &Resolver) -> Vec<Edge> {
    let mut out = Vec::new();
    for i in imports {
        let resolution = resolver.resolve(from_rel, &i.specifier);
        let to_rel = match &resolution {
            Resolution::Resolved(r) => Some(r.clone()),
            _ => None,
        };
        let names: Vec<String> = if i.names.is_empty() { vec![String::new()] } else { i.names.clone() };
        for name in names {
            out.push(Edge {
                from_rel: from_rel.to_string(),
                to_rel: to_rel.clone(),
                specifier: i.specifier.clone(),
                name,
                kind: i.kind.as_str().to_string(),
                resolution: resolution.as_str().to_string(),
                line: i.line,
            });
        }
    }
    out
}

/// Replaces every edge leaving `from_rel` in one savepoint, for the same reason
/// `symbols::store` does: a half-written edge set looks complete to the hash check.
pub fn store(ix: &mut Index, from_rel: &str, edges: &[Edge]) -> anyhow::Result<()> {
    ix.savepoint("edges", |tx| {
        tx.execute("DELETE FROM edges WHERE from_rel = ?1", params![from_rel])?;
        let mut stmt = tx.prepare(
            "INSERT INTO edges(from_rel, to_rel, specifier, name, kind, resolution, line)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )?;
        for e in edges {
            stmt.execute(params![e.from_rel, e.to_rel, e.specifier, e.name, e.kind, e.resolution, e.line])?;
        }
        Ok(())
    })
}

const COLUMNS: &str = "from_rel, to_rel, specifier, name, kind, resolution, line";

fn row(r: &rusqlite::Row) -> rusqlite::Result<Edge> {
    Ok(Edge {
        from_rel: r.get(0)?,
        to_rel: r.get(1)?,
        specifier: r.get(2)?,
        name: r.get(3)?,
        kind: r.get(4)?,
        resolution: r.get(5)?,
        line: r.get(6)?,
    })
}

pub fn from_file(ix: &Index, rel: &str) -> anyhow::Result<Vec<Edge>> {
    let mut stmt = ix.conn().prepare(&format!("SELECT {COLUMNS} FROM edges WHERE from_rel = ?1 ORDER BY line, id"))?;
    let rows = stmt.query_map(params![rel], row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn resolved(ix: &Index) -> anyhow::Result<Vec<Edge>> {
    let mut stmt = ix
        .conn()
        .prepare(&format!("SELECT {COLUMNS} FROM edges WHERE resolution = 'resolved' ORDER BY from_rel, line, id"))?;
    let rows = stmt.query_map([], row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn dependents(ix: &Index, rel: &str) -> anyhow::Result<Vec<String>> {
    let mut stmt = ix.conn().prepare(
        "SELECT DISTINCT from_rel FROM edges WHERE to_rel = ?1 AND resolution = 'resolved' ORDER BY from_rel",
    )?;
    let rows = stmt.query_map(params![rel], |r| r.get(0))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::imports;
    use crate::parse::parse_file;
    use crate::walk::canonical_root;
    use std::collections::HashSet;
    use std::path::PathBuf;

    fn mini() -> PathBuf {
        canonical_root(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mini"))
    }

    #[test]
    fn edges_from_the_mini_fixture_resolve_and_round_trip() {
        let root = mini();
        let index_ts = parse_file(&root, &root.join("src/index.ts")).unwrap().unwrap();
        let indexed: HashSet<String> = ["src/index.ts", "src/util.ts"].iter().map(|s| s.to_string()).collect();
        let resolver = Resolver::new(&root, indexed);
        let edges = from_imports(&index_ts.rel, &imports::extract(&index_ts), &resolver);
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].to_rel.as_deref(), Some("src/util.ts"));
        assert_eq!(
            (edges[0].name.as_str(), edges[0].kind.as_str(), edges[0].resolution.as_str(), edges[0].line),
            ("helper", "import", "resolved", 1)
        );

        let mut ix = Index::open_in_memory().unwrap();
        store(&mut ix, "src/index.ts", &edges).unwrap();
        store(&mut ix, "src/index.ts", &edges).unwrap();
        assert_eq!(from_file(&ix, "src/index.ts").unwrap(), edges, "store replaces, it does not accumulate");
        assert_eq!(resolved(&ix).unwrap().len(), 1);
        assert_eq!(dependents(&ix, "src/util.ts").unwrap(), vec!["src/index.ts".to_string()]);
        assert!(dependents(&ix, "src/index.ts").unwrap().is_empty());
    }

    #[test]
    fn side_effect_and_external_imports_keep_a_row() {
        let root = mini();
        let resolver = Resolver::new(&root, HashSet::new());
        let src = "import \"./util\";\nimport react from \"react\";\nimport { x } from \"./nowhere\";\n".to_string();
        let file = crate::parse::parse_source(&root.join("src/a.ts"), "src/a.ts", src).unwrap();
        let edges = from_imports("src/a.ts", &imports::extract(&file), &resolver);
        let view: Vec<(&str, &str, Option<&str>)> =
            edges.iter().map(|e| (e.name.as_str(), e.resolution.as_str(), e.to_rel.as_deref())).collect();
        // util.ts exists on disk but is not in the (empty) indexed set: external, not a graph node.
        assert_eq!(view, vec![("", "external", None), ("default", "external", None), ("x", "unresolved", None)]);
    }
}
