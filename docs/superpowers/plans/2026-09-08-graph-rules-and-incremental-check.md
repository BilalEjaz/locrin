# Graph Rules and Incremental Check Implementation Plan (version one, plan 2 of 4)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the index an import graph and ship the five rules that need it or need control flow (`unused-import`, `unreachable`, `dead-export`, `dead-file`, `boundary-violation`), then make `check` incremental for real: cached per-file findings, `--base` and `--since` git scopes, and a SARIF reporter.

**Architecture:** Part A (branch `engine/graph`) adds three index tables (`edges`, `allow_lines`, `findings_cache`), export names on symbols, import extraction, a heuristic module resolver (relative paths, tsconfig `paths` and `baseUrl`, workspace packages, barrels one level), entry-point detection, and a `Scope` on every rule: file rules run over the files parsed this run, graph rules run as queries over the index. Part B (branch `engine/incremental`) serves unchanged files' file-rule findings from `findings_cache`, reports graph findings for the neighbours of whatever changed, takes the scope from `git diff`, and renders SARIF 2.1.0. Nothing in either part runs an LLM or touches the network.

**Tech Stack:** Rust 2021 (stable 1.80+), tree-sitter 0.23 + tree-sitter-typescript 0.23, rusqlite 0.32 (bundled), blake3, serde + serde_json, toml, globset, clap 4. Tests are plain `#[test]` functions over fixture directories under `crates/*/tests/fixtures`. `git` on PATH for Part B Task 17 only.

**Spec:** `docs/superpowers/specs/2026-09-05-agent-native-quality-gate-design.md`. This plan implements: 3.2 (edges, findings_cache, the incremental rule, resolution and its blind spots), 4.1 (`dead-export`, `dead-file`, `unreachable`, `unused-import`, `boundary-violation` config-driven half), 4.3 (blocking policy), 7.3 (SARIF), 7.5 (config sections `entry points` and `boundaries`), 8.1 and 8.2 (`check --base <ref>`, `check --since <ref>`), 9 (parse failures exempt a file from every rule, including graph rules), 10.1 (must-flag / must-not-flag / edge fixtures per rule), and the 3.4 warm-diff target (30 files under 1 s). Plan 1 (`docs/superpowers/plans/2026-09-05-engine-core.md`) is the code this builds on; read `crates/rules/src/lib.rs` and `crates/cli/src/run.rs` before Task 9.

**What this plan does not do.** `swallowed-error`, the two test rules, and the security pack are plan 3. `init`, hooks, and the MCP server are plan 4. `already-exists` is release two. The labelled corpus (spec 10.2) does not exist yet; Task 15 substitutes a hand-labelled 20-sample precision check per new rule on FastLift and STOPS for the founder if a rule misses 17/20.

## Global Constraints

- Product name is Locrin: binary `locrin`, config `locrin.toml` (`core::config::CONFIG_FILE`), baseline `locrin-baseline.json` (`core::baseline::BASELINE_FILE`). Do not add a fourth spelling.
- No LLM anywhere. No network anywhere. The only subprocess in this plan is `git` (Task 17), and only when `--base` or `--since` is passed.
- Languages: TypeScript (`.ts`, `.mts`, `.cts`), TSX (`.tsx`), JavaScript (`.js`, `.jsx`, `.mjs`, `.cjs`). `.d.ts` is never indexed; an import that resolves only to a `.d.ts` is `external`.
- Index schema version becomes `"2"` in Task 1 (spec 9: a mismatch rebuilds the index, log once). The schema is not bumped again in this plan; every table this plan needs is created in Task 1.
- Resolution never guesses (spec 3.2): a relative or aliased specifier that matches nothing on disk is `unresolved`; a bare specifier that no `paths` alias or workspace package owns is `external` because it cannot name a file in this repository; a specifier that exists on disk but is not an indexed source file (`.d.ts`, asset, JSON, excluded file, package directory) is `external`. Computed dynamic imports are skipped, never recorded.
- Rule contract: every rule declares `id`, `description` (one line, used by SARIF), `scope` (`File` or `Graph`), `category`, `default_severity`, `confidence`. `run` returns `anyhow::Result<Vec<Finding>>`. New rules and their defaults, all `Category::Erosion`:
  - `unused-import`: severity Low, confidence High (blocks; an unused binding is a fact, not a heuristic).
  - `unreachable`: severity Medium, confidence High.
  - `dead-export`: severity Low, confidence Medium (advisory; an unresolved import anywhere could be the consumer).
  - `dead-file`: severity Medium, confidence Medium (advisory).
  - `boundary-violation`: severity High, confidence High (the operator declared the rule; a resolved edge across it is certain).
- Finding ids stay `blake3(rule + "\x1f" + rel + "\x1f" + anchor)[..16]`. Anchors for the new rules are named in each task so ids survive line shifts.
- Spec 9 gate holds for graph findings too: a finding on a file whose index row says `parse_status = 'error'` is dropped, and a finding on a line the index recorded as carrying `locrin:allow` is dropped, even when that file was not parsed this run.
- Performance targets stay tests: cold index of `<home>/fasting-app` under 5 s release, warm single-file check under 300 ms, and (new, Task 19) warm 30-file check under 1 s. Benchmarks are `#[ignore]` and run with `cargo test --release -p locrin-cli -- --ignored --nocapture`. Task 15 re-runs the first two after Part A because import extraction and resolution add work to the cold path.
- Git: Part A on branch `engine/graph` off `main`; Part B on `engine/incremental` off `main` after Part A merges (stacked on `engine/graph` if Part A is still in review; retarget the PR before merging the base with `--delete-branch`). One commit per task, `cargo fmt --all` before every commit, plain messages in the existing `engine: ...` style, no attribution trailers of any kind, never `git add -A`. No em dashes in any text, code, or commit.
- Rust on the founder's machine: `export PATH="$HOME/.cargo/bin:$PATH"` in Git Bash before any `cargo` command.
- Expected line numbers in fixture tests were counted by hand. If a test fails only on a line number, recount against the fixture file before touching the rule; if the fixture and the rule agree and the plan is wrong, fix the assertion and say so in the commit body.

## Deviations recorded during execution

Both parts shipped with departures from the plan above. Each one is recorded here so the plan and the code agree: Part A's first, then Part B's.

### Part A

- The index schema version is `"3"`, not the `"2"` the Global Constraints name. Task 15c added `size` and `mtime` columns to `files` for the stat shortcut, which is a schema change and so a second bump. Spec 9 still applies: a version mismatch rebuilds the index and logs once.
- `dead-file` ships disabled by default. It came in below the spec 10.2 precision gate on the corpus substitute, 6/9 after the fixes, so a repository opts in with `[rules.dead-file]` and `enabled = true`.
- Tasks 15b and 15c were added after the plan's Task 15 gates tripped, on precision and on the benchmarks respectively. Their briefs are in the SDD workspace and their results are in the precision report.
- The blanket entry globs `supabase/functions/**` and `plugins/**` were dropped after review. They cost `dead-export` recall: every file under them counted as an entry point, so exports nothing imports were never reported.
- `Index::upsert_file_stat` sits beside `upsert_file` rather than where the plan placed it.
- `remove_missing` moved inside `index_files` so that a run's recording and its pruning share one transaction scope, and a run that fails part way commits nothing.
- Non-recording runs, which is to say the baseline commands, index the repository into an in-memory database. The graph rules answer by querying an index, and on a fresh cache the repository's index is empty, so a baseline built from a read-only run would hold no graph findings at all.

Spec 4.1 still lists dead-file without a default; the founder decides whether the spec records the ships-off default.

### Part B

- Task 16 was integrated into the `run.rs` that Part A left behind rather than replacing it. The task's brief was written before Task 15c, which had already rebuilt the pipeline around the stat shortcut, so the brief's replacement text described code that no longer existed.
- Task 16b was added, and was not in the plan at all. Task 16 made `scan` warm the findings cache, which meant the file rules had to run over every parsed file rather than over the changed ones alone, and that pushed the cold benchmark from about 3 s to about 7.8 s against a 5 s target. Task 16b runs the file rules per file across the rayon pool: `Rule` is now `Sync`, `RuleContext.index` is `Option<&Index>` because no file rule reads an index, and `run_file_rules` is the new entry point. The cold benchmark is back inside the target.
- Named paths conflict with `--changed`, `--base` and `--since` at the CLI rather than one silently winning over the other. Each names the files the run sees, so a silent override gives a narrower run than the operator asked for and says nothing about it.
- The watermark-derived `before` set widens `--changed` and nothing else. A `--base` or `--since` verdict must be a function of the tree and the ref, and folding in the index's record of what changed since the last run would make the same command on the same tree answer differently on a second run, the first having consumed the deletion. The trade is recorded in spec 3.2.
- The warm-diff benchmark's listing goes one directory level down into `app/` rather than taking a flat slice of it. The spec's pull request is 30 files and the top level of `app/` holds 11, so a flat listing could not reach the number the spec names. The benchmark now fails rather than measuring fewer.
- The SARIF driver lists every rule, including one that ships off, and says so in `defaultConfiguration.enabled` as well as in `properties.enabledByDefault`. A consumer reading the standard, which is what GitHub code scanning does, learns the default enablement without knowing locrin's properties.

## File structure

Part A creates or modifies:

- `crates/core/src/index.rs` (modify): schema v2, `allow_lines` API, `parse_status`, `all_files`, cascading `remove_missing`.
- `crates/core/src/tree.rs` (create): tiny tree-sitter helpers shared by `symbols`, `imports`, and the rules: `text`, `line`, `has_keyword`.
- `crates/core/src/symbols.rs` (modify): `export_name`, export clauses, defaults, re-exports, `exported(ix)` query.
- `crates/core/src/imports.rs` (create): import, re-export, dynamic import, and require extraction with per-binding lines.
- `crates/core/src/project.rs` (create): JSONC stripping, `tsconfig.json` (`extends`, `baseUrl`, `paths`), `package.json` (`main`, `bin`, `exports`, `workspaces`), string path helpers.
- `crates/core/src/resolve.rs` (create): `Resolver` and `Resolution`.
- `crates/core/src/edges.rs` (create): `Edge`, `from_imports`, store and queries.
- `crates/core/src/entry.rs` (create): `EntryPoints`.
- `crates/core/src/indexer.rs` (create): `record` (one parsed file into every table).
- `crates/core/src/config.rs` (modify): `entry_points`, `boundaries`.
- `crates/core/src/lib.rs` (modify): new modules, `ALLOW_MARK` moves here.
- `crates/rules/src/lib.rs` (modify): `Scope`, `description`, `finding_at`, `line_span`, `RuleContext { index, entries }`, gate via index, registry.
- `crates/rules/src/{unused_import,unreachable,dead_export,dead_file,boundary}.rs` (create) with `crates/rules/tests/<rule>.rs` and `crates/rules/tests/fixtures/<rule>/{flag,clean,edge}`.
- `crates/rules/tests/common/mod.rs` (modify): builds an in-memory index for fixtures.
- `crates/cli/src/run.rs` (modify): whole-repository walk on every run, explicit paths as a scope, `EntryPoints`, `Resolver`.
- `crates/cli/tests/fixtures/repo` (modify) and `crates/cli/tests/cli.rs` (modify).

Part B creates or modifies:

- `crates/core/src/cache.rs` (create): `findings_cache` API and `config_hash`.
- `crates/cli/src/run.rs` (modify): cache-aware pipeline, adjacency, `scan` warms the cache.
- `crates/cli/src/git.rs` (create): `DiffScope`, `changed_files`.
- `crates/cli/src/main.rs` (modify): `--base`, `--since`, `--sarif`.
- `crates/reporters/src/sarif.rs` (create).
- `crates/cli/tests/cli.rs`, `crates/cli/tests/bench.rs` (modify).

---

## Part A: the import graph and five rules (branch `engine/graph`)

### Task 1: Schema version 2

**Files:**
- Modify: `crates/core/src/index.rs`

**Interfaces:**
- Consumes: nothing new.
- Produces: `SCHEMA_VERSION = "2"`; tables `symbols.export_name TEXT` (nullable), `edges`, `allow_lines`, `findings_cache`; `Index::parse_status(&self, rel) -> Result<Option<String>>`, `Index::all_files(&self) -> Result<Vec<String>>`, `Index::replace_allow_lines(&mut self, rel, lines: &[u32]) -> Result<()>`, `Index::is_allowed(&self, rel, line) -> Result<bool>`; `remove_missing` deletes from every table.

- [ ] **Step 1: Write the failing tests** (append inside `mod tests` in `index.rs`)

```rust
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
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p locrin-core index::tests`
Expected: compile errors, `replace_allow_lines`, `is_allowed`, `parse_status`, `all_files` not found.

- [ ] **Step 3: Implement the schema and the methods**

Replace `SCHEMA_VERSION` and `SCHEMA` at the top of `index.rs`:

```rust
pub const SCHEMA_VERSION: &str = "2";

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
const PER_FILE_TABLES: &[(&str, &str)] = &[
    ("symbols", "rel"),
    ("edges", "from_rel"),
    ("allow_lines", "rel"),
    ("findings_cache", "rel"),
    ("files", "rel"),
];
```

Add these methods to `impl Index` (after `changed`), and rewrite `remove_missing`:

```rust
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
            removed += 1;
        }
        tx.commit()?;
        Ok(removed)
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p locrin-core`
Expected: all pass, including the existing schema-mismatch tests (they now see `"2"`).

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add crates/core/src/index.rs
git commit -m "engine: schema v2 with edges, allow lines, and a findings cache table"
```

### Task 2: Export names on symbols

**Files:**
- Create: `crates/core/src/tree.rs`
- Modify: `crates/core/src/symbols.rs`, `crates/core/src/lib.rs`

**Interfaces:**
- Produces: `tree::text(node, src) -> &str`, `tree::line(node) -> u32`, `tree::has_keyword(node, kw) -> bool`; `Symbol.export_name: Option<String>`; `symbols::exported(ix) -> Result<Vec<Symbol>>` (every symbol with an export name, ordered by rel, start_line, start_col); `store` writes `export_name`.
- Export naming rules: a declaration under `export` is exported under its own name; under `export default` as `"default"`; `export default <expression>` yields one symbol kind `"default"` name `"default"`; `export { a, b as c }` yields one symbol per specifier, kind `"export"`, `name` = local, `export_name` = alias or local; the same with a `from` source yields kind `"reexport"`; `export * as ns from` yields kind `"reexport"`, name `"*"`, export_name `ns`; `export * from` yields no symbol (the edge carries it, Task 6).

- [ ] **Step 1: Write `tree.rs`** (no test of its own; `symbols` and `imports` tests cover it)

```rust
//! Small helpers over tree-sitter nodes shared by the extractors and the rules.

use tree_sitter::Node;

/// The source text of a node, or `""` when the span is not valid UTF-8.
pub fn text<'a>(node: Node, src: &'a str) -> &'a str {
    node.utf8_text(src.as_bytes()).unwrap_or("")
}

/// The 1-based line a node starts on.
pub fn line(node: Node) -> u32 {
    node.start_position().row as u32 + 1
}

/// Whether `node` has an anonymous child token spelled `keyword`, such as the
/// `default` in `export default` or the `type` in `import type`. Keyword tokens
/// are anonymous nodes whose kind is the keyword itself, so this is the only
/// way to see them; `to_sexp` never prints them.
pub fn has_keyword(node: Node, keyword: &str) -> bool {
    let mut cursor = node.walk();
    node.children(&mut cursor).any(|c| !c.is_named() && c.kind() == keyword)
}
```

Add `pub mod tree;` to `lib.rs` (alphabetical, after `symbols`).

- [ ] **Step 2: Write the failing tests** (append inside `mod tests` in `symbols.rs`)

```rust
    const EXPORTS: &str = r#"const a = 1;
function b() {}
export { a, b as c };
export default function main() {}
export { d } from "./d";
export * as e from "./e";
export * from "./f";
export const g = 2;
"#;

    #[test]
    fn export_names_cover_clauses_defaults_and_reexports() {
        let p = parse_source(Path::new("src/x.ts"), "src/x.ts", EXPORTS.to_string()).unwrap();
        let syms = extract(&p);
        let view: Vec<(String, String, Option<String>)> =
            syms.iter().map(|s| (s.kind.clone(), s.name.clone(), s.export_name.clone())).collect();
        assert_eq!(
            view,
            vec![
                ("const".into(), "a".into(), None),
                ("function".into(), "b".into(), None),
                ("export".into(), "a".into(), Some("a".into())),
                ("export".into(), "b".into(), Some("c".into())),
                ("function".into(), "main".into(), Some("default".into())),
                ("reexport".into(), "d".into(), Some("d".into())),
                ("reexport".into(), "*".into(), Some("e".into())),
                ("const".into(), "g".into(), Some("g".into())),
            ]
        );
        assert!(syms.iter().all(|s| s.exported == s.export_name.is_some()));
    }

    #[test]
    fn anonymous_default_exports_are_default_symbols() {
        for src in ["export default class {}\n", "export default function () {}\n", "export default 42;\n"] {
            let p = parse_source(Path::new("src/y.ts"), "src/y.ts", src.to_string()).unwrap();
            let syms = extract(&p);
            assert_eq!(syms.len(), 1, "{src}");
            assert_eq!(
                (syms[0].kind.as_str(), syms[0].name.as_str(), syms[0].export_name.as_deref()),
                ("default", "default", Some("default")),
                "{src}"
            );
        }
    }

    #[test]
    fn exported_query_returns_only_exported_symbols_in_order() {
        let mut ix = Index::open_in_memory().unwrap();
        let p = parse_source(Path::new("src/x.ts"), "src/x.ts", EXPORTS.to_string()).unwrap();
        let syms = extract(&p);
        store(&mut ix, &p, &syms).unwrap();
        let names: Vec<String> = exported(&ix).unwrap().into_iter().map(|s| s.export_name.unwrap()).collect();
        assert_eq!(names, vec!["a", "c", "default", "d", "e", "g"]);
    }
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p locrin-core symbols::tests`
Expected: compile errors, no field `export_name`, no function `exported`.

- [ ] **Step 4: Implement**

Replace everything in `symbols.rs` above `#[cfg(test)]` with:

```rust
use rusqlite::params;
use tree_sitter::Node;

use crate::index::Index;
use crate::parse::ParsedFile;
use crate::tree::{has_keyword, text};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub rel: String,
    pub kind: String,
    pub name: String,
    /// The name other files import this symbol by, or None when it is not exported.
    /// Differs from `name` for `export default` and `export { a as b }`.
    pub export_name: Option<String>,
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
    pub exported: bool,
}

/// How a declaration is exported, if at all.
#[derive(Clone, Copy)]
enum Export {
    No,
    Named,
    Default,
}

fn span_of(node: Node) -> (u32, u32, u32, u32) {
    let s = node.start_position();
    let e = node.end_position();
    (s.row as u32 + 1, s.column as u32, e.row as u32 + 1, e.column as u32)
}

fn push(out: &mut Vec<Symbol>, rel: &str, kind: &str, name: &str, export_name: Option<String>, node: Node) {
    let (sl, sc, el, ec) = span_of(node);
    out.push(Symbol {
        rel: rel.to_string(),
        kind: kind.to_string(),
        name: name.to_string(),
        exported: export_name.is_some(),
        export_name,
        start_line: sl,
        start_col: sc,
        end_line: el,
        end_col: ec,
    });
}

fn export_name(export: Export, own: &str) -> Option<String> {
    match export {
        Export::No => None,
        Export::Named => Some(own.to_string()),
        Export::Default => Some("default".to_string()),
    }
}

fn unquote(s: &str) -> String {
    s.trim_matches(|c| c == '"' || c == '\'').to_string()
}

fn collect(node: Node, src: &str, rel: &str, export: Export, out: &mut Vec<Symbol>) {
    let kind = match node.kind() {
        "function_declaration" | "generator_function_declaration" => "function",
        "class_declaration" | "abstract_class_declaration" => "class",
        "interface_declaration" => "interface",
        "type_alias_declaration" => "type",
        "enum_declaration" => "enum",
        "lexical_declaration" | "variable_declaration" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "variable_declarator" {
                    if let Some(name) = child.child_by_field_name("name") {
                        if name.kind() == "identifier" {
                            let name = text(name, src);
                            push(out, rel, "const", name, export_name(export, name), child);
                        }
                    }
                }
            }
            return;
        }
        "export_statement" => {
            collect_export(node, src, rel, out);
            return;
        }
        _ => return,
    };
    if let Some(name) = node.child_by_field_name("name") {
        let name = text(name, src);
        push(out, rel, kind, name, export_name(export, name), node);
    }
}

fn collect_export(node: Node, src: &str, rel: &str, out: &mut Vec<Symbol>) {
    let default = has_keyword(node, "default");
    if let Some(decl) = node.child_by_field_name("declaration") {
        let before = out.len();
        collect(decl, src, rel, if default { Export::Default } else { Export::Named }, out);
        // `export default function () {}`: a declaration with no name to export under.
        if out.len() == before && default {
            push(out, rel, "default", "default", Some("default".to_string()), node);
        }
        return;
    }
    if node.child_by_field_name("value").is_some() {
        // `export default <expression>`: an anonymous class, an object, a literal.
        push(out, rel, "default", "default", Some("default".to_string()), node);
        return;
    }
    let kind = if node.child_by_field_name("source").is_some() { "reexport" } else { "export" };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "export_clause" => {
                let mut inner = child.walk();
                for spec in child.named_children(&mut inner) {
                    if spec.kind() != "export_specifier" {
                        continue;
                    }
                    let Some(name) = spec.child_by_field_name("name") else { continue };
                    let local = unquote(text(name, src));
                    let alias = spec.child_by_field_name("alias").map(|a| unquote(text(a, src)));
                    push(out, rel, kind, &local, Some(alias.unwrap_or_else(|| local.clone())), spec);
                }
            }
            "namespace_export" => {
                // `export * as ns from "./x"`: the whole module under one name.
                if let Some(id) = child.named_child(0) {
                    push(out, rel, "reexport", "*", Some(unquote(text(id, src))), node);
                }
            }
            _ => {}
        }
    }
}

pub fn extract(file: &ParsedFile) -> Vec<Symbol> {
    let mut out = Vec::new();
    let root = file.tree.root_node();
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        collect(child, &file.source, &file.rel, Export::No, &mut out);
    }
    out
}

pub fn enclosing_symbol(file: &ParsedFile, line: u32) -> Option<String> {
    extract(file).into_iter().find(|s| s.start_line <= line && line <= s.end_line).map(|s| s.name)
}

/// Replaces the stored symbols for `file.rel` atomically. The delete and every
/// insert share one transaction, so a failure part way through the loop rolls
/// the whole replacement back rather than leaving a partial symbol set that the
/// content-hash check would consider up to date and never repair.
pub fn store(index: &mut Index, file: &ParsedFile, syms: &[Symbol]) -> anyhow::Result<()> {
    let conn = index.conn();
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM symbols WHERE rel = ?1", params![file.rel])?;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO symbols(rel, kind, name, export_name, start_line, start_col, end_line, end_col, exported)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )?;
        for s in syms {
            stmt.execute(params![
                s.rel,
                s.kind,
                s.name,
                s.export_name,
                s.start_line,
                s.start_col,
                s.end_line,
                s.end_col,
                s.exported as i64
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Every exported symbol in the index, in a reproducible order.
pub fn exported(index: &Index) -> anyhow::Result<Vec<Symbol>> {
    let mut stmt = index.conn().prepare(
        "SELECT rel, kind, name, export_name, start_line, start_col, end_line, end_col
         FROM symbols WHERE export_name IS NOT NULL ORDER BY rel, start_line, start_col",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(Symbol {
            rel: r.get(0)?,
            kind: r.get(1)?,
            name: r.get(2)?,
            export_name: r.get(3)?,
            start_line: r.get(4)?,
            start_col: r.get(5)?,
            end_line: r.get(6)?,
            end_col: r.get(7)?,
            exported: true,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p locrin-core`
Expected: all pass. If `anonymous_default_exports_are_default_symbols` fails on the function case only, print `p.tree.root_node().to_sexp()` for it: the grammar may put the anonymous function under `declaration` (handled by the `out.len() == before` branch) or under `value` (handled by the value branch); either way the fix is in `collect_export`, not the test.

- [ ] **Step 6: Commit**

```bash
cargo fmt --all
git add crates/core/src/tree.rs crates/core/src/symbols.rs crates/core/src/lib.rs
git commit -m "engine: export names on symbols, including clauses, defaults, and re-exports"
```

### Task 3: Import extraction

**Files:**
- Create: `crates/core/src/imports.rs`
- Modify: `crates/core/src/lib.rs`

**Interfaces:**
- Consumes: `tree::{text, line, has_keyword}`, `parse::ParsedFile`.
- Produces:
  ```rust
  pub enum ImportKind { Import, Reexport, Dynamic, Require }   // as_str: "import" | "reexport" | "dynamic" | "require"
  pub struct Binding { pub local: String, pub imported: String, pub line: u32, pub type_only: bool }
  pub struct Import { pub specifier: String, pub kind: ImportKind, pub line: u32, pub names: Vec<String>, pub bindings: Vec<Binding> }
  pub fn extract(file: &ParsedFile) -> Vec<Import>
  ```
  `names` are what is taken from the target: `"default"`, `"*"` (whole module: namespace import, star re-export, dynamic import, require), or the exported names. Empty for a side-effect import. `bindings` are the local names an `import` statement introduces (empty for re-exports and calls).

- [ ] **Step 1: Write the module with its failing tests**

```rust
//! Import extraction: static imports, re-exports, `import()` with a literal, and
//! `require()` with a literal. Computed specifiers are skipped, never guessed
//! (spec 3.2); a file that builds its paths at runtime is a known blind spot.

use tree_sitter::Node;

use crate::parse::ParsedFile;
use crate::tree::{has_keyword, line, text};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportKind {
    Import,
    Reexport,
    Dynamic,
    Require,
}

impl ImportKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ImportKind::Import => "import",
            ImportKind::Reexport => "reexport",
            ImportKind::Dynamic => "dynamic",
            ImportKind::Require => "require",
        }
    }
}

/// One local name an import statement brings into scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub local: String,
    pub imported: String,
    pub line: u32,
    pub type_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    pub specifier: String,
    pub kind: ImportKind,
    pub line: u32,
    /// Names taken from the target module: `default`, `*` for the whole module,
    /// or the exported names. Empty for a side-effect import.
    pub names: Vec<String>,
    pub bindings: Vec<Binding>,
}

fn unquote(s: &str) -> String {
    s.trim_matches(|c| c == '"' || c == '\'').to_string()
}

/// The value of a plain string literal, or None for anything with escapes or
/// interpolation: a path the engine cannot read off the source is not a path
/// it should record.
fn literal(node: Node, src: &str) -> Option<String> {
    if node.kind() != "string" {
        return None;
    }
    let mut cursor = node.walk();
    let parts: Vec<Node> = node.named_children(&mut cursor).collect();
    match parts.as_slice() {
        [] => Some(String::new()),
        [f] if f.kind() == "string_fragment" => Some(text(*f, src).to_string()),
        _ => None,
    }
}

fn import_statement(node: Node, src: &str) -> Option<Import> {
    let mut cursor = node.walk();
    // `import x = require("y")` carries its source inside the clause.
    if let Some(req) = node.children(&mut cursor).find(|c| c.kind() == "import_require_clause") {
        let specifier = literal(req.child_by_field_name("source")?, src)?;
        let local = text(req.named_child(0)?, src).to_string();
        return Some(Import {
            specifier,
            kind: ImportKind::Require,
            line: line(node),
            names: vec!["*".into()],
            bindings: vec![Binding { local, imported: "*".into(), line: line(node), type_only: false }],
        });
    }
    let specifier = literal(node.child_by_field_name("source")?, src)?;
    let statement_type_only = has_keyword(node, "type");
    let mut names = Vec::new();
    let mut bindings = Vec::new();
    let mut cursor = node.walk();
    if let Some(clause) = node.children(&mut cursor).find(|c| c.kind() == "import_clause") {
        let mut inner = clause.walk();
        for part in clause.named_children(&mut inner) {
            match part.kind() {
                "identifier" => {
                    names.push("default".into());
                    bindings.push(Binding {
                        local: text(part, src).into(),
                        imported: "default".into(),
                        line: line(part),
                        type_only: statement_type_only,
                    });
                }
                "namespace_import" => {
                    let Some(id) = part.named_child(0) else { continue };
                    names.push("*".into());
                    bindings.push(Binding {
                        local: text(id, src).into(),
                        imported: "*".into(),
                        line: line(part),
                        type_only: statement_type_only,
                    });
                }
                "named_imports" => {
                    let mut specs = part.walk();
                    for spec in part.named_children(&mut specs) {
                        if spec.kind() != "import_specifier" {
                            continue;
                        }
                        let Some(name) = spec.child_by_field_name("name") else { continue };
                        let imported = unquote(text(name, src));
                        let local = spec
                            .child_by_field_name("alias")
                            .map(|a| text(a, src).to_string())
                            .unwrap_or_else(|| imported.clone());
                        names.push(imported.clone());
                        bindings.push(Binding {
                            local,
                            imported,
                            line: line(spec),
                            type_only: statement_type_only || has_keyword(spec, "type"),
                        });
                    }
                }
                _ => {}
            }
        }
    }
    Some(Import { specifier, kind: ImportKind::Import, line: line(node), names, bindings })
}

fn export_statement(node: Node, src: &str) -> Option<Import> {
    let specifier = literal(node.child_by_field_name("source")?, src)?;
    let mut names = Vec::new();
    let mut saw_clause = false;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "export_clause" => {
                saw_clause = true;
                let mut specs = child.walk();
                for spec in child.named_children(&mut specs) {
                    if spec.kind() != "export_specifier" {
                        continue;
                    }
                    if let Some(name) = spec.child_by_field_name("name") {
                        names.push(unquote(text(name, src)));
                    }
                }
            }
            "namespace_export" => {
                saw_clause = true;
                names.push("*".into());
            }
            _ => {}
        }
    }
    if !saw_clause {
        names.push("*".into()); // `export * from "./x"`
    }
    Some(Import { specifier, kind: ImportKind::Reexport, line: line(node), names, bindings: vec![] })
}

fn call(node: Node, src: &str) -> Option<Import> {
    let func = node.child_by_field_name("function")?;
    let kind = match func.kind() {
        "import" => ImportKind::Dynamic,
        "identifier" if text(func, src) == "require" => ImportKind::Require,
        _ => return None,
    };
    let args = node.child_by_field_name("arguments")?;
    let specifier = literal(args.named_child(0)?, src)?;
    Some(Import { specifier, kind, line: line(node), names: vec!["*".into()], bindings: vec![] })
}

fn walk(node: Node, src: &str, out: &mut Vec<Import>) {
    let found = match node.kind() {
        "import_statement" => import_statement(node, src),
        "export_statement" => export_statement(node, src),
        "call_expression" => call(node, src),
        _ => None,
    };
    if let Some(i) = found {
        out.push(i);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, src, out);
    }
}

pub fn extract(file: &ParsedFile) -> Vec<Import> {
    let mut out = Vec::new();
    walk(file.tree.root_node(), &file.source, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse_source;
    use std::path::Path;

    const SRC: &str = r#"import React, { useState, type Foo as F } from "react";
import type { Bar } from "./bar";
import * as ns from "./ns";
import "./side";
import x = require("./req");
const y = require("./r2");
const z = await import("./dyn");
const w = require(name);
const v = import(`./${name}`);
export { d } from "./d";
export * from "./e";
export * as f from "./f";
export const g = 1;
"#;

    fn parsed() -> ParsedFile {
        parse_source(Path::new("src/a.ts"), "src/a.ts", SRC.to_string()).unwrap()
    }

    #[test]
    fn extracts_every_import_form_and_skips_computed_paths() {
        let imports = extract(&parsed());
        let view: Vec<(String, &str, Vec<String>)> =
            imports.iter().map(|i| (i.specifier.clone(), i.kind.as_str(), i.names.clone())).collect();
        assert_eq!(
            view,
            vec![
                ("react".into(), "import", vec!["default".into(), "useState".into(), "Foo".into()]),
                ("./bar".into(), "import", vec!["Bar".into()]),
                ("./ns".into(), "import", vec!["*".into()]),
                ("./side".into(), "import", vec![]),
                ("./req".into(), "require", vec!["*".into()]),
                ("./r2".into(), "require", vec!["*".into()]),
                ("./dyn".into(), "dynamic", vec!["*".into()]),
                ("./d".into(), "reexport", vec!["d".into()]),
                ("./e".into(), "reexport", vec!["*".into()]),
                ("./f".into(), "reexport", vec!["*".into()]),
            ]
        );
    }

    #[test]
    fn bindings_carry_local_names_lines_and_type_flags() {
        let imports = extract(&parsed());
        let react: Vec<(&str, &str, u32, bool)> =
            imports[0].bindings.iter().map(|b| (b.local.as_str(), b.imported.as_str(), b.line, b.type_only)).collect();
        assert_eq!(react, vec![("React", "default", 1, false), ("useState", "useState", 1, false), ("F", "Foo", 1, true)]);
        assert!(imports[1].bindings[0].type_only, "import type marks every binding");
        assert_eq!(imports[2].bindings[0].local, "ns");
        assert!(imports[3].bindings.is_empty(), "a side-effect import binds nothing");
        assert_eq!(imports[4].bindings[0].local, "x");
        assert!(imports[7].bindings.is_empty(), "a re-export binds nothing");
    }

    #[test]
    fn lines_point_at_the_statement() {
        let imports = extract(&parsed());
        assert_eq!(imports.iter().map(|i| i.line).collect::<Vec<_>>(), vec![1, 2, 3, 4, 5, 6, 7, 10, 11, 12]);
    }
}
```

Add `pub mod imports;` to `lib.rs`.

- [ ] **Step 2: Run the tests**

Run: `cargo test -p locrin-core imports::tests`
Expected: PASS. If the first test fails on the `react` entry only, the grammar has placed the inline `type` modifier somewhere unexpected; print the sexp of line 1 and adjust `has_keyword(spec, "type")` to look at that node. Do not change the expected vector.

- [ ] **Step 3: Commit**

```bash
cargo fmt --all
git add crates/core/src/imports.rs crates/core/src/lib.rs
git commit -m "engine: import extraction for static, re-export, dynamic, and require forms"
```

### Task 4: Project files: tsconfig, package.json, JSONC

**Files:**
- Create: `crates/core/src/project.rs`
- Modify: `crates/core/src/lib.rs`

**Interfaces:**
- Produces:
  ```rust
  pub fn strip_jsonc(text: &str) -> String
  pub fn parent_dir(rel: &str) -> String          // "src/a.ts" -> "src", "a.ts" -> ""
  pub fn join(dir: &str, tail: &str) -> String    // "" + "x" -> "x"; "src" + "../y" -> "src/../y"
  pub fn normalize(path: &str) -> Option<String>  // resolves "." and ".."; None when ".." climbs above the root
  pub struct TsConfig { pub base_url: Option<String>, pub paths_dir: String, pub paths: Vec<(String, Vec<String>)> }
  impl TsConfig { pub fn load(root: &Path) -> TsConfig; pub fn paths_base(&self) -> &str }
  pub struct PackageJson { pub name: Option<String>, pub main: Option<String>, pub bin: Vec<String>, pub exports: Vec<String>, pub workspaces: Vec<String> }
  impl PackageJson { pub fn load(dir: &Path) -> Option<PackageJson>; pub fn entry_files(&self, dir_rel: &str) -> Vec<String> }
  pub fn workspace_packages(root: &Path, workspaces: &[String]) -> Vec<(String, String)>   // (name, repo-relative dir)
  ```
  All paths are `/`-separated and repo-relative. `TsConfig::load` follows relative `extends` up to five levels and `extends` naming a package via `node_modules/<name>` (with `.json` appended when missing); a child file's `baseUrl` and `paths` override the parent's. `paths_base` is `baseUrl` when set anywhere in the chain, else the directory of the file that declared `paths` (TypeScript 4.1 rule).

- [ ] **Step 1: Write the module with its failing tests**

```rust
//! The two project files resolution reads: `tsconfig.json` (aliases) and
//! `package.json` (entry points, workspaces). Both are read leniently: a file
//! that is missing or unparsable simply contributes nothing.

use std::path::Path;

use serde_json::Value;

/// Removes `//` and `/* */` comments and trailing commas outside strings, so a
/// tsconfig.json (which allows both) parses as JSON.
pub fn strip_jsonc(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let mut in_string = false;
    while i < chars.len() {
        let c = chars[i];
        if in_string {
            out.push(c);
            if c == '\\' && i + 1 < chars.len() {
                out.push(chars[i + 1]);
                i += 2;
                continue;
            }
            if c == '"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        match c {
            '"' => {
                in_string = true;
                out.push(c);
                i += 1;
            }
            '/' if chars.get(i + 1) == Some(&'/') => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '/' if chars.get(i + 1) == Some(&'*') => {
                i += 2;
                while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                    i += 1;
                }
                i += 2;
            }
            ',' => {
                let mut j = i + 1;
                while j < chars.len() && chars[j].is_whitespace() {
                    j += 1;
                }
                if !matches!(chars.get(j), Some('}') | Some(']')) {
                    out.push(c);
                }
                i += 1;
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

pub fn parent_dir(rel: &str) -> String {
    match rel.rfind('/') {
        Some(i) => rel[..i].to_string(),
        None => String::new(),
    }
}

pub fn join(dir: &str, tail: &str) -> String {
    if dir.is_empty() {
        tail.to_string()
    } else {
        format!("{dir}/{tail}")
    }
}

/// Collapses `.` and `..` segments and a leading `./`. None when `..` would climb
/// above the repository root, which no import from inside it may do.
pub fn normalize(path: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    Some(parts.join("/"))
}

fn with_json_ext(p: &str) -> String {
    if p.ends_with(".json") {
        p.to_string()
    } else {
        format!("{p}.json")
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TsConfig {
    /// `compilerOptions.baseUrl`, repo-relative, from whichever file in the extends chain set it.
    pub base_url: Option<String>,
    /// Directory of the file that declared `paths`, repo-relative.
    pub paths_dir: String,
    /// `compilerOptions.paths` in declaration order: pattern with at most one `*`, and its targets.
    pub paths: Vec<(String, Vec<String>)>,
}

impl TsConfig {
    pub fn load(root: &Path) -> TsConfig {
        Self::load_file(root, "tsconfig.json", 0).unwrap_or_default()
    }

    /// The directory `paths` targets are relative to.
    pub fn paths_base(&self) -> &str {
        self.base_url.as_deref().unwrap_or(&self.paths_dir)
    }

    fn load_file(root: &Path, rel: &str, depth: usize) -> Option<TsConfig> {
        if depth > 5 {
            return None;
        }
        let text = std::fs::read_to_string(root.join(rel)).ok()?;
        let value: Value = serde_json::from_str(&strip_jsonc(&text)).ok()?;
        let dir = parent_dir(rel);
        let mut cfg = value
            .get("extends")
            .and_then(Value::as_str)
            .map(|e| {
                if e.starts_with('.') {
                    normalize(&join(&dir, e)).unwrap_or_default()
                } else {
                    format!("node_modules/{e}")
                }
            })
            .and_then(|p| Self::load_file(root, &with_json_ext(&p), depth + 1))
            .unwrap_or_default();
        let opts = value.get("compilerOptions");
        if let Some(base) = opts.and_then(|o| o.get("baseUrl")).and_then(Value::as_str) {
            cfg.base_url = Some(normalize(&join(&dir, base)).unwrap_or_default());
        }
        if let Some(paths) = opts.and_then(|o| o.get("paths")).and_then(Value::as_object) {
            cfg.paths_dir = dir;
            cfg.paths = paths
                .iter()
                .map(|(k, v)| {
                    let targets = v
                        .as_array()
                        .map(|a| a.iter().filter_map(Value::as_str).map(String::from).collect())
                        .unwrap_or_default();
                    (k.clone(), targets)
                })
                .collect();
        }
        Some(cfg)
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PackageJson {
    pub name: Option<String>,
    pub main: Option<String>,
    pub bin: Vec<String>,
    /// Every string leaf under `exports`, whatever the nesting of conditions and subpaths.
    pub exports: Vec<String>,
    pub workspaces: Vec<String>,
}

fn collect_strings(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) => out.push(s.clone()),
        Value::Object(o) => o.values().for_each(|x| collect_strings(x, out)),
        Value::Array(a) => a.iter().for_each(|x| collect_strings(x, out)),
        _ => {}
    }
}

fn string_list(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(String::from).collect())
        .unwrap_or_default()
}

impl PackageJson {
    pub fn load(dir: &Path) -> Option<PackageJson> {
        let text = std::fs::read_to_string(dir.join("package.json")).ok()?;
        let v: Value = serde_json::from_str(&text).ok()?;
        let mut p = PackageJson {
            name: v.get("name").and_then(Value::as_str).map(String::from),
            main: v.get("main").and_then(Value::as_str).map(String::from),
            ..PackageJson::default()
        };
        match v.get("bin") {
            Some(Value::String(s)) => p.bin.push(s.clone()),
            Some(Value::Object(o)) => p.bin.extend(o.values().filter_map(Value::as_str).map(String::from)),
            _ => {}
        }
        if let Some(e) = v.get("exports") {
            collect_strings(e, &mut p.exports);
        }
        p.workspaces = match v.get("workspaces") {
            Some(Value::Object(o)) => string_list(o.get("packages")),
            other => string_list(other),
        };
        Some(p)
    }

    /// Every file this package points at, as repo-relative paths under `dir_rel`.
    /// Only `exports` leaves that start with `./` are files; the rest are conditions.
    pub fn entry_files(&self, dir_rel: &str) -> Vec<String> {
        self.main
            .iter()
            .chain(self.bin.iter())
            .chain(self.exports.iter().filter(|e| e.starts_with("./")))
            .filter_map(|p| normalize(&join(dir_rel, p)))
            .collect()
    }
}

/// Workspace packages as (name, repo-relative dir). Globs of the form `dir/*` are
/// expanded one level and plain directories are taken as they are; anything
/// fancier is ignored, which covers `packages/*` and `apps/*` in practice.
pub fn workspace_packages(root: &Path, workspaces: &[String]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for pattern in workspaces {
        let pattern = pattern.trim_end_matches('/');
        let dirs: Vec<String> = if let Some(prefix) = pattern.strip_suffix("/*") {
            std::fs::read_dir(root.join(prefix))
                .map(|rd| {
                    rd.filter_map(Result::ok)
                        .filter(|e| e.path().is_dir())
                        .map(|e| join(prefix, &e.file_name().to_string_lossy()))
                        .collect()
                })
                .unwrap_or_default()
        } else {
            vec![pattern.to_string()]
        };
        for dir in dirs {
            if let Some(name) = PackageJson::load(&root.join(&dir)).and_then(|p| p.name) {
                out.push((name, dir));
            }
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct Cleanup(PathBuf);

    impl Drop for Cleanup {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).ok();
        }
    }

    fn fresh(tag: &str) -> Cleanup {
        let dir = std::env::temp_dir().join(format!("locrin-project-{tag}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        Cleanup(dir)
    }

    fn write(c: &Cleanup, rel: &str, text: &str) {
        let p = c.0.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    #[test]
    fn strips_comments_and_trailing_commas_but_not_strings() {
        let src = "{\n  // line\n  \"a\": \"http://x/*y*/\", /* block */\n  \"b\": [1, 2,],\n}\n";
        let v: Value = serde_json::from_str(&strip_jsonc(src)).unwrap();
        assert_eq!(v["a"], "http://x/*y*/");
        assert_eq!(v["b"], serde_json::json!([1, 2]));
    }

    #[test]
    fn path_helpers() {
        assert_eq!(parent_dir("src/a/b.ts"), "src/a");
        assert_eq!(parent_dir("b.ts"), "");
        assert_eq!(join("", "x.ts"), "x.ts");
        assert_eq!(join("src", "./x.ts"), "src/./x.ts");
        assert_eq!(normalize("src/./a/../b.ts").as_deref(), Some("src/b.ts"));
        assert_eq!(normalize("./x").as_deref(), Some("x"));
        assert_eq!(normalize("../x"), None);
    }

    #[test]
    fn tsconfig_follows_extends_and_paths_base_rules() {
        let dir = fresh("tsconfig");
        write(&dir, "config/base.json", "{ \"compilerOptions\": { \"baseUrl\": \"..\" } }");
        write(
            &dir,
            "tsconfig.json",
            "{\n  \"extends\": \"./config/base.json\",\n  \"compilerOptions\": {\n    // alias\n    \"paths\": { \"@/*\": [\"src/*\"], \"lib\": [\"src/lib/index.ts\"], },\n  },\n}\n",
        );
        let cfg = TsConfig::load(&dir.0);
        assert_eq!(cfg.base_url.as_deref(), Some(""));
        assert_eq!(cfg.paths_base(), "");
        assert_eq!(
            cfg.paths,
            vec![("@/*".into(), vec!["src/*".into()]), ("lib".into(), vec!["src/lib/index.ts".into()])]
        );

        // Without a baseUrl anywhere, paths are relative to the declaring file's directory.
        let dir = fresh("tsconfig2");
        write(&dir, "tsconfig.json", "{ \"compilerOptions\": { \"paths\": { \"~/*\": [\"./app/*\"] } } }");
        assert_eq!(TsConfig::load(&dir.0).paths_base(), "");

        let dir = fresh("tsconfig3");
        assert_eq!(TsConfig::load(&dir.0), TsConfig::default(), "no file means no aliases");
    }

    #[test]
    fn package_json_entry_files_and_workspaces() {
        let dir = fresh("pkg");
        write(
            &dir,
            "package.json",
            r#"{ "name": "root", "main": "src/index.ts", "bin": { "cli": "./bin/run.js" },
                "exports": { ".": { "import": "./src/index.ts", "types": "./dist/index.d.ts" }, "./sub": "./src/sub.ts" },
                "workspaces": ["packages/*", "tools"] }"#,
        );
        write(&dir, "packages/a/package.json", "{ \"name\": \"@x/a\", \"main\": \"index.ts\" }");
        write(&dir, "packages/b/package.json", "{ \"name\": \"@x/b\" }");
        write(&dir, "tools/package.json", "{ \"name\": \"tools\" }");
        let p = PackageJson::load(&dir.0).unwrap();
        assert_eq!(
            p.entry_files(""),
            vec!["src/index.ts", "bin/run.js", "src/index.ts", "dist/index.d.ts", "src/sub.ts"]
        );
        assert_eq!(
            workspace_packages(&dir.0, &p.workspaces),
            vec![
                ("@x/a".to_string(), "packages/a".to_string()),
                ("@x/b".to_string(), "packages/b".to_string()),
                ("tools".to_string(), "tools".to_string())
            ]
        );
        assert!(PackageJson::load(&dir.0.join("nowhere")).is_none());
    }
}
```

Add `pub mod project;` to `lib.rs`.

- [ ] **Step 2: Run the tests**

Run: `cargo test -p locrin-core project::tests`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
cargo fmt --all
git add crates/core/src/project.rs crates/core/src/lib.rs
git commit -m "engine: read tsconfig aliases and package.json entry points and workspaces"
```

### Task 5: Module resolver

**Files:**
- Create: `crates/core/src/resolve.rs`
- Modify: `crates/core/src/lib.rs`

**Interfaces:**
- Consumes: `project::{TsConfig, PackageJson, workspace_packages, join, normalize, parent_dir}`.
- Produces:
  ```rust
  pub enum Resolution { Resolved(String), External, Unresolved }   // as_str: "resolved" | "external" | "unresolved"
  pub struct Resolver { .. }
  impl Resolver {
      pub fn new(root: &Path, indexed: HashSet<String>) -> Resolver
      pub fn resolve(&self, from_rel: &str, specifier: &str) -> Resolution
  }
  ```
  `indexed` is the set of repo-relative source files this run walks (plus any explicitly named). `root` must be the canonical repository root.

- [ ] **Step 1: Write the module with its failing tests**

```rust
//! Heuristic module resolution (spec 3.2). Relative specifiers, tsconfig `paths`
//! and `baseUrl`, workspace packages, and the extension probing TypeScript does.
//! Never guesses: what it cannot find on disk is `Unresolved`, what it finds on
//! disk but does not index is `External`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::project::{join, normalize, parent_dir, workspace_packages, PackageJson, TsConfig};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// A repo-relative path of an indexed source file.
    Resolved(String),
    /// A package, a declaration file, an asset, or an excluded file: real, but not a node in this graph.
    External,
    /// Nothing on disk matches. Recorded as such, never guessed.
    Unresolved,
}

impl Resolution {
    pub fn as_str(&self) -> &'static str {
        match self {
            Resolution::Resolved(_) => "resolved",
            Resolution::External => "external",
            Resolution::Unresolved => "unresolved",
        }
    }
}

const SOURCE_EXTS: &[&str] = &["ts", "tsx", "js", "jsx", "mts", "cts", "mjs", "cjs"];

/// TypeScript lets a specifier carry the *output* extension of a source file.
const OUTPUT_TO_SOURCE: &[(&str, &str)] =
    &[(".js", ".ts"), (".js", ".tsx"), (".jsx", ".tsx"), (".mjs", ".mts"), (".cjs", ".cts")];

pub struct Resolver {
    root: PathBuf,
    indexed: HashSet<String>,
    tsconfig: TsConfig,
    packages: Vec<(String, String)>,
}

/// Matches a tsconfig `paths` pattern (at most one `*`) and returns the text the star stood for.
fn match_pattern(pattern: &str, specifier: &str) -> Option<String> {
    match pattern.split_once('*') {
        Some((prefix, suffix)) => {
            if specifier.len() >= prefix.len() + suffix.len()
                && specifier.starts_with(prefix)
                && specifier.ends_with(suffix)
            {
                Some(specifier[prefix.len()..specifier.len() - suffix.len()].to_string())
            } else {
                None
            }
        }
        None => (pattern == specifier).then(String::new),
    }
}

impl Resolver {
    pub fn new(root: &Path, indexed: HashSet<String>) -> Resolver {
        let tsconfig = TsConfig::load(root);
        let packages = PackageJson::load(root).map(|p| workspace_packages(root, &p.workspaces)).unwrap_or_default();
        Resolver { root: root.to_path_buf(), indexed, tsconfig, packages }
    }

    pub fn resolve(&self, from_rel: &str, specifier: &str) -> Resolution {
        if specifier.starts_with('.') {
            return match normalize(&join(&parent_dir(from_rel), specifier)) {
                Some(p) => self.probe(&p),
                None => Resolution::Unresolved,
            };
        }
        if specifier.starts_with('/') {
            // An absolute path names a machine, not a repository.
            return Resolution::Unresolved;
        }
        let mut aliased: Option<Resolution> = None;
        for (pattern, targets) in &self.tsconfig.paths {
            let Some(rest) = match_pattern(pattern, specifier) else { continue };
            for target in targets {
                let candidate = target.replacen('*', &rest, 1);
                let Some(p) = normalize(&join(self.tsconfig.paths_base(), &candidate)) else { continue };
                match self.probe(&p) {
                    Resolution::Resolved(r) => return Resolution::Resolved(r),
                    Resolution::External => aliased = Some(Resolution::External),
                    Resolution::Unresolved => {
                        aliased.get_or_insert(Resolution::Unresolved);
                    }
                }
            }
        }
        if let Some(r) = aliased {
            // An alias matched but no target is an indexed file: the alias points
            // at a declaration or an asset (External) or at nothing (Unresolved).
            return r;
        }
        for (name, dir) in &self.packages {
            if specifier == name {
                let main =
                    PackageJson::load(&self.root.join(dir)).and_then(|p| p.main).unwrap_or_else(|| "index".into());
                return normalize(&join(dir, &main)).map(|p| self.probe(&p)).unwrap_or(Resolution::Unresolved);
            }
            if let Some(sub) = specifier.strip_prefix(&format!("{name}/")) {
                return normalize(&join(dir, sub)).map(|p| self.probe(&p)).unwrap_or(Resolution::Unresolved);
            }
        }
        // A bare specifier that no alias or workspace owns is a package. It cannot
        // name a file in this repository, so External is a fact, not a guess.
        Resolution::External
    }

    fn probe(&self, p: &str) -> Resolution {
        let mut candidates = vec![p.to_string()];
        for (output, source) in OUTPUT_TO_SOURCE {
            if let Some(stem) = p.strip_suffix(output) {
                candidates.push(format!("{stem}{source}"));
            }
        }
        for ext in SOURCE_EXTS {
            candidates.push(format!("{p}.{ext}"));
        }
        for ext in SOURCE_EXTS {
            candidates.push(format!("{p}/index.{ext}"));
        }
        if let Some(hit) = candidates.iter().find(|c| self.indexed.contains(c.as_str())) {
            return Resolution::Resolved(hit.clone());
        }
        let on_disk = self.root.join(p);
        let exists = on_disk.is_file()
            || on_disk.join("package.json").is_file()
            || on_disk.join("index.d.ts").is_file()
            || self.root.join(format!("{p}.d.ts")).is_file()
            || SOURCE_EXTS.iter().any(|e| self.root.join(format!("{p}.{e}")).is_file());
        if exists {
            Resolution::External
        } else {
            Resolution::Unresolved
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Cleanup(PathBuf);

    impl Drop for Cleanup {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).ok();
        }
    }

    fn fresh(tag: &str) -> Cleanup {
        let dir = std::env::temp_dir().join(format!("locrin-resolve-{tag}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        Cleanup(dir)
    }

    fn write(c: &Cleanup, rel: &str, text: &str) {
        let p = c.0.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn resolver(c: &Cleanup, indexed: &[&str]) -> Resolver {
        Resolver::new(&c.0, indexed.iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn relative_specifiers_probe_extensions_and_index_files() {
        let dir = fresh("relative");
        write(&dir, "src/a.ts", "");
        write(&dir, "src/b.tsx", "");
        write(&dir, "src/c/index.ts", "");
        write(&dir, "src/gen/x.ts", "");
        write(&dir, "src/types.d.ts", "");
        write(&dir, "src/logo.png", "");
        let r = resolver(&dir, &["src/a.ts", "src/b.tsx", "src/c/index.ts", "src/main.ts"]);
        assert_eq!(r.resolve("src/main.ts", "./a"), Resolution::Resolved("src/a.ts".into()));
        assert_eq!(
            r.resolve("src/main.ts", "./a.js"),
            Resolution::Resolved("src/a.ts".into()),
            "output extension names the source"
        );
        assert_eq!(r.resolve("src/main.ts", "./b"), Resolution::Resolved("src/b.tsx".into()));
        assert_eq!(r.resolve("src/main.ts", "./c"), Resolution::Resolved("src/c/index.ts".into()));
        assert_eq!(r.resolve("src/c/index.ts", "../a"), Resolution::Resolved("src/a.ts".into()));
        assert_eq!(r.resolve("src/main.ts", "./gen/x"), Resolution::External, "on disk but not indexed (excluded)");
        assert_eq!(r.resolve("src/main.ts", "./types"), Resolution::External, "declaration file");
        assert_eq!(r.resolve("src/main.ts", "./logo.png"), Resolution::External, "asset");
        assert_eq!(r.resolve("src/main.ts", "./missing"), Resolution::Unresolved);
        assert_eq!(r.resolve("src/main.ts", "../../escape"), Resolution::Unresolved);
        assert_eq!(r.resolve("src/main.ts", "/etc/x"), Resolution::Unresolved);
    }

    #[test]
    fn aliases_workspaces_and_bare_specifiers() {
        let dir = fresh("alias");
        write(
            &dir,
            "tsconfig.json",
            "{ \"compilerOptions\": { \"baseUrl\": \".\", \"paths\": { \"@/*\": [\"src/*\"], \"types\": [\"src/types.d.ts\"] } } }",
        );
        write(&dir, "package.json", "{ \"name\": \"root\", \"workspaces\": [\"packages/*\"] }");
        write(&dir, "packages/ui/package.json", "{ \"name\": \"@acme/ui\", \"main\": \"src/index.ts\" }");
        write(&dir, "packages/ui/src/index.ts", "");
        write(&dir, "packages/ui/src/button.tsx", "");
        write(&dir, "src/util.ts", "");
        write(&dir, "src/types.d.ts", "");
        let r = resolver(&dir, &["src/util.ts", "packages/ui/src/index.ts", "packages/ui/src/button.tsx", "src/main.ts"]);
        assert_eq!(r.resolve("src/main.ts", "@/util"), Resolution::Resolved("src/util.ts".into()));
        assert_eq!(
            r.resolve("src/main.ts", "@/nope"),
            Resolution::Unresolved,
            "an alias that points at nothing is unresolved, not external"
        );
        assert_eq!(r.resolve("src/main.ts", "types"), Resolution::External);
        assert_eq!(r.resolve("src/main.ts", "@acme/ui"), Resolution::Resolved("packages/ui/src/index.ts".into()));
        assert_eq!(
            r.resolve("src/main.ts", "@acme/ui/src/button"),
            Resolution::Resolved("packages/ui/src/button.tsx".into())
        );
        assert_eq!(r.resolve("src/main.ts", "react"), Resolution::External);
        assert_eq!(r.resolve("src/main.ts", "node:fs"), Resolution::External);
    }

    #[test]
    fn pattern_matching() {
        assert_eq!(match_pattern("@/*", "@/a/b").as_deref(), Some("a/b"));
        assert_eq!(match_pattern("*", "x").as_deref(), Some("x"));
        assert_eq!(match_pattern("lib", "lib").as_deref(), Some(""));
        assert_eq!(match_pattern("lib", "lib/x"), None);
        assert_eq!(match_pattern("@/*", "src/x"), None);
    }
}
```

Add `pub mod resolve;` to `lib.rs`.

- [ ] **Step 2: Run the tests**

Run: `cargo test -p locrin-core resolve::tests`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
cargo fmt --all
git add crates/core/src/resolve.rs crates/core/src/lib.rs
git commit -m "engine: heuristic module resolver with aliases, workspaces, and extension probing"
```

### Task 6: Edges

**Files:**
- Create: `crates/core/src/edges.rs`
- Modify: `crates/core/src/lib.rs`

**Interfaces:**
- Consumes: `imports::Import`, `resolve::{Resolver, Resolution}`, `index::Index`.
- Produces:
  ```rust
  pub struct Edge { pub from_rel: String, pub to_rel: Option<String>, pub specifier: String, pub name: String, pub kind: String, pub resolution: String, pub line: u32 }
  pub fn from_imports(from_rel: &str, imports: &[Import], resolver: &Resolver) -> Vec<Edge>
  pub fn store(ix: &mut Index, from_rel: &str, edges: &[Edge]) -> Result<()>      // replace all edges from one file, one transaction
  pub fn from_file(ix: &Index, rel: &str) -> Result<Vec<Edge>>                    // ordered by line, id
  pub fn resolved(ix: &Index) -> Result<Vec<Edge>>                                // every resolved edge, ordered by from_rel, line, id
  pub fn dependents(ix: &Index, rel: &str) -> Result<Vec<String>>                 // distinct from_rel of resolved edges into rel, sorted
  ```
  One edge per imported name; a side-effect import yields one edge with `name == ""` so the target still counts as imported.

- [ ] **Step 1: Write the module with its failing tests**

```rust
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

/// Replaces every edge leaving `from_rel` in one transaction, for the same reason
/// `symbols::store` does: a half-written edge set looks complete to the hash check.
pub fn store(ix: &mut Index, from_rel: &str, edges: &[Edge]) -> anyhow::Result<()> {
    let tx = ix.conn().unchecked_transaction()?;
    tx.execute("DELETE FROM edges WHERE from_rel = ?1", params![from_rel])?;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO edges(from_rel, to_rel, specifier, name, kind, resolution, line)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )?;
        for e in edges {
            stmt.execute(params![e.from_rel, e.to_rel, e.specifier, e.name, e.kind, e.resolution, e.line])?;
        }
    }
    tx.commit()?;
    Ok(())
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
    let mut stmt = ix
        .conn()
        .prepare("SELECT DISTINCT from_rel FROM edges WHERE to_rel = ?1 AND resolution = 'resolved' ORDER BY from_rel")?;
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
        assert_eq!((edges[0].name.as_str(), edges[0].kind.as_str(), edges[0].resolution.as_str(), edges[0].line), ("helper", "import", "resolved", 1));

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
```

Add `pub mod edges;` to `lib.rs`.

- [ ] **Step 2: Run the tests**

Run: `cargo test -p locrin-core edges::tests`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
cargo fmt --all
git add crates/core/src/edges.rs crates/core/src/lib.rs
git commit -m "engine: import edges stored per file with resolution status"
```

### Task 7: Entry points

**Files:**
- Create: `crates/core/src/entry.rs`
- Modify: `crates/core/src/lib.rs`

**Interfaces:**
- Consumes: `project::{PackageJson, workspace_packages}`, globset.
- Produces:
  ```rust
  pub const DEFAULT_ENTRY_GLOBS: &[&str]
  pub struct EntryPoints { .. }
  impl EntryPoints {
      pub fn detect(root: &Path, extra: &[String]) -> anyhow::Result<EntryPoints>
      pub fn is_entry(&self, rel: &str) -> bool
  }
  ```
  A file is an entry point when it matches a default glob, an `extra` glob from the config, or (extension aside) a `main`, `bin`, or `./`-prefixed `exports` leaf of the root `package.json` or of any workspace package. Entry points are exempt from `dead-export` and `dead-file`.

- [ ] **Step 1: Write the module with its failing tests**

```rust
//! Files a framework, a runner, or a package consumer loads by convention rather
//! than by an import statement. The graph cannot see those loads, so these files
//! and their exports are never reported dead (spec 4.1).

use std::collections::HashSet;
use std::path::Path;

use anyhow::Context;
use globset::{Glob, GlobSet, GlobSetBuilder};

use crate::project::{workspace_packages, PackageJson};

pub const DEFAULT_ENTRY_GLOBS: &[&str] = &[
    // Next.js and Expo Router load routes from these folders.
    "app/**",
    "pages/**",
    "src/app/**",
    "src/pages/**",
    // Conventional roots.
    "index.*",
    "src/index.*",
    "App.*",
    "src/App.*",
    "main.*",
    "src/main.*",
    // Test runners, storybook, mocks.
    "**/*.test.*",
    "**/*.spec.*",
    "**/__tests__/**",
    "**/__mocks__/**",
    "**/*.stories.*",
    // Tooling executes these directly.
    "**/*.config.*",
    "**/*.setup.*",
    "**/scripts/**",
    "**/bin/**",
];

pub struct EntryPoints {
    globs: GlobSet,
    /// Extension-less repo-relative paths named by package.json files.
    files: HashSet<String>,
}

/// `src/index.ts` and `src/index.js` are one entry as far as package.json is concerned.
fn strip_ext(rel: &str) -> String {
    match (rel.rfind('/'), rel.rfind('.')) {
        (Some(slash), Some(dot)) if dot > slash => rel[..dot].to_string(),
        (None, Some(dot)) => rel[..dot].to_string(),
        _ => rel.to_string(),
    }
}

impl EntryPoints {
    pub fn detect(root: &Path, extra: &[String]) -> anyhow::Result<EntryPoints> {
        let mut b = GlobSetBuilder::new();
        for g in DEFAULT_ENTRY_GLOBS.iter().map(|s| s.to_string()).chain(extra.iter().cloned()) {
            b.add(Glob::new(&g).with_context(|| format!("entry_points contains an invalid glob: {g}"))?);
        }
        let mut files = HashSet::new();
        if let Some(pkg) = PackageJson::load(root) {
            files.extend(pkg.entry_files("").iter().map(|f| strip_ext(f)));
            for (_, dir) in workspace_packages(root, &pkg.workspaces) {
                if let Some(p) = PackageJson::load(&root.join(&dir)) {
                    files.extend(p.entry_files(&dir).iter().map(|f| strip_ext(f)));
                }
            }
        }
        Ok(EntryPoints { globs: b.build()?, files })
    }

    pub fn is_entry(&self, rel: &str) -> bool {
        self.globs.is_match(rel) || self.files.contains(&strip_ext(rel))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn mini() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mini")
    }

    #[test]
    fn package_main_and_default_globs_are_entries() {
        let e = EntryPoints::detect(&mini(), &[]).unwrap();
        assert!(e.is_entry("src/index.ts"), "package.json main");
        assert!(e.is_entry("src/index.js"), "extension does not matter for package.json entries");
        assert!(e.is_entry("app/(tabs)/home.tsx"));
        assert!(e.is_entry("src/lib/a.test.ts"));
        assert!(e.is_entry("jest.config.ts"));
        assert!(e.is_entry("scripts/build.ts"));
        assert!(!e.is_entry("src/util.ts"));
        assert!(!e.is_entry("src/config.ts"), "config.ts is not *.config.*");
    }

    #[test]
    fn extra_globs_extend_and_bad_globs_fail_loudly() {
        let e = EntryPoints::detect(&mini(), &["tools/**".into()]).unwrap();
        assert!(e.is_entry("tools/run.ts"));
        let err = format!("{:#}", EntryPoints::detect(&mini(), &["tools/[".into()]).unwrap_err());
        assert!(err.contains("tools/["), "{err}");
    }

    #[test]
    fn strip_ext_handles_dots_in_directories() {
        assert_eq!(strip_ext("src/index.ts"), "src/index");
        assert_eq!(strip_ext("a.b/index"), "a.b/index");
        assert_eq!(strip_ext("index.ts"), "index");
    }
}
```

Add `pub mod entry;` to `lib.rs`.

- [ ] **Step 2: Run the tests**

Run: `cargo test -p locrin-core entry::tests`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
cargo fmt --all
git add crates/core/src/entry.rs crates/core/src/lib.rs
git commit -m "engine: entry point detection from package.json and framework conventions"
```

### Task 8: Config: `entry_points` and `boundaries`

**Files:**
- Modify: `crates/core/src/config.rs`

**Interfaces:**
- Produces:
  ```rust
  pub struct Boundary { pub name: Option<String>, pub from: String, pub forbid: Vec<String>, pub allow: Vec<String> }
  // Config gains: pub entry_points: Vec<String>, pub boundaries: Vec<Boundary>
  ```
  TOML shape:
  ```toml
  entry_points = ["tools/**"]

  [[boundaries]]
  name = "ui stays off the database"
  from = "src/ui/**"
  forbid = ["src/db/**"]

  [[boundaries]]
  from = "src/db/**"
  allow = ["src/shared/**"]
  ```
  A boundary with `forbid` flags an import from a `from` file into any `forbid` match. A boundary with `allow` flags an import from a `from` file into any resolved file that matches neither `allow` nor `from` itself. Exactly one of `forbid` or `allow` must be non-empty. All globs are validated at load.

- [ ] **Step 1: Write the failing tests** (append inside `mod tests`)

```rust
    #[test]
    fn parses_entry_points_and_boundaries() {
        let dir = fresh("config8");
        std::fs::write(
            path(&dir).join(CONFIG_FILE),
            "entry_points = [\"tools/**\"]\n\n[[boundaries]]\nname = \"ui stays off the database\"\nfrom = \"src/ui/**\"\nforbid = [\"src/db/**\"]\n\n[[boundaries]]\nfrom = \"src/db/**\"\nallow = [\"src/shared/**\"]\n",
        )
        .unwrap();
        let c = Config::load(path(&dir)).unwrap();
        assert_eq!(c.entry_points, vec!["tools/**"]);
        assert_eq!(c.boundaries.len(), 2);
        assert_eq!(c.boundaries[0].name.as_deref(), Some("ui stays off the database"));
        assert_eq!(c.boundaries[0].forbid, vec!["src/db/**"]);
        assert!(c.boundaries[1].name.is_none());
        assert_eq!(c.boundaries[1].allow, vec!["src/shared/**"]);
    }

    #[test]
    fn a_boundary_needs_exactly_one_of_forbid_or_allow() {
        for body in ["[[boundaries]]\nfrom = \"src/ui/**\"\n", "[[boundaries]]\nfrom = \"a/**\"\nforbid = [\"b/**\"]\nallow = [\"c/**\"]\n"] {
            let dir = fresh("config9");
            std::fs::write(path(&dir).join(CONFIG_FILE), body).unwrap();
            let err = format!("{:#}", Config::load(path(&dir)).unwrap_err());
            assert!(err.contains("boundaries"), "{err}");
            assert!(err.contains(CONFIG_FILE), "{err}");
        }
    }

    #[test]
    fn boundary_and_entry_globs_are_validated() {
        let dir = fresh("config10");
        std::fs::write(path(&dir).join(CONFIG_FILE), "[[boundaries]]\nfrom = \"src/[\"\nforbid = [\"x/**\"]\n").unwrap();
        let err = format!("{:#}", Config::load(path(&dir)).unwrap_err());
        assert!(err.contains("src/["), "{err}");

        let dir = fresh("config11");
        std::fs::write(path(&dir).join(CONFIG_FILE), "entry_points = [\"tools/[\"]\n").unwrap();
        let err = format!("{:#}", Config::load(path(&dir)).unwrap_err());
        assert!(err.contains("tools/["), "{err}");
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p locrin-core config::tests`
Expected: compile errors, no field `entry_points`, `boundaries`.

- [ ] **Step 3: Implement**

Add after `RuleOverride`:

```rust
/// One import direction the operator has ruled on (spec 7.5). `forbid` names
/// targets a `from` file may not import; `allow` names the only targets it may.
/// Exactly one of the two is set, so a boundary always reads one way.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Boundary {
    pub name: Option<String>,
    pub from: String,
    pub forbid: Vec<String>,
    pub allow: Vec<String>,
}
```

Add the two fields to `Config` (after `debug_allowed`) and to `Default`:

```rust
    pub entry_points: Vec<String>,
    pub boundaries: Vec<Boundary>,
```
```rust
            entry_points: vec![],
            boundaries: vec![],
```

Replace `validate_globs`:

```rust
    /// Checks that every configured glob compiles, naming the one that does not,
    /// and that every boundary reads one way.
    fn validate_globs(&self) -> anyhow::Result<()> {
        let lists: [(&str, &Vec<String>); 3] =
            [("excludes", &self.excludes), ("debug_allowed", &self.debug_allowed), ("entry_points", &self.entry_points)];
        for (field, globs) in lists {
            for g in globs {
                globset::Glob::new(g).with_context(|| format!("{field} contains an invalid glob: {g}"))?;
            }
        }
        for (i, b) in self.boundaries.iter().enumerate() {
            let label = b.name.clone().unwrap_or_else(|| format!("#{}", i + 1));
            if b.forbid.is_empty() == b.allow.is_empty() {
                anyhow::bail!("boundaries entry {label} must set exactly one of forbid or allow");
            }
            for g in std::iter::once(&b.from).chain(b.forbid.iter()).chain(b.allow.iter()) {
                globset::Glob::new(g).with_context(|| format!("boundaries entry {label} contains an invalid glob: {g}"))?;
            }
        }
        Ok(())
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p locrin-core`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add crates/core/src/config.rs
git commit -m "engine: entry_points and boundaries config sections"
```

### Task 9: Indexer, rule scope, and the run pipeline

This is the seam task. After it, every existing test still passes, the three plan-1 rules declare a scope and a description, graph rules can be written against `RuleContext.index`, and the CLI always walks the whole repository so the resolver sees every file.

**Files:**
- Create: `crates/core/src/indexer.rs`
- Modify: `crates/core/src/lib.rs`, `crates/rules/src/lib.rs`, `crates/rules/src/leftover_debug.rs`, `crates/rules/src/leftover_commented.rs`, `crates/rules/src/leftover_marker.rs`, `crates/rules/tests/common/mod.rs`, `crates/rules/Cargo.toml`, `crates/cli/src/run.rs`

**Interfaces:**
- Consumes: everything from Tasks 1 to 8.
- Produces:
  ```rust
  // core
  pub const ALLOW_MARK: &str = "locrin:allow";                 // moved from rules
  pub fn indexer::record(ix: &mut Index, file: &ParsedFile, hash: &str, resolver: &Resolver) -> Result<()>
  // rules
  pub enum Scope { File, Graph }
  pub trait Rule {
      fn id(&self) -> &'static str;
      fn description(&self) -> &'static str;
      fn scope(&self) -> Scope;
      fn category(&self) -> Category;
      fn default_severity(&self) -> Severity;
      fn confidence(&self) -> Confidence;
      fn run(&self, ctx: &RuleContext) -> anyhow::Result<Vec<Finding>>;
  }
  pub struct RuleContext<'a> { pub files: &'a [ParsedFile], pub config: &'a Config, pub index: &'a Index, pub entries: &'a EntryPoints }
  pub fn line_span(file: &ParsedFile, line: u32) -> Span
  pub fn finding_at(rule: &dyn Rule, rel: &str, span: Span, anchor: &str, evidence: &str, fix: &str) -> Finding
  pub fn run_rules(rules: &[Box<dyn Rule>], ctx: &RuleContext) -> anyhow::Result<Vec<Finding>>
  pub fn run_all(ctx: &RuleContext) -> anyhow::Result<Vec<Finding>>
  pub fn file_rules() -> Vec<Box<dyn Rule>>; pub fn graph_rules() -> Vec<Box<dyn Rule>>
  ```
  `record` writes symbols, edges, and allow lines first and the `files` row last, so a failure part way leaves the old content hash in place and the file is redone next run rather than half-indexed and considered current.

- [ ] **Step 1: Write `indexer.rs` with its test**

```rust
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
    let allow: Vec<u32> = file
        .source
        .lines()
        .enumerate()
        .filter(|(_, l)| l.contains(ALLOW_MARK))
        .map(|(i, _)| i as u32 + 1)
        .collect();
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
        ix.conn().execute_batch("DROP TABLE symbols").unwrap();
        let mut ix2 = Index::open_in_memory().unwrap();
        ix2.conn().execute_batch("DROP TABLE symbols").unwrap();
        assert!(record(&mut ix2, &file, "h2", &resolver).is_err());
        assert_eq!(ix2.parse_status("src/index.ts").unwrap(), None, "a failed record must not look indexed");
    }
}
```

In `crates/core/src/lib.rs` add `pub mod indexer;` and, after `ENGINE_NAME`:

```rust
/// The in-source suppression marker. A finding whose line carries this text is
/// dropped by the rule runner, and the indexer records the lines that carry it so
/// suppression also works for findings on files the run did not parse.
pub const ALLOW_MARK: &str = "locrin:allow";
```

Run: `cargo test -p locrin-core indexer` and expect PASS.

- [ ] **Step 2: Add `anyhow` to the rules crate**

In `crates/rules/Cargo.toml` `[dependencies]` add `anyhow.workspace = true`.

- [ ] **Step 3: Rewrite `crates/rules/src/lib.rs` above `#[cfg(test)]`**

```rust
pub mod leftover_commented;
pub mod leftover_debug;
pub mod leftover_marker;

use std::collections::HashMap;

use locrin_core::config::Config;
use locrin_core::entry::EntryPoints;
use locrin_core::finding::{make_id, Category, Confidence, Finding, Severity, Span};
use locrin_core::index::Index;
use locrin_core::parse::ParsedFile;
use locrin_core::symbols::enclosing_symbol;

pub use locrin_core::ALLOW_MARK;

/// What a rule reads. A `File` rule looks only at the files parsed this run and
/// its findings can be cached per file. A `Graph` rule queries the index and
/// runs every time, because a change anywhere can move its answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    File,
    Graph,
}

/// Everything a rule is allowed to see: the parsed files of this run, the
/// repository config, the index, and the entry points. Rules never touch the
/// filesystem themselves.
pub struct RuleContext<'a> {
    pub files: &'a [ParsedFile],
    pub config: &'a Config,
    pub index: &'a Index,
    pub entries: &'a EntryPoints,
}

pub trait Rule {
    fn id(&self) -> &'static str;
    /// One line for reporters and documentation; SARIF shows it as the rule's short description.
    fn description(&self) -> &'static str;
    fn scope(&self) -> Scope;
    fn category(&self) -> Category;
    fn default_severity(&self) -> Severity;
    fn confidence(&self) -> Confidence;
    fn run(&self, ctx: &RuleContext) -> anyhow::Result<Vec<Finding>>;
}

/// The files a file rule is allowed to look at: every parsed file whose tree came
/// back without a syntax error. Rules iterate this instead of `ctx.files`, so a
/// file that failed to parse is exempt from every rule rather than from whichever
/// rules happened to check `has_error`.
pub fn clean_files<'a>(ctx: &'a RuleContext) -> impl Iterator<Item = &'a ParsedFile> {
    let files: &'a [ParsedFile] = ctx.files;
    files.iter().filter(|f| !f.has_error)
}

/// The trimmed text of a 1-based line, or `""` when the line is out of range.
pub fn line_text(file: &ParsedFile, line: u32) -> &str {
    file.source.lines().nth(line.saturating_sub(1) as usize).unwrap_or("").trim()
}

/// A span covering one whole line.
pub fn line_span(file: &ParsedFile, line: u32) -> Span {
    let text = line_text(file, line);
    Span { start_line: line, start_col: 0, end_line: line, end_col: text.len() as u32 }
}

/// The identity anchor for a finding: the enclosing symbol name if there is
/// one, otherwise the line text. Anchoring on the symbol is what keeps a
/// finding's id stable when unrelated lines move around it.
pub fn anchor_for(file: &ParsedFile, line: u32) -> String {
    enclosing_symbol(file, line).unwrap_or_else(|| line_text(file, line).to_string())
}

/// Builds a finding from its parts. Every rule constructs findings through this
/// or through `finding`, which is what keeps ids consistent across rules.
pub fn finding_at(rule: &dyn Rule, rel: &str, span: Span, anchor: &str, evidence: &str, fix: &str) -> Finding {
    Finding {
        id: make_id(rule.id(), rel, anchor),
        rule: rule.id().to_string(),
        category: rule.category(),
        severity: rule.default_severity(),
        confidence: rule.confidence(),
        file: rel.to_string(),
        span,
        evidence: evidence.to_string(),
        fix: fix.to_string(),
        related: vec![],
        owasp: None,
        cwe: None,
    }
}

/// Builds a finding for a file rule at a line, anchored on the enclosing symbol.
pub fn finding(rule: &dyn Rule, file: &ParsedFile, line: u32, evidence: &str, fix: &str) -> Finding {
    finding_at(rule, &file.rel, line_span(file, line), &anchor_for(file, line), evidence, fix)
}

/// Runs the given rules, skipping any the config disables, dropping any finding
/// that came from a file which failed to parse or whose line carries the
/// `locrin:allow` marker, and applying the configured severity to every finding
/// that survives.
///
/// The spec 9 gate is enforced here rather than left to the rules. For a file
/// parsed this run the parse status and the line text are at hand; for any
/// other file (graph rules report on files the run did not parse) the index
/// remembers both from when the file was last indexed.
pub fn run_rules(rules: &[Box<dyn Rule>], ctx: &RuleContext) -> anyhow::Result<Vec<Finding>> {
    let by_rel: HashMap<&str, &ParsedFile> = ctx.files.iter().map(|f| (f.rel.as_str(), f)).collect();
    let dropped = |f: &Finding| -> anyhow::Result<bool> {
        if let Some(file) = by_rel.get(f.file.as_str()) {
            return Ok(file.has_error || line_text(file, f.span.start_line).contains(ALLOW_MARK));
        }
        Ok(ctx.index.parse_status(&f.file)?.as_deref() == Some("error")
            || ctx.index.is_allowed(&f.file, f.span.start_line)?)
    };
    let mut out = Vec::new();
    for rule in rules {
        if !ctx.config.rule_enabled(rule.id()) {
            continue;
        }
        let severity = ctx.config.severity_for(rule.id(), rule.default_severity());
        for mut f in rule.run(ctx)? {
            if dropped(&f)? {
                continue;
            }
            f.severity = severity;
            out.push(f);
        }
    }
    Ok(out)
}

/// The registry: every rule the engine ships, in the order they are declared.
pub fn all_rules() -> Vec<Box<dyn Rule>> {
    vec![
        Box::new(leftover_debug::LeftoverDebug),
        Box::new(leftover_commented::LeftoverCommented),
        Box::new(leftover_marker::LeftoverMarker),
    ]
}

pub fn file_rules() -> Vec<Box<dyn Rule>> {
    all_rules().into_iter().filter(|r| r.scope() == Scope::File).collect()
}

pub fn graph_rules() -> Vec<Box<dyn Rule>> {
    all_rules().into_iter().filter(|r| r.scope() == Scope::Graph).collect()
}

pub fn run_all(ctx: &RuleContext) -> anyhow::Result<Vec<Finding>> {
    run_rules(&all_rules(), ctx)
}
```

- [ ] **Step 4: Update the three existing rules**

In each of `leftover_debug.rs`, `leftover_commented.rs`, `leftover_marker.rs`: add `Scope` to the `use crate::{...}` line, add these two methods to the `impl Rule`, and change `fn run(...) -> Vec<Finding>` to `-> anyhow::Result<Vec<Finding>>` with the final `out` becoming `Ok(out)`.

```rust
    fn description(&self) -> &'static str {
        "console.log, console.debug, or debugger left in code" // leftover_debug
    }
    fn scope(&self) -> Scope {
        Scope::File
    }
```

Descriptions for the other two: `"Three or more consecutive commented-out statements"` (leftover_commented) and `"TODO or FIXME without an issue reference"` (leftover_marker).

- [ ] **Step 5: Update the tests in `rules/src/lib.rs`**

In `mod tests`: both test rules (`Always`, `Careless`) gain `description` (return `"test rule"`), `scope` (return `Scope::File`), and `run` returning `Ok(...)`. Every `RuleContext { files: &files, config: &config }` becomes `RuleContext { files: &files, config: &config, index: &ix, entries: &entries }` with these two lines above it:

```rust
        let ix = Index::open_in_memory().unwrap();
        let entries = EntryPoints::detect(Path::new("."), &[]).unwrap();
```

Every `run_rules(...)` call gets `.unwrap()`. Then add one new test:

```rust
    /// A graph rule reports on files the run did not parse. The gate still holds
    /// for them, from what the index remembers.
    struct Ghost;
    impl Rule for Ghost {
        fn id(&self) -> &'static str {
            "ghost"
        }
        fn description(&self) -> &'static str {
            "test rule"
        }
        fn scope(&self) -> Scope {
            Scope::Graph
        }
        fn category(&self) -> Category {
            Category::Erosion
        }
        fn default_severity(&self) -> Severity {
            Severity::Medium
        }
        fn confidence(&self) -> Confidence {
            Confidence::Medium
        }
        fn run(&self, _ctx: &RuleContext) -> anyhow::Result<Vec<Finding>> {
            let span = Span { start_line: 1, start_col: 0, end_line: 1, end_col: 0 };
            Ok(vec![
                finding_at(self, "src/broken.ts", span.clone(), "a", "hit", "fix"),
                finding_at(self, "src/allowed.ts", span.clone(), "a", "hit", "fix"),
                finding_at(self, "src/plain.ts", span, "a", "hit", "fix"),
            ])
        }
    }

    #[test]
    fn the_gate_uses_the_index_for_files_not_parsed_this_run() {
        let mut ix = Index::open_in_memory().unwrap();
        ix.upsert_file("src/broken.ts", "typescript", "h", "error").unwrap();
        ix.upsert_file("src/allowed.ts", "typescript", "h", "ok").unwrap();
        ix.replace_allow_lines("src/allowed.ts", &[1]).unwrap();
        ix.upsert_file("src/plain.ts", "typescript", "h", "ok").unwrap();
        let files: Vec<ParsedFile> = vec![];
        let config = Config::default();
        let entries = EntryPoints::detect(Path::new("."), &[]).unwrap();
        let ctx = RuleContext { files: &files, config: &config, index: &ix, entries: &entries };
        let out = run_rules(&[Box::new(Ghost)], &ctx).unwrap();
        assert_eq!(out.iter().map(|f| f.file.as_str()).collect::<Vec<_>>(), vec!["src/plain.ts"]);
    }
```

Add `use locrin_core::entry::EntryPoints; use locrin_core::index::Index; use locrin_core::finding::Span; use locrin_core::parse::ParsedFile;` to the test module's imports as needed.

- [ ] **Step 6: Rewrite `crates/rules/tests/common/mod.rs`**

```rust
#![allow(dead_code)]

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use locrin_core::config::Config;
use locrin_core::entry::EntryPoints;
use locrin_core::finding::Finding;
use locrin_core::index::{content_hash, Index};
use locrin_core::indexer;
use locrin_core::parse::{parse_file, ParsedFile};
use locrin_core::resolve::Resolver;
use locrin_core::walk::{canonical_root, source_files, WalkOptions};
use locrin_rules::{run_rules, Rule, RuleContext};

pub fn fixture(rule: &str, bucket: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(rule).join(bucket)
}

pub fn parse_dir(root: &Path) -> Vec<ParsedFile> {
    let root = canonical_root(root);
    source_files(&root, &WalkOptions::default())
        .unwrap()
        .into_iter()
        .filter_map(|p| parse_file(&root, &p).unwrap())
        .collect()
}

/// Indexes a fixture directory in memory the way the CLI indexes a repository,
/// so graph rules see real edges, symbols, and entry points.
pub fn index_dir(root: &Path, files: &[ParsedFile], config: &Config) -> (Index, EntryPoints) {
    let root = canonical_root(root);
    let indexed: HashSet<String> = files.iter().map(|f| f.rel.clone()).collect();
    let resolver = Resolver::new(&root, indexed);
    let mut ix = Index::open_in_memory().unwrap();
    for f in files {
        indexer::record(&mut ix, f, &content_hash(&f.source), &resolver).unwrap();
    }
    (ix, EntryPoints::detect(&root, &config.entry_points).unwrap())
}

pub fn run_on(rule: Box<dyn Rule>, root: &Path, config: &Config) -> Vec<Finding> {
    let files = parse_dir(root);
    let (ix, entries) = index_dir(root, &files, config);
    let ctx = RuleContext { files: &files, config, index: &ix, entries: &entries };
    run_rules(&[rule], &ctx).unwrap()
}

/// (file, start line) pairs in the order the rule produced them.
pub fn hits(findings: &[Finding]) -> Vec<(String, u32)> {
    findings.iter().map(|f| (f.file.clone(), f.span.start_line)).collect()
}
```

Run: `cargo test -p locrin-rules` and expect every existing rule test to pass unchanged.

- [ ] **Step 7: Restructure `crates/cli/src/run.rs`**

Replace `candidate_files`, `index_files`, and `full_findings` with the following (keep `Options`, `Indexed`, `check`, `scan`, `baseline_create`, `baseline_accept`; `scan` and `full_findings` change as shown):

```rust
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::Context;
use locrin_core::baseline::Baseline;
use locrin_core::config::Config;
use locrin_core::entry::EntryPoints;
use locrin_core::finding::{Finding, Verdict};
use locrin_core::index::{content_hash, Index};
use locrin_core::indexer;
use locrin_core::lang::Language;
use locrin_core::parse::{parse_source, rel_path, ParsedFile};
use locrin_core::resolve::Resolver;
use locrin_core::walk::{canonical_path, canonical_root, source_files, WalkOptions};
use locrin_rules::{run_all, RuleContext};

pub struct Options {
    pub root: PathBuf,
    pub paths: Vec<PathBuf>,
    pub changed_only: bool,
    pub json: bool,
}

struct Indexed {
    files: Vec<ParsedFile>,
    changed: usize,
}

/// The files named on the command line, canonical and inside the root, or None
/// when nothing was named. A directory expands to the walked files beneath it,
/// so the config's excludes still apply inside it; a file is taken as named,
/// excluded or not, because naming a file is an instruction.
fn explicit_files(root: &Path, paths: &[PathBuf], walked: &[PathBuf]) -> anyhow::Result<Option<Vec<PathBuf>>> {
    if paths.is_empty() {
        return Ok(None);
    }
    let mut out = Vec::new();
    for p in paths {
        let abs = if p.is_absolute() { p.clone() } else { root.join(p) };
        // Canonicalise before anything else: `src/../src/dirty.ts` and
        // `src/dirty.ts` are one file, and only one of them may reach the index
        // or a finding id.
        let canon = canonical_path(&abs).with_context(|| format!("no such path: {}", p.display()))?;
        if !canon.starts_with(root) {
            anyhow::bail!("path is outside the repository root: {}", p.display());
        }
        if canon.is_dir() {
            out.extend(walked.iter().filter(|f| f.starts_with(&canon)).cloned());
        } else if Language::from_path(&canon).is_some() {
            out.push(canon);
        }
    }
    out.sort();
    out.dedup();
    Ok(Some(out))
}

/// Reads and hashes every candidate, parses the ones that changed, are in scope,
/// or all of them when `parse_all`, records the changed ones when `record`, and
/// returns the parsed files.
///
/// A file that is not valid UTF-8 is not a source file this engine can reason
/// about, so it is reported once and skipped rather than aborting the run.
///
/// `record` is what separates a run that observes the repository from one that
/// merely reads it. The baseline commands read every file to find the finding
/// they were asked about; if they also stamped the hashes they saw, an edit made
/// between two commands would look already-seen and the next `--changed` check
/// would skip it. Only `check` and `scan` are entitled to move the watermark.
fn index_files(
    root: &Path,
    candidates: &[PathBuf],
    parse_all: bool,
    scope: Option<&HashSet<String>>,
    record: bool,
    resolver: &Resolver,
    ix: &mut Index,
) -> anyhow::Result<Indexed> {
    let mut files = Vec::new();
    let mut changed = 0;
    for path in candidates {
        let rel = rel_path(root, path);
        let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        let source = match String::from_utf8(bytes) {
            Ok(s) => s,
            Err(_) => {
                eprintln!("warning: {rel} is not valid UTF-8; skipped");
                continue;
            }
        };
        let hash = content_hash(&source);
        let is_changed = ix.changed(&rel, &hash)?;
        let in_scope = scope.is_some_and(|s| s.contains(&rel));
        if !is_changed && !parse_all && !in_scope {
            continue;
        }
        let Some(parsed) = parse_source(path, &rel, source) else { continue };
        if parsed.has_error {
            eprintln!("warning: parse errors in {rel}; excluded from rules");
        }
        if is_changed {
            changed += 1;
            if record {
                indexer::record(ix, &parsed, &hash, resolver)?;
            }
        }
        files.push(parsed);
    }
    Ok(Indexed { files, changed })
}

/// Every current finding for a run. `record` decides whether the pass is allowed
/// to leave its mark on the index; see `index_files`.
///
/// The walk is always the whole repository, whatever was named on the command
/// line: the resolver has to know every file to resolve an import, and the
/// graph rules answer for the repository, not for a subset. Named paths narrow
/// what is parsed and what is reported, never what is indexed.
fn full_findings(root: &Path, opts: &Options, record: bool) -> anyhow::Result<Vec<Finding>> {
    let config = Config::load(root)?;
    let walked = source_files(root, &WalkOptions { excludes: config.excludes.clone() })?;
    let explicit = explicit_files(root, &opts.paths, &walked)?;
    let mut candidates = walked;
    if let Some(e) = &explicit {
        candidates.extend(e.iter().cloned());
        candidates.sort();
        candidates.dedup();
    }
    let rels: Vec<String> = candidates.iter().map(|p| rel_path(root, p)).collect();
    let scope: Option<HashSet<String>> = explicit.as_ref().map(|e| e.iter().map(|p| rel_path(root, p)).collect());
    let resolver = Resolver::new(root, rels.iter().cloned().collect());
    let mut ix = Index::open(root)?;
    // A whole-repository check parses everything because, until the findings
    // cache lands (plan 2 part B), that is the only way to run the file rules
    // over every file. A named path or `--changed` parses only what it must.
    let parse_all = scope.is_none() && !opts.changed_only;
    let indexed = index_files(root, &candidates, parse_all, scope.as_ref(), record, &resolver, &mut ix)?;
    // The candidate list is the whole repository on every run now, so pruning
    // rows for files that went away is safe whenever the run is recording.
    if record {
        ix.remove_missing(&rels)?;
    }
    let entries = EntryPoints::detect(root, &config.entry_points)?;
    let ctx = RuleContext { files: &indexed.files, config: &config, index: &ix, entries: &entries };
    let mut findings = run_all(&ctx)?;
    if let Some(scope) = &scope {
        findings.retain(|f| scope.contains(&f.file));
    } else if opts.changed_only {
        let changed: HashSet<&str> = indexed.files.iter().map(|f| f.rel.as_str()).collect();
        findings.retain(|f| changed.contains(f.file.as_str()));
    }
    Ok(findings)
}
```

And `scan` becomes:

```rust
pub fn scan(root: &Path) -> anyhow::Result<(usize, usize)> {
    let root = canonical_root(root);
    let config = Config::load(&root)?;
    let candidates = source_files(&root, &WalkOptions { excludes: config.excludes.clone() })?;
    let rels: Vec<String> = candidates.iter().map(|p| rel_path(&root, p)).collect();
    let resolver = Resolver::new(&root, rels.iter().cloned().collect());
    let mut ix = Index::open(&root)?;
    let indexed = index_files(&root, &candidates, false, None, true, &resolver, &mut ix)?;
    ix.remove_missing(&rels)?;
    Ok((candidates.len(), indexed.changed))
}
```

- [ ] **Step 8: Run the whole workspace**

Run: `cargo test --workspace`
Expected: everything green. The CLI fixture still yields "BLOCK  2 finding(s)" because no graph rule exists yet.

- [ ] **Step 9: Commit**

```bash
cargo fmt --all
git add crates/core/src/indexer.rs crates/core/src/lib.rs crates/rules/Cargo.toml crates/rules/src/lib.rs crates/rules/src/leftover_debug.rs crates/rules/src/leftover_commented.rs crates/rules/src/leftover_marker.rs crates/rules/tests/common/mod.rs crates/cli/src/run.rs Cargo.lock
git commit -m "engine: indexer records edges and allow lines, rules declare a scope, whole-repository walk on every run"
```

### Task 10: `unused-import`

**Files:**
- Create: `crates/rules/src/unused_import.rs`, `crates/rules/tests/unused_import.rs`, fixtures under `crates/rules/tests/fixtures/unused_import/{flag,clean,edge}/`
- Modify: `crates/rules/src/lib.rs` (module + registry)

**Interfaces:**
- Consumes: `imports::extract`, `clean_files`, `finding_at`, `line_span`.
- Produces: rule `unused-import`, `Scope::File`, Erosion, Low, High. Anchor: `"{specifier}\x1f{local}"`. Evidence: `` `{local}` is imported from "{specifier}" but never used ``. Fix: `` Remove `{local}` from the import, or the whole statement if nothing else from it is used ``.
- Rule: a binding is used when its local name appears anywhere in the file as an `identifier`, `type_identifier`, `shorthand_property_identifier`, or `jsx_identifier` outside import statements and outside `export ... from` statements. Shadowing counts as use (fewer flags, never a wrong one). Under the classic JSX runtime `React` (or the `/** @jsx h */` factory) is referenced by every element, so those bindings are exempt when the file contains JSX. Type-only imports are checked like any other.

- [ ] **Step 1: Write the fixtures**

`flag/a.ts`:
```ts
import { used, unused } from "./lib";
import type { OnlyType } from "./types";
import * as ns from "./ns";
import def from "./def";

export function run(): number {
  return used(1);
}
```

`clean/b.tsx`:
```tsx
import React from "react";
import { Text, type Props } from "./ui";
import { helper } from "./helper";
import "./side-effects";
import * as ns from "./ns";

export function Row(p: Props) {
  return <Text>{ns.label(helper(p))}</Text>;
}
```

`clean/c.ts`:
```ts
import { key } from "./key";
import { Shape } from "./shape";
import { reexported } from "./re";
import { a as renamed } from "./a";

export const obj = { key };
export const size: Shape = { w: 1 };
export { reexported };
export const total = renamed + 1;
```

`edge/d.ts`:
```ts
import { a, b } from "./ab"; // locrin:allow
import { shadow } from "./shadow";
import { gone } from "./gone";

export function f(shadow: number): number {
  return shadow;
}
```

`edge/e.tsx`:
```tsx
/** @jsx h */
import { h } from "preact";
export const el = <div />;
```

`edge/f.ts` (a name that only appears in an `export ... from` clause names the *other* module's export, not the local binding, so the import on line 1 is unused):
```ts
import { fromElsewhere } from "./elsewhere";
export { fromElsewhere as alias } from "./elsewhere";
```

- [ ] **Step 2: Write the failing test** `crates/rules/tests/unused_import.rs`

```rust
mod common;

use common::{fixture, hits, run_on};
use locrin_core::config::Config;
use locrin_core::finding::{Confidence, Severity};
use locrin_rules::unused_import::UnusedImport;

#[test]
fn flags_every_unused_binding_including_type_namespace_and_default() {
    let out = run_on(Box::new(UnusedImport), &fixture("unused_import", "flag"), &Config::default());
    assert_eq!(hits(&out), vec![("a.ts".into(), 1), ("a.ts".into(), 2), ("a.ts".into(), 3), ("a.ts".into(), 4)]);
    assert_eq!(out[0].evidence, "`unused` is imported from \"./lib\" but never used");
    assert!(out.iter().all(|f| f.severity == Severity::Low && f.confidence == Confidence::High));
    assert_eq!(out[0].rule, "unused-import");
}

#[test]
fn jsx_types_shorthand_export_clauses_and_aliases_count_as_use() {
    let out = run_on(Box::new(UnusedImport), &fixture("unused_import", "clean"), &Config::default());
    assert!(out.is_empty(), "{out:?}");
}

#[test]
fn allow_marker_shadowing_jsx_pragma_and_export_from() {
    let out = run_on(Box::new(UnusedImport), &fixture("unused_import", "edge"), &Config::default());
    // Files come in walk order: d.ts, e.tsx, f.ts. e.tsx is clean (pragma factory).
    assert_eq!(hits(&out), vec![("d.ts".into(), 3), ("f.ts".into(), 1)], "{out:?}");
}
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test -p locrin-rules --test unused_import`
Expected: compile error, no module `unused_import`.

- [ ] **Step 4: Implement `crates/rules/src/unused_import.rs`**

```rust
//! Flags an imported binding the file never references. Purely syntactic: any
//! occurrence of the local name as an identifier, type identifier, shorthand
//! property, or JSX identifier outside the import statements counts as a use,
//! so shadowing means "not flagged" rather than "wrong". Under the classic JSX
//! runtime `React` (or the `@jsx` pragma factory) is referenced by every element.

use std::collections::HashSet;

use locrin_core::finding::{Category, Confidence, Finding, Severity};
use locrin_core::imports;
use tree_sitter::Node;

use crate::{clean_files, finding_at, line_span, Rule, RuleContext, Scope};

pub struct UnusedImport;

const REFERENCE_KINDS: &[&str] = &["identifier", "type_identifier", "shorthand_property_identifier", "jsx_identifier"];

struct Usage {
    names: HashSet<String>,
    has_jsx: bool,
}

fn collect(node: Node, src: &str, u: &mut Usage) {
    match node.kind() {
        "import_statement" => return,
        // `export { x } from "./y"` names x in y, not a local binding.
        "export_statement" if node.child_by_field_name("source").is_some() => return,
        k if REFERENCE_KINDS.contains(&k) => {
            u.names.insert(node.utf8_text(src.as_bytes()).unwrap_or("").to_string());
        }
        k if k.starts_with("jsx_") => u.has_jsx = true,
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect(child, src, u);
    }
}

/// The factory named by a `/** @jsx h */` pragma, if any.
fn jsx_factory(src: &str) -> Option<String> {
    let i = src.find("@jsx ")?;
    src[i + 5..].split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$')).next().map(String::from)
}

impl Rule for UnusedImport {
    fn id(&self) -> &'static str {
        "unused-import"
    }
    fn description(&self) -> &'static str {
        "Imported name that the file never references"
    }
    fn scope(&self) -> Scope {
        Scope::File
    }
    fn category(&self) -> Category {
        Category::Erosion
    }
    fn default_severity(&self) -> Severity {
        Severity::Low
    }
    fn confidence(&self) -> Confidence {
        Confidence::High
    }

    fn run(&self, ctx: &RuleContext) -> anyhow::Result<Vec<Finding>> {
        let mut out = Vec::new();
        for file in clean_files(ctx) {
            let mut usage = Usage { names: HashSet::new(), has_jsx: false };
            collect(file.tree.root_node(), &file.source, &mut usage);
            let factory = jsx_factory(&file.source);
            for import in imports::extract(file) {
                for b in &import.bindings {
                    if usage.names.contains(&b.local) {
                        continue;
                    }
                    if usage.has_jsx && (b.local == "React" || factory.as_deref() == Some(b.local.as_str())) {
                        continue;
                    }
                    let evidence = format!("`{}` is imported from \"{}\" but never used", b.local, import.specifier);
                    let fix =
                        format!("Remove `{}` from the import, or the whole statement if nothing else from it is used", b.local);
                    let anchor = format!("{}\x1f{}", import.specifier, b.local);
                    out.push(finding_at(self, &file.rel, line_span(file, b.line), &anchor, &evidence, &fix));
                }
            }
        }
        Ok(out)
    }
}
```

Register it: `pub mod unused_import;` in `rules/src/lib.rs` and `Box::new(unused_import::UnusedImport)` appended to `all_rules()`. Update `registry_lists_every_shipped_rule` to expect `["leftover-debug", "leftover-commented-code", "leftover-agent-marker", "unused-import"]`.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p locrin-rules`
Expected: PASS. Then `cargo test -p locrin-cli`: the fixture repo has no imports, so the CLI counts are unchanged.

- [ ] **Step 6: Commit**

```bash
cargo fmt --all
git add crates/rules/src/unused_import.rs crates/rules/src/lib.rs crates/rules/tests/unused_import.rs crates/rules/tests/fixtures/unused_import
git commit -m "engine: unused-import rule"
```

### Task 11: `unreachable`

**Files:**
- Create: `crates/rules/src/unreachable.rs`, `crates/rules/tests/unreachable.rs`, fixtures under `crates/rules/tests/fixtures/unreachable/{flag,clean,edge}/`
- Modify: `crates/rules/src/lib.rs`

**Interfaces:**
- Produces: rule `unreachable`, `Scope::File`, Erosion, Medium, High. Anchor: `anchor_for(file, first dead line)`. Evidence: the trimmed text of the first dead statement. Fix: `` Delete the code after the `{keyword}` on line {n}, or move it before the `{keyword}` ``. Span: from the first dead statement's start to the last consecutive dead statement's end.
- Rule: inside a `statement_block`, the `program`, a `switch_case`, or a `switch_default`, any statement after a sibling `return`, `throw`, `break`, or `continue` is dead, except statements that run regardless of position: function declarations (hoisted), `var` declarations without an initializer (hoisted, no effect), type-level declarations (erased), imports and exports, and empty statements. One finding per terminator. Only direct siblings are considered; `if (a) { return } else { return }` followed by code is not flagged (that needs flow analysis, which is out of scope).

- [ ] **Step 1: Write the fixtures**

`flag/a.ts` (31 lines; line numbers matter):
```ts
export function a(x: number): number {
  return x;
  console.log("never");
}
export function b(): void {
  throw new Error("boom");
  cleanup();
  more();
}
export function c(xs: number[]): number {
  for (const x of xs) {
    if (x > 1) {
      continue;
      xs.push(x);
    }
    break;
    xs.pop();
  }
  return 0;
}
export function d(k: number): string {
  switch (k) {
    case 1:
      return "one";
      break;
    default:
      return "other";
  }
}
function cleanup() {}
function more() {}
```

`clean/b.ts`:
```ts
export function a(x: number): number {
  if (x > 0) {
    return x;
  }
  return -x;
}
export function b(): number {
  return helper();
  function helper(): number {
    return 1;
  }
}
export function c(): void {
  throw new Error("x");
  type Local = number;
}
export function d(k: number): number {
  switch (k) {
    case 1:
      return 1;
    case 2: {
      return 2;
    }
    default:
      return 0;
  }
}
export const e = (): void => {
  return;
};
```

`edge/c.ts`:
```ts
export function a(): number {
  return 1;
  // a trailing comment is not code
}
export function b(): number {
  return 2;
  var hoisted;
  var assigned = 3;
}
export function c(): number {
  return 3;
  console.log("allowed"); // locrin:allow
}
```

- [ ] **Step 2: Write the failing test** `crates/rules/tests/unreachable.rs`

```rust
mod common;

use common::{fixture, hits, run_on};
use locrin_core::config::Config;
use locrin_core::finding::{Confidence, Severity};
use locrin_rules::unreachable::Unreachable;

#[test]
fn flags_code_after_return_throw_break_and_continue() {
    let out = run_on(Box::new(Unreachable), &fixture("unreachable", "flag"), &Config::default());
    assert_eq!(
        hits(&out),
        vec![("a.ts".into(), 3), ("a.ts".into(), 7), ("a.ts".into(), 14), ("a.ts".into(), 17), ("a.ts".into(), 25)],
        "{out:?}"
    );
    let b = &out[1];
    assert_eq!((b.span.start_line, b.span.end_line), (7, 8), "one finding spans every dead statement in the block");
    assert_eq!(b.evidence, "cleanup();");
    assert_eq!(b.fix, "Delete the code after the `throw` on line 6, or move it before the `throw`");
    assert!(out.iter().all(|f| f.severity == Severity::Medium && f.confidence == Confidence::High));
}

#[test]
fn branches_hoisted_functions_type_declarations_and_case_blocks_are_clean() {
    let out = run_on(Box::new(Unreachable), &fixture("unreachable", "clean"), &Config::default());
    assert!(out.is_empty(), "{out:?}");
}

#[test]
fn comments_hoisted_vars_and_allow_marker() {
    let out = run_on(Box::new(Unreachable), &fixture("unreachable", "edge"), &Config::default());
    assert_eq!(hits(&out), vec![("c.ts".into(), 8)], "{out:?}");
}
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test -p locrin-rules --test unreachable`
Expected: compile error, no module `unreachable`.

- [ ] **Step 4: Implement `crates/rules/src/unreachable.rs`**

```rust
//! Flags statements that follow an unconditional `return`, `throw`, `break`, or
//! `continue` in the same block. Sibling analysis only, no flow graph: what it
//! flags is dead beyond argument, and what needs a flow graph is left alone.

use locrin_core::finding::{Category, Confidence, Finding, Severity, Span};
use locrin_core::parse::ParsedFile;
use locrin_core::tree::line;
use tree_sitter::Node;

use crate::{anchor_for, clean_files, finding_at, line_text, Rule, RuleContext, Scope};

pub struct Unreachable;

const TERMINATORS: &[&str] = &["return_statement", "throw_statement", "break_statement", "continue_statement"];

/// Statements that take effect regardless of where they sit: hoisted, erased,
/// or empty. Code after a terminator made only of these is not dead.
const POSITION_FREE: &[&str] = &[
    "function_declaration",
    "generator_function_declaration",
    "type_alias_declaration",
    "interface_declaration",
    "ambient_declaration",
    "function_signature",
    "import_alias",
    "import_statement",
    "export_statement",
    "empty_statement",
];

fn body<'a>(node: Node<'a>) -> Vec<Node<'a>> {
    let mut cursor = node.walk();
    match node.kind() {
        "statement_block" | "program" => {
            node.named_children(&mut cursor).filter(|c| c.kind() != "comment" && c.kind() != "hash_bang_line").collect()
        }
        "switch_case" | "switch_default" => node.children_by_field_name("body", &mut cursor).collect(),
        _ => vec![],
    }
}

fn position_free(node: Node) -> bool {
    if POSITION_FREE.contains(&node.kind()) {
        return true;
    }
    // `var x;` is hoisted and does nothing where it stands; `var x = 1;` does.
    if node.kind() == "variable_declaration" {
        let mut cursor = node.walk();
        return node
            .named_children(&mut cursor)
            .all(|d| d.kind() != "variable_declarator" || d.child_by_field_name("value").is_none());
    }
    false
}

struct Dead<'a> {
    keyword: &'static str,
    terminator_line: u32,
    first: Node<'a>,
    last: Node<'a>,
}

fn scan<'a>(node: Node<'a>, out: &mut Vec<Dead<'a>>) {
    let stmts = body(node);
    for (i, s) in stmts.iter().enumerate() {
        let Some(kind) = TERMINATORS.iter().find(|t| **t == s.kind()) else { continue };
        let dead: Vec<Node> = stmts[i + 1..].iter().copied().filter(|n| !position_free(*n)).collect();
        if let (Some(first), Some(last)) = (dead.first(), dead.last()) {
            out.push(Dead {
                keyword: kind.trim_end_matches("_statement"),
                terminator_line: line(*s),
                first: *first,
                last: *last,
            });
        }
        break; // one finding per block: everything after the terminator is covered
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        scan(child, out);
    }
}

fn span(d: &Dead) -> Span {
    Span {
        start_line: line(d.first),
        start_col: d.first.start_position().column as u32,
        end_line: d.last.end_position().row as u32 + 1,
        end_col: d.last.end_position().column as u32,
    }
}

fn report(rule: &Unreachable, file: &ParsedFile, d: &Dead) -> Finding {
    let first_line = line(d.first);
    let fix = format!(
        "Delete the code after the `{}` on line {}, or move it before the `{}`",
        d.keyword, d.terminator_line, d.keyword
    );
    finding_at(rule, &file.rel, span(d), &anchor_for(file, first_line), line_text(file, first_line), &fix)
}

impl Rule for Unreachable {
    fn id(&self) -> &'static str {
        "unreachable"
    }
    fn description(&self) -> &'static str {
        "Code after an unconditional return, throw, break, or continue"
    }
    fn scope(&self) -> Scope {
        Scope::File
    }
    fn category(&self) -> Category {
        Category::Erosion
    }
    fn default_severity(&self) -> Severity {
        Severity::Medium
    }
    fn confidence(&self) -> Confidence {
        Confidence::High
    }

    fn run(&self, ctx: &RuleContext) -> anyhow::Result<Vec<Finding>> {
        let mut out = Vec::new();
        for file in clean_files(ctx) {
            let mut dead = Vec::new();
            scan(file.tree.root_node(), &mut dead);
            dead.sort_by_key(|d| (line(d.first), d.first.start_position().column));
            out.extend(dead.iter().map(|d| report(self, file, d)));
        }
        Ok(out)
    }
}
```

Register: `pub mod unreachable;`, `Box::new(unreachable::Unreachable)` in `all_rules()`, registry test expects `[..., "unused-import", "unreachable"]`.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p locrin-rules`
Expected: PASS. If the `case 1: return "one"; break;` finding at line 25 is missing, print the sexp of the switch: the `body` children of `switch_case` must include both statements; if the grammar nests them differently, fix `body`, not the fixture.

- [ ] **Step 6: Commit**

```bash
cargo fmt --all
git add crates/rules/src/unreachable.rs crates/rules/src/lib.rs crates/rules/tests/unreachable.rs crates/rules/tests/fixtures/unreachable
git commit -m "engine: unreachable rule"
```

### Task 12: `dead-export`

**Files:**
- Create: `crates/rules/src/dead_export.rs`, `crates/rules/tests/dead_export.rs`, fixtures under `crates/rules/tests/fixtures/dead_export/{flag,clean,edge}/`
- Modify: `crates/rules/src/lib.rs`

**Interfaces:**
- Consumes: `symbols::exported(ix)`, `edges::resolved(ix)`, `ctx.entries.is_entry`.
- Produces: rule `dead-export`, `Scope::Graph`, Erosion, Low, Medium. Anchor: the export name. Span: the symbol's span. Evidence: `` `{export_name}` is exported from {rel} but imported nowhere ``. Fix for kinds `export`, `reexport`, `default`: `Remove the export; nothing imports it`; otherwise `` Drop the `export` keyword if `{name}` is only used in this file, or delete it if it is unused ``.
- Also `pub(crate) fn imported_names(ctx) -> Result<HashMap<String, HashSet<String>>>`: for every file, the set of names resolved edges take from it (`*` means the whole module). Reused by `dead-file`.
- Rule: an exported symbol is live when some resolved edge into its file names it or names `*`. Exempt: files that match an entry point, and files with no incoming edge at all (those are `dead-file`'s to report; one finding beats one per export). Barrels are one level by construction: `export { x } from "./a"` in `b.ts` is an edge into `a.ts` naming `x` (keeps `a.x` alive) and a `reexport` symbol on `b.ts` (checked in its own right).

- [ ] **Step 1: Write the fixtures**

`flag/package.json`: `{ "name": "flag", "main": "src/main.ts" }`

`flag/src/main.ts`:
```ts
import { used } from "./lib";
import * as all from "./ns";
import { y } from "./barrel";
export const run = (): number => used() + all.x + y;
```

`flag/src/lib.ts`:
```ts
export function used(): number {
  return 1;
}
export function unused(): number {
  return 2;
}
export default function (): number {
  return 3;
}
```

`flag/src/ns.ts`:
```ts
export const x = 1;
export const y = 2;
```

`flag/src/barrel.ts`:
```ts
export { used } from "./lib";
export * from "./ns";
```

`clean/package.json`: `{ "name": "clean", "main": "src/index.ts" }`

`clean/src/index.ts`:
```ts
import { a } from "./a";
import b from "./b";
import { renamed } from "./c";
import { util } from "./util.js";
export const total = a + b() + renamed + util;
```

`clean/src/a.ts`: `export const a = 1;`
`clean/src/b.ts`: `export default function b(): number { return 2; }`
`clean/src/c.ts`: `const local = 3;\nexport { local as renamed };`
`clean/src/util.ts`: `export const util = 4;`
`clean/app/page.tsx`: `export default function Page() { return null; }`
`clean/src/a.test.ts`: `export const helper = 1;`

`edge/package.json`: `{ "name": "edge", "main": "src/main.ts" }`

`edge/src/main.ts`:
```ts
import "./side";
export const run = 1;
```

`edge/src/side.ts`:
```ts
export function setup(): void {}
```

`edge/src/orphan.ts`:
```ts
export const nobodyImportsThisFile = 1;
```

- [ ] **Step 2: Write the failing test** `crates/rules/tests/dead_export.rs`

```rust
mod common;

use common::{fixture, run_on};
use locrin_core::config::Config;
use locrin_core::finding::{Confidence, Severity};
use locrin_rules::dead_export::DeadExport;

fn view(out: &[locrin_core::finding::Finding]) -> Vec<(String, u32, String)> {
    out.iter().map(|f| (f.file.clone(), f.span.start_line, f.evidence.clone())).collect()
}

#[test]
fn flags_unimported_exports_defaults_and_barrel_reexports() {
    let out = run_on(Box::new(DeadExport), &fixture("dead_export", "flag"), &Config::default());
    assert_eq!(
        view(&out),
        vec![
            ("src/barrel.ts".into(), 1, "`used` is exported from src/barrel.ts but imported nowhere".into()),
            ("src/lib.ts".into(), 4, "`unused` is exported from src/lib.ts but imported nowhere".into()),
            ("src/lib.ts".into(), 7, "`default` is exported from src/lib.ts but imported nowhere".into()),
        ],
        "{out:?}"
    );
    assert!(out.iter().all(|f| f.severity == Severity::Low && f.confidence == Confidence::Medium));
    assert_eq!(out[0].fix, "Remove the export; nothing imports it");
    assert!(out[1].fix.starts_with("Drop the `export` keyword if `unused`"));
}

#[test]
fn entries_aliases_output_extensions_and_default_imports_are_live() {
    let out = run_on(Box::new(DeadExport), &fixture("dead_export", "clean"), &Config::default());
    assert!(out.is_empty(), "{out:?}");
}

#[test]
fn side_effect_import_keeps_the_file_but_not_its_exports_and_orphans_are_left_to_dead_file() {
    let out = run_on(Box::new(DeadExport), &fixture("dead_export", "edge"), &Config::default());
    assert_eq!(view(&out).iter().map(|(f, l, _)| (f.as_str(), *l)).collect::<Vec<_>>(), vec![("src/side.ts", 1)], "{out:?}");
}
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test -p locrin-rules --test dead_export`
Expected: compile error, no module `dead_export`.

- [ ] **Step 4: Implement `crates/rules/src/dead_export.rs`**

```rust
//! Flags an exported name that no resolved import anywhere in the repository asks
//! for. Barrels are followed one level (spec 3.2). Entry points are exempt: a
//! framework or a runner imports them by convention the graph cannot see. Files
//! with no incoming edge at all are left to `dead-file`, which says it once.
//! Medium confidence because an unresolved import anywhere could be the consumer.

use std::collections::{HashMap, HashSet};

use locrin_core::edges;
use locrin_core::finding::{Category, Confidence, Finding, Severity, Span};
use locrin_core::symbols;

use crate::{finding_at, Rule, RuleContext, Scope};

pub struct DeadExport;

/// For every file, the names resolved edges take from it. `*` is the whole module.
pub(crate) fn imported_names(ctx: &RuleContext) -> anyhow::Result<HashMap<String, HashSet<String>>> {
    let mut out: HashMap<String, HashSet<String>> = HashMap::new();
    for e in edges::resolved(ctx.index)? {
        if let Some(to) = e.to_rel {
            out.entry(to).or_default().insert(e.name);
        }
    }
    Ok(out)
}

impl Rule for DeadExport {
    fn id(&self) -> &'static str {
        "dead-export"
    }
    fn description(&self) -> &'static str {
        "Exported symbol that no file imports"
    }
    fn scope(&self) -> Scope {
        Scope::Graph
    }
    fn category(&self) -> Category {
        Category::Erosion
    }
    fn default_severity(&self) -> Severity {
        Severity::Low
    }
    fn confidence(&self) -> Confidence {
        Confidence::Medium
    }

    fn run(&self, ctx: &RuleContext) -> anyhow::Result<Vec<Finding>> {
        let used = imported_names(ctx)?;
        let mut out = Vec::new();
        for s in symbols::exported(ctx.index)? {
            let Some(export_name) = s.export_name.as_deref() else { continue };
            if ctx.entries.is_entry(&s.rel) {
                continue;
            }
            let Some(names) = used.get(&s.rel) else { continue }; // no importer at all: dead-file territory
            if names.contains(export_name) || names.contains("*") {
                continue;
            }
            let evidence = format!("`{export_name}` is exported from {} but imported nowhere", s.rel);
            let fix = match s.kind.as_str() {
                "export" | "reexport" | "default" => "Remove the export; nothing imports it".to_string(),
                _ => format!(
                    "Drop the `export` keyword if `{}` is only used in this file, or delete it if it is unused",
                    s.name
                ),
            };
            let span = Span { start_line: s.start_line, start_col: s.start_col, end_line: s.end_line, end_col: s.end_col };
            out.push(finding_at(self, &s.rel, span, export_name, &evidence, &fix));
        }
        Ok(out)
    }
}
```

Register: `pub mod dead_export;`, `Box::new(dead_export::DeadExport)`, registry test gains `"dead-export"`.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p locrin-rules`
Expected: PASS. Then run `cargo test -p locrin-cli`: `check_blocks_on_debug_and_reports_marker` now FAILS because `clean.ts` and `dirty.ts` export functions nobody imports. That is expected and is fixed in Task 15 by making the fixture a real small project; do not touch the CLI tests here.

- [ ] **Step 6: Commit**

```bash
cargo fmt --all
git add crates/rules/src/dead_export.rs crates/rules/src/lib.rs crates/rules/tests/dead_export.rs crates/rules/tests/fixtures/dead_export
git commit -m "engine: dead-export rule over the import graph"
```

### Task 13: `dead-file`

**Files:**
- Create: `crates/rules/src/dead_file.rs`, `crates/rules/tests/dead_file.rs`, fixtures under `crates/rules/tests/fixtures/dead_file/{flag,clean,edge}/`
- Modify: `crates/rules/src/lib.rs`

**Interfaces:**
- Consumes: `dead_export::imported_names`, `ctx.index.all_files()`, `ctx.entries`.
- Produces: rule `dead-file`, `Scope::Graph`, Erosion, Medium, Medium. Anchor: `"file"`. Span: line 1, cols 0 to 0. Evidence: `{rel} is imported nowhere and is not an entry point`. Fix: `Delete the file, or add it to entry_points in locrin.toml if a framework or script loads it by convention`.
- Known blind spot, documented in the module: two files that import only each other are "imported somewhere" and are not reported. Precision over recall.

- [ ] **Step 1: Write the fixtures**

`flag/package.json`: `{ "name": "flag", "main": "src/main.ts" }`
`flag/src/main.ts`: `import { a } from "./a";\nexport const run = a;`
`flag/src/a.ts`: `export const a = 1;`
`flag/src/orphan.ts`: `export const orphan = 1;`
`flag/src/cycle1.ts`: `import { c2 } from "./cycle2";\nexport const c1 = c2;`
`flag/src/cycle2.ts`: `import { c1 } from "./cycle1";\nexport const c2 = c1;`

`clean/package.json`: `{ "name": "clean", "main": "src/main.ts" }`
`clean/src/main.ts`: `import { a } from "./a";\nexport const run = a;`
`clean/src/a.ts`: `export const a = 1;`
`clean/app/index.tsx`: `export default function Home() { return null; }`
`clean/scripts/build.ts`: `export const build = 1;`
`clean/src/x.test.ts`: `export const t = 1;`
`clean/jest.config.ts`: `export default {};`

`edge/package.json`: `{ "name": "edge" }`
`edge/tools/run.ts`: `export const run = 1;`
`edge/src/left.ts`: `// locrin:allow\nexport const left = 1;`
`edge/src/gone.ts`: `export const gone = 1;`

- [ ] **Step 2: Write the failing test** `crates/rules/tests/dead_file.rs`

```rust
mod common;

use common::{fixture, hits, run_on};
use locrin_core::config::Config;
use locrin_core::finding::{Confidence, Severity};
use locrin_rules::dead_file::DeadFile;

#[test]
fn flags_orphans_but_not_cycles() {
    let out = run_on(Box::new(DeadFile), &fixture("dead_file", "flag"), &Config::default());
    assert_eq!(hits(&out), vec![("src/orphan.ts".into(), 1)], "{out:?}");
    assert_eq!(out[0].evidence, "src/orphan.ts is imported nowhere and is not an entry point");
    assert!(out.iter().all(|f| f.severity == Severity::Medium && f.confidence == Confidence::Medium));
}

#[test]
fn entry_points_by_package_and_convention_are_clean() {
    let out = run_on(Box::new(DeadFile), &fixture("dead_file", "clean"), &Config::default());
    assert!(out.is_empty(), "{out:?}");
}

#[test]
fn config_entry_points_and_allow_marker_on_line_one() {
    let config = Config { entry_points: vec!["tools/**".into()], ..Config::default() };
    let out = run_on(Box::new(DeadFile), &fixture("dead_file", "edge"), &config);
    assert_eq!(hits(&out), vec![("src/gone.ts".into(), 1)], "{out:?}");
}
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test -p locrin-rules --test dead_file`
Expected: compile error, no module `dead_file`.

- [ ] **Step 4: Implement `crates/rules/src/dead_file.rs`**

```rust
//! Flags a source file that no resolved import reaches and that is not an entry
//! point. Blind spot, by design: files that import only each other count as
//! imported and are not reported; precision beats recall for an advisory.

use locrin_core::finding::{Category, Confidence, Finding, Severity, Span};

use crate::dead_export::imported_names;
use crate::{finding_at, Rule, RuleContext, Scope};

pub struct DeadFile;

impl Rule for DeadFile {
    fn id(&self) -> &'static str {
        "dead-file"
    }
    fn description(&self) -> &'static str {
        "Source file that nothing imports and no framework loads"
    }
    fn scope(&self) -> Scope {
        Scope::Graph
    }
    fn category(&self) -> Category {
        Category::Erosion
    }
    fn default_severity(&self) -> Severity {
        Severity::Medium
    }
    fn confidence(&self) -> Confidence {
        Confidence::Medium
    }

    fn run(&self, ctx: &RuleContext) -> anyhow::Result<Vec<Finding>> {
        let imported = imported_names(ctx)?;
        let mut out = Vec::new();
        for rel in ctx.index.all_files()? {
            if imported.contains_key(&rel) || ctx.entries.is_entry(&rel) {
                continue;
            }
            let span = Span { start_line: 1, start_col: 0, end_line: 1, end_col: 0 };
            out.push(finding_at(
                self,
                &rel,
                span,
                "file",
                &format!("{rel} is imported nowhere and is not an entry point"),
                "Delete the file, or add it to entry_points in locrin.toml if a framework or script loads it by convention",
            ));
        }
        Ok(out)
    }
}
```

Register: `pub mod dead_file;`, `Box::new(dead_file::DeadFile)`, registry test gains `"dead-file"`.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p locrin-rules`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
cargo fmt --all
git add crates/rules/src/dead_file.rs crates/rules/src/lib.rs crates/rules/tests/dead_file.rs crates/rules/tests/fixtures/dead_file
git commit -m "engine: dead-file rule"
```

### Task 14: `boundary-violation`

**Files:**
- Create: `crates/rules/src/boundary.rs`, `crates/rules/tests/boundary.rs`, fixtures under `crates/rules/tests/fixtures/boundary/flag/`
- Modify: `crates/rules/src/lib.rs`

**Interfaces:**
- Consumes: `config.boundaries`, `edges::resolved(ix)`.
- Produces: rule `boundary-violation`, `Scope::Graph`, Erosion, High, High. Anchor: `"{label}\x1f{specifier}"` where label is the boundary name or `from -> forbidden` / `from -> outside allow list`. Span: the import line, cols 0 to 0. Evidence: `{from} imports "{specifier}" ({to}), which the boundary `{label}` forbids`. Fix: `Move the shared code somewhere both sides may import, or change the boundary in locrin.toml`. `related`: `[to_rel]`. One finding per import line per boundary (several names on one line are one violation).

- [ ] **Step 1: Write the fixture** (one directory; the config is built in the test)

`flag/package.json`: `{ "name": "flag", "main": "src/main.ts" }`
`flag/src/main.ts`: `import { screen } from "./ui/screen";\nimport { client } from "./db/client";\nexport const run = screen + client;`
`flag/src/ui/screen.ts`: `import { client, other } from "../db/client";\nimport { util } from "../shared/util";\nexport const screen = client + other + util;`
`flag/src/db/client.ts`: `import { util } from "../shared/util";\nimport { screen } from "../ui/screen";\nexport const client = util + screen;\nexport const other = 2;`
`flag/src/shared/util.ts`: `export const util = 1;`

- [ ] **Step 2: Write the failing test** `crates/rules/tests/boundary.rs`

```rust
mod common;

use common::{fixture, hits, run_on};
use locrin_core::config::{Boundary, Config};
use locrin_core::finding::{Confidence, Severity};
use locrin_rules::boundary::BoundaryViolation;

fn config() -> Config {
    Config {
        boundaries: vec![
            Boundary {
                name: Some("ui stays off the database".into()),
                from: "src/ui/**".into(),
                forbid: vec!["src/db/**".into()],
                allow: vec![],
            },
            Boundary { name: None, from: "src/db/**".into(), forbid: vec![], allow: vec!["src/shared/**".into()] },
        ],
        ..Config::default()
    }
}

#[test]
fn forbid_and_allow_boundaries_flag_one_finding_per_import_line() {
    let out = run_on(Box::new(BoundaryViolation), &fixture("boundary", "flag"), &config());
    assert_eq!(hits(&out), vec![("src/db/client.ts".into(), 2), ("src/ui/screen.ts".into(), 1)], "{out:?}");
    let ui = &out[1];
    assert_eq!(
        ui.evidence,
        "src/ui/screen.ts imports \"../db/client\" (src/db/client.ts), which the boundary `ui stays off the database` forbids"
    );
    assert_eq!(ui.related, vec!["src/db/client.ts".to_string()]);
    assert!(out[0].evidence.contains("src/db/** -> outside allow list"), "{}", out[0].evidence);
    assert!(out.iter().all(|f| f.severity == Severity::High && f.confidence == Confidence::High));
}

#[test]
fn no_boundaries_means_no_findings() {
    let out = run_on(Box::new(BoundaryViolation), &fixture("boundary", "flag"), &Config::default());
    assert!(out.is_empty());
}

#[test]
fn a_from_glob_may_import_itself_under_allow() {
    let config = Config {
        boundaries: vec![Boundary { name: None, from: "src/**".into(), forbid: vec![], allow: vec![] }],
        ..Config::default()
    };
    // Every import in the fixture stays inside src/**, so an allow list that is
    // empty apart from the implicit "self" rule flags nothing.
    let out = run_on(Box::new(BoundaryViolation), &fixture("boundary", "flag"), &config);
    assert!(out.is_empty(), "{out:?}");
}
```

(The third test bypasses `Config::load` validation on purpose: it checks the rule's own handling of the implicit self-allow, which `load` would never let through with both lists empty.)

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test -p locrin-rules --test boundary`
Expected: compile error, no module `boundary`.

- [ ] **Step 4: Implement `crates/rules/src/boundary.rs`**

```rust
//! Flags a resolved import that crosses a direction the config forbids (spec
//! 4.1, the config-driven half of boundary checking). Inferred boundaries are
//! release two.

use globset::{Glob, GlobMatcher, GlobSet, GlobSetBuilder};
use locrin_core::config::{Boundary, CONFIG_FILE};
use locrin_core::edges;
use locrin_core::finding::{Category, Confidence, Finding, Severity, Span};

use crate::{finding_at, Rule, RuleContext, Scope};

pub struct BoundaryViolation;

struct Compiled<'a> {
    boundary: &'a Boundary,
    from: GlobMatcher,
    forbid: GlobSet,
    allow: GlobSet,
    label: String,
}

fn set(globs: &[String]) -> anyhow::Result<GlobSet> {
    let mut b = GlobSetBuilder::new();
    for g in globs {
        b.add(Glob::new(g)?);
    }
    Ok(b.build()?)
}

fn compile(boundaries: &[Boundary]) -> anyhow::Result<Vec<Compiled>> {
    boundaries
        .iter()
        .map(|b| {
            let label = b.name.clone().unwrap_or_else(|| {
                format!("{} -> {}", b.from, if b.forbid.is_empty() { "outside allow list" } else { "forbidden" })
            });
            Ok(Compiled {
                boundary: b,
                from: Glob::new(&b.from)?.compile_matcher(),
                forbid: set(&b.forbid)?,
                allow: set(&b.allow)?,
                label,
            })
        })
        .collect()
}

impl Compiled<'_> {
    fn violated(&self, from: &str, to: &str) -> bool {
        if !self.from.is_match(from) {
            return false;
        }
        if !self.boundary.forbid.is_empty() {
            return self.forbid.is_match(to);
        }
        // An allow list: anything outside it is a violation, except the
        // boundary's own side, which may always talk to itself.
        !self.allow.is_match(to) && !self.from.is_match(to)
    }
}

impl Rule for BoundaryViolation {
    fn id(&self) -> &'static str {
        "boundary-violation"
    }
    fn description(&self) -> &'static str {
        "Import that crosses a direction the config forbids"
    }
    fn scope(&self) -> Scope {
        Scope::Graph
    }
    fn category(&self) -> Category {
        Category::Erosion
    }
    fn default_severity(&self) -> Severity {
        Severity::High
    }
    fn confidence(&self) -> Confidence {
        Confidence::High
    }

    fn run(&self, ctx: &RuleContext) -> anyhow::Result<Vec<Finding>> {
        if ctx.config.boundaries.is_empty() {
            return Ok(vec![]);
        }
        let compiled = compile(&ctx.config.boundaries)?;
        let mut out: Vec<Finding> = Vec::new();
        for e in edges::resolved(ctx.index)? {
            let Some(to) = e.to_rel.as_deref() else { continue };
            for c in &compiled {
                if !c.violated(&e.from_rel, to) {
                    continue;
                }
                let anchor = format!("{}\x1f{}", c.label, e.specifier);
                // Several names on one import line are one violation.
                if out.iter().any(|f| f.file == e.from_rel && f.span.start_line == e.line && f.related[0] == to) {
                    continue;
                }
                let evidence =
                    format!("{} imports \"{}\" ({}), which the boundary `{}` forbids", e.from_rel, e.specifier, to, c.label);
                let fix = format!("Move the shared code somewhere both sides may import, or change the boundary in {CONFIG_FILE}");
                let span = Span { start_line: e.line, start_col: 0, end_line: e.line, end_col: 0 };
                let mut f = finding_at(self, &e.from_rel, span, &anchor, &evidence, &fix);
                f.related = vec![to.to_string()];
                out.push(f);
            }
        }
        Ok(out)
    }
}
```

Register: `pub mod boundary;`, `Box::new(boundary::BoundaryViolation)`, registry test expects the full list `["leftover-debug", "leftover-commented-code", "leftover-agent-marker", "unused-import", "unreachable", "dead-export", "dead-file", "boundary-violation"]`.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p locrin-rules`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
cargo fmt --all
git add crates/rules/src/boundary.rs crates/rules/src/lib.rs crates/rules/tests/boundary.rs crates/rules/tests/fixtures/boundary
git commit -m "engine: boundary-violation rule from config"
```

### Task 15: CLI fixture, end-to-end tests, benchmarks, precision check

**Files:**
- Modify: `crates/cli/tests/fixtures/repo/package.json`, `crates/cli/tests/cli.rs`
- Create: `crates/cli/tests/fixtures/repo/src/index.ts`, `docs/superpowers/plans/2026-09-08-graph-rules-precision.md`

- [ ] **Step 1: Make the fixture a small real project**

`crates/cli/tests/fixtures/repo/package.json`: `{ "name": "repo", "main": "src/index.ts" }`

`crates/cli/tests/fixtures/repo/src/index.ts`:
```ts
import { ok } from "./clean";
import { bad } from "./dirty";
export const total = ok() + bad();
```

Now `clean.ts` and `dirty.ts` are imported, `index.ts` is the package main, and the full check is back to exactly the two plan-1 findings.

- [ ] **Step 2: Run the CLI tests**

Run: `cargo test -p locrin-cli`
Expected: every existing test passes again.

- [ ] **Step 3: Add end-to-end tests** (append to `crates/cli/tests/cli.rs`)

```rust
/// Removing the only import of an export makes that export dead in a file the
/// edit never touched. A named-path check reports on the named file only, so the
/// dead export shows up on a full check; part B widens narrowed checks to the
/// neighbours of what changed.
#[test]
fn removing_an_import_surfaces_a_dead_export_on_a_full_check() {
    let dir = copy_fixture();
    locrin(dir.path()).arg("check").output().unwrap();
    std::fs::write(dir.path().join("src/index.ts"), "import { ok } from \"./clean\";\nexport const total = ok();\n")
        .unwrap();
    let out = locrin(dir.path()).args(["check", "--json"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let dead: Vec<&str> = v["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["rule"] == "dead-export")
        .map(|f| f["file"].as_str().unwrap())
        .collect();
    assert_eq!(dead, vec!["src/dirty.ts"], "{v}");
}

#[test]
fn unused_import_blocks_and_names_the_binding() {
    let dir = copy_fixture();
    std::fs::write(
        dir.path().join("src/index.ts"),
        "import { ok } from \"./clean\";\nimport { bad } from \"./dirty\";\nexport const total = ok();\n",
    )
    .unwrap();
    let out = locrin(dir.path()).args(["check", "src/index.ts"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("unused-import"), "{text}");
    assert!(text.contains("`bad` is imported from \"./dirty\" but never used"), "{text}");
}

#[test]
fn boundaries_from_config_block() {
    let dir = copy_fixture();
    std::fs::write(
        dir.path().join("locrin.toml"),
        "[[boundaries]]\nname = \"index stays off dirty\"\nfrom = \"src/index.ts\"\nforbid = [\"src/dirty.ts\"]\n",
    )
    .unwrap();
    let out = locrin(dir.path()).args(["check", "--json"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let rules: Vec<&str> = v["findings"].as_array().unwrap().iter().map(|f| f["rule"].as_str().unwrap()).collect();
    assert!(rules.contains(&"boundary-violation"), "{v}");
    assert_eq!(v["status"], "block");
}

#[test]
fn a_new_orphan_file_is_advisory_not_blocking() {
    let dir = copy_fixture();
    locrin(dir.path()).args(["baseline", "create"]).assert().success();
    std::fs::write(dir.path().join("src/orphan.ts"), "export const orphan = 1;\n").unwrap();
    let out = locrin(dir.path()).args(["check", "--json"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["status"], "advisory", "{v}");
    assert_eq!(v["findings"][0]["rule"], "dead-file");
    assert_eq!(v["findings"][0]["file"], "src/orphan.ts");
}
```

Run: `cargo test -p locrin-cli` and expect PASS.

- [ ] **Step 4: Re-run the benchmarks in release mode**

Run: `cargo test --release -p locrin-cli -- --ignored --nocapture`
Expected: all three pass. Record the three numbers in the commit body of Step 6. If cold index exceeds 5 s, the suspects in order are: `probe` touching the disk for every bare specifier (it must not; check the `External` early return), `EntryPoints::detect` or `Resolver::new` being built per file instead of per run, and `symbols::extract` being called twice per file (`record` and `anchor_for`). Fix the cause; do not raise the target.

- [ ] **Step 5: Precision check on FastLift (spec 4.1 gate, corpus substitute)**

Build release, run `locrin check --json` against `<home>/fasting-app` with a fresh `LOCRIN_CACHE_DIR`, and for each of the five new rules take the first 20 findings in verdict order (fewer if the rule produced fewer). For each, open the file and decide true positive or false positive by reading the code, not the rule. Write `docs/superpowers/plans/2026-09-08-graph-rules-precision.md` with one table per rule (file, line, verdict, one-line reason) and a summary line per rule: `N/20 true positives`. Also record the total count per rule and the repository-wide edge resolution counts (`SELECT resolution, count(*) FROM edges GROUP BY resolution` against the cache database), because a high unresolved count explains a low `dead-export` precision.

Gate: a rule at 17/20 or better ships as planned. A rule below 17/20 STOPS this task: report the table and the failure pattern to the founder and wait. Do not lower a rule's confidence or disable it on your own; that is the founder's call (spec 4.3 and the founder's ask-before-deciding rule).

- [ ] **Step 6: Commit, then open the Part A pull request**

```bash
cargo fmt --all
git add crates/cli/tests/fixtures/repo/package.json crates/cli/tests/fixtures/repo/src/index.ts crates/cli/tests/cli.rs docs/superpowers/plans/2026-09-08-graph-rules-precision.md
git commit -m "engine: graph rules end to end, benchmarks re-run, precision check on FastLift"
git push -u origin engine/graph
gh pr create --base main --title "Locrin engine: import graph and five rules (plan 2 part A)" --body-file docs/superpowers/plans/2026-09-08-graph-rules-precision.md
```

The PR body is the precision report, so the reviewer sees the numbers first.

---

## Part B: incremental check, git scope, SARIF (branch `engine/incremental`)

### Task 16: Findings cache and adjacency

After this task a warm `check` parses only changed files; unchanged files' file-rule findings come from `findings_cache`; graph rules still run every time (they are SQL over the index, milliseconds); a narrowed check (`--changed`, named paths, and from Task 17 `--base`/`--since`) reports graph findings for the scope plus the files whose edges touch it. `scan` warms the cache so the first hook check after `init` is warm.

**Files:**
- Create: `crates/core/src/cache.rs`
- Modify: `crates/core/src/lib.rs`, `crates/cli/src/run.rs`, `crates/cli/src/main.rs` (help text only), `crates/cli/tests/cli.rs`

**Interfaces:**
- Produces:
  ```rust
  pub fn cache::config_hash(config: &Config) -> String                 // blake3(engine version + "\x1f" + toml of config)
  pub struct CachedFile { pub content_hash: String, pub config_hash: String, pub by_rule: HashMap<String, Vec<Finding>> }
  pub fn cache::load_all(ix: &Index) -> Result<HashMap<String, CachedFile>>
  pub fn cache::put(ix: &mut Index, rel: &str, content_hash: &str, config_hash: &str, rule: &str, findings: &[Finding]) -> Result<()>
  pub fn cache::clear(ix: &mut Index, rel: &str) -> Result<()>
  ```
  A cached entry is valid for a file when its `content_hash` equals the file's current hash and its `config_hash` equals this run's. Cached findings are post-`run_rules` (gate applied, severity override applied), so they can be served as they are.

- [ ] **Step 1: Write `cache.rs` with its tests**

```rust
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
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?))
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
```

Add `pub mod cache;` to `lib.rs`. Run `cargo test -p locrin-core cache` and expect PASS.

- [ ] **Step 2: Write the failing end-to-end tests** (append to `crates/cli/tests/cli.rs`)

```rust
/// The dead export lives in a file the edit never touched. A `--changed` run
/// must still report it, because the changed file's old edge reached it.
#[test]
fn changed_only_reports_graph_findings_on_neighbours() {
    let dir = copy_fixture();
    locrin(dir.path()).arg("check").output().unwrap();
    std::fs::write(dir.path().join("src/index.ts"), "import { ok } from \"./clean\";\nexport const total = ok();\n")
        .unwrap();
    let out = locrin(dir.path()).args(["check", "--changed", "--json"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let files: Vec<&str> = v["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["rule"] == "dead-export")
        .map(|f| f["file"].as_str().unwrap())
        .collect();
    assert_eq!(files, vec!["src/dirty.ts"], "{v}");
    // The file rule finding on dirty.ts (console.log) is NOT in a --changed run: dirty.ts did not change.
    assert!(!v["findings"].as_array().unwrap().iter().any(|f| f["rule"] == "leftover-debug"), "{v}");
}

/// After a scan, a full check must not re-parse anything: the cache answers for
/// every unchanged file. Observable through the index: `scan` then `check` leaves
/// exactly one cache row per (file, file rule), and a `check` after an edit to
/// one file rewrites only that file's rows.
#[test]
fn full_check_serves_unchanged_files_from_the_cache() {
    let dir = copy_fixture();
    locrin(dir.path()).arg("scan").assert().success();
    let db = walkdir(&dir.path().join(".cache")).into_iter().find(|p| p.ends_with("index.db")).unwrap();
    let count = |sql: &str| -> i64 {
        let c = rusqlite::Connection::open(&db).unwrap();
        c.query_row(sql, [], |r| r.get(0)).unwrap()
    };
    let per_file = count("SELECT count(DISTINCT rule) FROM findings_cache WHERE rel = 'src/clean.ts'");
    assert!(per_file >= 5, "scan warms every file rule, got {per_file}");
    let before = count("SELECT count(*) FROM findings_cache");

    let out = locrin(dir.path()).arg("check").output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8(out.stdout).unwrap().starts_with("BLOCK  2 finding(s)"));
    assert_eq!(count("SELECT count(*) FROM findings_cache"), before, "a warm check adds no rows");

    std::fs::write(dir.path().join("src/clean.ts"), "export function ok(): number {\n  debugger;\n  return 1;\n}\n").unwrap();
    locrin(dir.path()).arg("check").output().unwrap();
    let stale = count(
        "SELECT count(*) FROM findings_cache c JOIN files f ON f.rel = c.rel WHERE f.content_hash <> c.content_hash",
    );
    assert_eq!(stale, 0, "every cache row must carry its file's current hash");
}

/// A severity override changes what the cache may serve.
#[test]
fn config_change_invalidates_the_cache() {
    let dir = copy_fixture();
    locrin(dir.path()).arg("check").output().unwrap();
    std::fs::write(dir.path().join("locrin.toml"), "[rules.leftover-debug]\nseverity = \"low\"\n").unwrap();
    let out = locrin(dir.path()).args(["check", "--json"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let debug = v["findings"].as_array().unwrap().iter().find(|f| f["rule"] == "leftover-debug").unwrap();
    assert_eq!(debug["severity"], "low", "{v}");
}
```

Add `rusqlite = { workspace = true }` to `[dev-dependencies]` in `crates/cli/Cargo.toml`.

Run: `cargo test -p locrin-cli` and expect the three new tests to FAIL.

- [ ] **Step 3: Rewrite the pipeline in `crates/cli/src/run.rs`**

Replace the file's contents above `pub fn check` with the following, and replace `scan` as shown after it. `check`, `baseline_create`, and `baseline_accept` stay as they are.

```rust
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::Context;
use locrin_core::baseline::Baseline;
use locrin_core::cache;
use locrin_core::config::Config;
use locrin_core::edges;
use locrin_core::entry::EntryPoints;
use locrin_core::finding::{Finding, Verdict};
use locrin_core::index::{content_hash, Index};
use locrin_core::indexer;
use locrin_core::lang::Language;
use locrin_core::parse::{parse_source, rel_path, ParsedFile};
use locrin_core::resolve::Resolver;
use locrin_core::walk::{canonical_path, canonical_root, source_files, WalkOptions};
use locrin_rules::{file_rules, graph_rules, run_rules, RuleContext};

pub struct Options {
    pub root: PathBuf,
    pub paths: Vec<PathBuf>,
    pub changed_only: bool,
    pub json: bool,
}

/// What one pass over the repository produced.
pub struct Run {
    pub findings: Vec<Finding>,
    pub files: usize,
    pub changed: usize,
}

/// The files named on the command line, canonical and inside the root, or None
/// when nothing was named. A directory expands to the walked files beneath it,
/// so the config's excludes still apply inside it; a file is taken as named,
/// excluded or not, because naming a file is an instruction.
fn explicit_files(root: &Path, paths: &[PathBuf], walked: &[PathBuf]) -> anyhow::Result<Option<Vec<PathBuf>>> {
    if paths.is_empty() {
        return Ok(None);
    }
    let mut out = Vec::new();
    for p in paths {
        let abs = if p.is_absolute() { p.clone() } else { root.join(p) };
        let canon = canonical_path(&abs).with_context(|| format!("no such path: {}", p.display()))?;
        if !canon.starts_with(root) {
            anyhow::bail!("path is outside the repository root: {}", p.display());
        }
        if canon.is_dir() {
            out.extend(walked.iter().filter(|f| f.starts_with(&canon)).cloned());
        } else if Language::from_path(&canon).is_some() {
            out.push(canon);
        }
    }
    out.sort();
    out.dedup();
    Ok(Some(out))
}

/// Files whose edges touch any file in `set`, in either direction, in the index as it is now.
fn neighbours(ix: &Index, set: &HashSet<String>) -> anyhow::Result<HashSet<String>> {
    let mut out = HashSet::new();
    for rel in set {
        out.extend(edges::from_file(ix, rel)?.into_iter().filter_map(|e| e.to_rel));
        out.extend(edges::dependents(ix, rel)?);
    }
    Ok(out)
}

/// One pass: walk, hash, parse what must be parsed, record when allowed, run the
/// file rules on the parsed files and the graph rules on the index, serve every
/// unchanged file's file-rule findings from the cache, and narrow the report to
/// the scope when there is one.
///
/// `record` is what separates a run that observes the repository from one that
/// merely reads it. The baseline commands read every file to find the finding
/// they were asked about; if they also stamped the hashes they saw, an edit made
/// between two commands would look already-seen and the next `--changed` check
/// would skip it. Only `check` and `scan` are entitled to move the watermark,
/// and only they write the cache.
///
/// Scope semantics: file-rule findings are reported for the scope exactly;
/// graph-rule findings are reported for the scope plus its neighbours before and
/// after re-indexing, because removing an import from a scoped file is what
/// makes an export dead in a file outside it (spec 3.2, the incremental rule).
fn pass(root: &Path, opts: &Options, record: bool) -> anyhow::Result<Run> {
    let config = Config::load(root)?;
    let walked = source_files(root, &WalkOptions { excludes: config.excludes.clone() })?;
    let explicit = explicit_files(root, &opts.paths, &walked)?;
    let mut candidates = walked;
    if let Some(e) = &explicit {
        candidates.extend(e.iter().cloned());
        candidates.sort();
        candidates.dedup();
    }
    let rels: Vec<String> = candidates.iter().map(|p| rel_path(root, p)).collect();
    let resolver = Resolver::new(root, rels.iter().cloned().collect());
    let mut ix = Index::open(root)?;
    let cfg_hash = cache::config_hash(&config);
    let cached = cache::load_all(&ix)?;
    let file_rules = file_rules();
    let enabled: Vec<&str> = file_rules.iter().map(|r| r.id()).filter(|id| config.rule_enabled(id)).collect();

    let mut explicit_scope: Option<HashSet<String>> =
        explicit.as_ref().map(|e| e.iter().map(|p| rel_path(root, p)).collect());
    let mut parsed: Vec<ParsedFile> = Vec::new();
    let mut served: Vec<Finding> = Vec::new();
    let mut changed_rels: HashSet<String> = HashSet::new();
    let mut before: HashSet<String> = HashSet::new();
    let mut changed = 0;

    for path in &candidates {
        let rel = rel_path(root, path);
        let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        let source = match String::from_utf8(bytes) {
            Ok(s) => s,
            Err(_) => {
                eprintln!("warning: {rel} is not valid UTF-8; skipped");
                continue;
            }
        };
        let hash = content_hash(&source);
        let is_changed = ix.changed(&rel, &hash)?;
        let in_scope = explicit_scope.as_ref().is_some_and(|s| s.contains(&rel));
        let hit = cached
            .get(&rel)
            .filter(|c| c.content_hash == hash && c.config_hash == cfg_hash && enabled.iter().all(|r| c.by_rule.contains_key(*r)));
        if !is_changed && !in_scope {
            if let Some(c) = hit {
                for r in &enabled {
                    served.extend(c.by_rule[*r].iter().cloned());
                }
                continue;
            }
        }
        let Some(file) = parse_source(path, &rel, source) else { continue };
        if file.has_error {
            eprintln!("warning: parse errors in {rel}; excluded from rules");
        }
        if is_changed {
            changed += 1;
            changed_rels.insert(rel.clone());
            if record {
                // The edges this file had before the edit: their targets may
                // have just lost their last importer.
                before.extend(edges::from_file(&ix, &rel)?.into_iter().filter_map(|e| e.to_rel));
                indexer::record(&mut ix, &file, &hash, &resolver)?;
                cache::clear(&mut ix, &rel)?;
            }
        }
        parsed.push(file);
    }
    if record {
        ix.remove_missing(&rels)?;
    }

    let entries = EntryPoints::detect(root, &config.entry_points)?;
    // Both rule sets run inside one block so the shared borrow of `ix` ends
    // before the cache writes below take it mutably.
    let (fresh, graph) = {
        let ctx = RuleContext { files: &parsed, config: &config, index: &ix, entries: &entries };
        (run_rules(&file_rules, &ctx)?, run_rules(&graph_rules(), &ctx)?)
    };
    if record {
        let mut by_file_rule: HashMap<(&str, &str), Vec<&Finding>> = HashMap::new();
        for f in &fresh {
            by_file_rule.entry((f.file.as_str(), f.rule.as_str())).or_default().push(f);
        }
        for file in &parsed {
            let hash = content_hash(&file.source);
            for r in &enabled {
                let fs: Vec<Finding> =
                    by_file_rule.get(&(file.rel.as_str(), *r)).map(|v| v.iter().map(|f| (*f).clone()).collect()).unwrap_or_default();
                cache::put(&mut ix, &file.rel, &hash, &cfg_hash, r, &fs)?;
            }
        }
    }

    // Narrow the report. `--changed` takes the changed set as its scope.
    if explicit_scope.is_none() && opts.changed_only {
        explicit_scope = Some(changed_rels.clone());
    }
    let mut findings: Vec<Finding> = fresh.into_iter().chain(served).collect();
    let mut graph = graph;
    if let Some(scope) = &explicit_scope {
        findings.retain(|f| scope.contains(&f.file));
        let mut wide = scope.clone();
        wide.extend(before);
        wide.extend(neighbours(&ix, scope)?);
        graph.retain(|f| wide.contains(&f.file));
    }
    findings.extend(graph);
    Ok(Run { findings, files: candidates.len(), changed })
}

/// Every current finding for a run, with the index updated when `record`.
fn full_findings(root: &Path, opts: &Options, record: bool) -> anyhow::Result<Vec<Finding>> {
    Ok(pass(root, opts, record)?.findings)
}
```

And `scan`:

```rust
/// Indexes the repository and warms the findings cache, so the first `check`
/// after it (a hook, say) pays for nothing but the changed files.
pub fn scan(root: &Path) -> anyhow::Result<(usize, usize)> {
    let root = canonical_root(root);
    let opts = Options { root: root.clone(), paths: vec![], changed_only: false, json: false };
    let run = pass(&root, &opts, true)?;
    Ok((run.files, run.changed))
}
```

In `crates/cli/src/main.rs` change the `--changed` doc comment to:

```rust
        /// Only files whose content changed since the last index, plus the
        /// graph findings their edges reach.
```

and the `Scan` doc comment to `/// Index the repository and warm the findings cache without printing a verdict`.

- [ ] **Step 4: Run every test**

Run: `cargo test --workspace`
Expected: PASS, including the three new CLI tests and every plan-1 CLI test (`baseline_create_then_check_passes_and_changed_only_sees_edits` in particular: after `baseline create`, a `check` passes; a `--changed` with no edits is `PASS 0 finding(s)`; the edit shows up).

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add crates/core/src/cache.rs crates/core/src/lib.rs crates/cli/src/run.rs crates/cli/src/main.rs crates/cli/Cargo.toml crates/cli/tests/cli.rs Cargo.lock
git commit -m "engine: findings cache, neighbour scope for graph findings, scan warms the cache"
```

### Task 17: `check --base <ref>` and `check --since <ref>`

**Files:**
- Create: `crates/cli/src/git.rs`
- Modify: `crates/cli/src/main.rs`, `crates/cli/src/run.rs`, `crates/cli/tests/cli.rs`

**Interfaces:**
- Produces:
  ```rust
  pub enum DiffScope { Base(String), Since(String) }
  pub fn git::changed_files(root: &Path, scope: &DiffScope) -> anyhow::Result<Vec<String>>   // repo-relative, sorted, existing files only
  // Options gains: pub diff: Option<DiffScope>
  ```
  `Base(ref)`: files that differ between the working tree and `git merge-base <ref> HEAD`, plus untracked files not ignored. This is the pull-request view (spec 8.1) and it includes uncommitted edits. `Since(ref)`: files changed by commits in `<ref>..HEAD`, committed work only. This is the deployment gate (spec 8.2). Both are filtered to files that exist on disk, live under `root`, and are in the walked set (so excludes apply). Deleted files are not checked; their disappearance is picked up by `remove_missing` and the graph rules. Git failures are engine errors (exit 2) that quote git's own message.

- [ ] **Step 1: Write the failing end-to-end test** (append to `crates/cli/tests/cli.rs`)

```rust
fn git(dir: &std::path::Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {:?}: {}", args, String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

#[test]
fn base_sees_the_working_tree_and_since_sees_only_commits() {
    let dir = copy_fixture();
    git(dir.path(), &["init", "-q"]);
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-qm", "init"]);
    let base = git(dir.path(), &["rev-parse", "HEAD"]);

    // dirty.ts blocks on a full check but is untouched by this diff.
    std::fs::write(dir.path().join("src/clean.ts"), "export function ok(): number {\n  debugger;\n  return 1;\n}\n").unwrap();
    let out = locrin(dir.path()).args(["check", "--base", &base, "--json"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let files: Vec<&str> = v["findings"].as_array().unwrap().iter().map(|f| f["file"].as_str().unwrap()).collect();
    assert_eq!(files, vec!["src/clean.ts"], "{v}");
    assert_eq!(v["status"], "block");

    let out = locrin(dir.path()).args(["check", "--since", &base]).output().unwrap();
    assert!(String::from_utf8(out.stdout).unwrap().starts_with("PASS  0 finding(s)"), "uncommitted work is invisible to --since");

    git(dir.path(), &["commit", "-qam", "edit"]);
    let out = locrin(dir.path()).args(["check", "--since", &base, "--json"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["findings"][0]["file"], "src/clean.ts", "{v}");

    // An untracked file is part of the working tree view.
    std::fs::write(dir.path().join("src/new.ts"), "export const n = 1;\nconsole.log(n);\n").unwrap();
    let out = locrin(dir.path()).args(["check", "--base", &base, "--json"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(v["findings"].as_array().unwrap().iter().any(|f| f["file"] == "src/new.ts"), "{v}");
}

#[test]
fn a_bad_ref_is_an_engine_error() {
    let dir = copy_fixture();
    git(dir.path(), &["init", "-q"]);
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-qm", "init"]);
    let out = locrin(dir.path()).args(["check", "--base", "no-such-ref"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(err.starts_with("error:"), "{err}");
    assert!(err.contains("no-such-ref"), "{err}");
}
```

Run: `cargo test -p locrin-cli` and expect the two new tests to FAIL (unknown argument `--base`).

- [ ] **Step 2: Write `crates/cli/src/git.rs`**

```rust
//! The two git-derived scopes: a pull request's working tree against its base
//! (`--base`) and the commits since a deploy tag (`--since`). Paths come back
//! from git relative to the repository top level, which may sit above the root
//! Locrin was pointed at, so every path is re-rooted here.

use std::path::Path;
use std::process::Command;

use anyhow::Context;
use locrin_core::parse::rel_path;
use locrin_core::walk::canonical_root;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffScope {
    Base(String),
    Since(String),
}

fn git(dir: &Path, args: &[&str]) -> anyhow::Result<Vec<u8>> {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .context("running git; is it installed and on PATH?")?;
    if !out.status.success() {
        anyhow::bail!("git {} failed: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(out.stdout)
}

fn nul_separated(bytes: &[u8]) -> Vec<String> {
    bytes.split(|b| *b == 0).filter(|s| !s.is_empty()).map(|s| String::from_utf8_lossy(s).into_owned()).collect()
}

pub fn changed_files(root: &Path, scope: &DiffScope) -> anyhow::Result<Vec<String>> {
    let top = String::from_utf8(git(root, &["rev-parse", "--show-toplevel"])?)?.trim().to_string();
    let top = canonical_root(Path::new(&top));
    let names = match scope {
        DiffScope::Base(r) => {
            let mb = String::from_utf8(git(&top, &["merge-base", r, "HEAD"])?)?.trim().to_string();
            let mut v = nul_separated(&git(&top, &["diff", "--name-only", "--diff-filter=ACMR", "-z", &mb])?);
            v.extend(nul_separated(&git(&top, &["ls-files", "--others", "--exclude-standard", "-z"])?));
            v
        }
        DiffScope::Since(r) => nul_separated(&git(&top, &["diff", "--name-only", "--diff-filter=ACMR", "-z", r, "HEAD"])?),
    };
    let mut out: Vec<String> = names
        .into_iter()
        .filter_map(|n| {
            let abs = top.join(&n);
            (abs.is_file() && abs.starts_with(root)).then(|| rel_path(root, &abs))
        })
        .collect();
    out.sort();
    out.dedup();
    Ok(out)
}
```

- [ ] **Step 3: Wire it into `run.rs` and `main.rs`**

`run.rs`: add `pub diff: Option<crate::git::DiffScope>` to `Options`, and in `pass`, right after `explicit_scope` is first computed, add:

```rust
    if let Some(diff) = &opts.diff {
        let listed: HashSet<String> = crate::git::changed_files(root, diff)?.into_iter().collect();
        let walked: HashSet<&str> = rels.iter().map(|s| s.as_str()).collect();
        // Excludes apply to a diff-derived scope: generated code in a PR is still generated code.
        let scope: HashSet<String> = listed.into_iter().filter(|r| walked.contains(r.as_str())).collect();
        explicit_scope = Some(scope);
    }
```

Every `Options { ... }` literal in `run.rs` (`scan`, `baseline_create`, `baseline_accept`) gains `diff: None`.

`main.rs`: add `mod git;`, and to `Cmd::Check`:

```rust
        /// Files that differ from the merge base with REF, plus untracked files (the pull-request view)
        #[arg(long, value_name = "REF", conflicts_with_all = ["changed", "since"])]
        base: Option<String>,
        /// Files changed by the commits in REF..HEAD (the deployment gate)
        #[arg(long, value_name = "REF", conflicts_with_all = ["changed", "base"])]
        since: Option<String>,
```

and in `real_main`:

```rust
        Cmd::Check { paths, changed, json, base, since } => {
            let diff = base.map(git::DiffScope::Base).or(since.map(git::DiffScope::Since));
            let opts = run::Options { root, paths, changed_only: changed, json, diff };
```

- [ ] **Step 4: Run every test**

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add crates/cli/src/git.rs crates/cli/src/main.rs crates/cli/src/run.rs crates/cli/tests/cli.rs
git commit -m "engine: check --base and --since take the scope from git"
```

### Task 18: SARIF 2.1.0 reporter

**Files:**
- Create: `crates/reporters/src/sarif.rs`
- Modify: `crates/reporters/src/lib.rs`, `crates/cli/src/main.rs`, `crates/cli/tests/cli.rs`

**Interfaces:**
- Produces:
  ```rust
  pub struct RuleMeta { pub id: String, pub description: String, pub severity: Severity, pub category: Category }
  pub fn sarif::render(v: &Verdict, rules: &[RuleMeta], version: &str) -> String   // pretty JSON, full verdict, never capped
  ```
  Level mapping: High to `error`, Medium to `warning`, Low to `note`. Columns are 1-based in SARIF, so `start_col + 1` and `end_col + 1`. `uriBaseId` is `%SRCROOT%`. The finding id goes in `partialFingerprints["locrin/id"]` so GitHub code scanning keeps an alert stable across line shifts. `properties` carries `confidence`, `category`, `related`, and `owasp`/`cwe` when present.

- [ ] **Step 1: Write the module with its tests**

```rust
//! SARIF 2.1.0 for GitHub code scanning and third-party tools (spec 7.3). The
//! full verdict, never the agent cap: a CI consumer wants everything.

use locrin_core::finding::{Category, Finding, Severity, Verdict};
use serde_json::{json, Value};

pub struct RuleMeta {
    pub id: String,
    pub description: String,
    pub severity: Severity,
    pub category: Category,
}

fn level(s: Severity) -> &'static str {
    match s {
        Severity::High => "error",
        Severity::Medium => "warning",
        Severity::Low => "note",
    }
}

fn result(f: &Finding, rule_index: Option<usize>) -> Value {
    let mut r = json!({
        "ruleId": f.rule,
        "level": level(f.severity),
        "message": { "text": format!("{} Fix: {}", f.evidence, f.fix) },
        "locations": [{
            "physicalLocation": {
                "artifactLocation": { "uri": f.file, "uriBaseId": "%SRCROOT%" },
                "region": {
                    "startLine": f.span.start_line,
                    "startColumn": f.span.start_col + 1,
                    "endLine": f.span.end_line,
                    "endColumn": f.span.end_col + 1
                }
            }
        }],
        "partialFingerprints": { "locrin/id": f.id },
        "properties": { "confidence": f.confidence, "category": f.category, "related": f.related }
    });
    if let Some(i) = rule_index {
        r["ruleIndex"] = json!(i);
    }
    if let Some(o) = &f.owasp {
        r["properties"]["owasp"] = json!(o);
    }
    if let Some(c) = &f.cwe {
        r["properties"]["cwe"] = json!(c);
    }
    r
}

pub fn render(v: &Verdict, rules: &[RuleMeta], version: &str) -> String {
    let results: Vec<Value> =
        v.findings.iter().map(|f| result(f, rules.iter().position(|r| r.id == f.rule))).collect();
    let rule_objects: Vec<Value> = rules
        .iter()
        .map(|r| {
            json!({
                "id": r.id,
                "name": r.id,
                "shortDescription": { "text": r.description },
                "defaultConfiguration": { "level": level(r.severity) },
                "properties": { "category": r.category }
            })
        })
        .collect();
    let doc = json!({
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": [{
            "tool": { "driver": {
                "name": "locrin",
                "version": version,
                "informationUri": "https://locrin.com",
                "rules": rule_objects
            } },
            "invocations": [{ "executionSuccessful": true, "exitCode": v.exit_code() }],
            "results": results
        }]
    });
    serde_json::to_string_pretty(&doc).expect("sarif serialises")
}

#[cfg(test)]
mod tests {
    use super::*;
    use locrin_core::finding::*;

    fn f(rule: &str, sev: Severity, line: u32) -> Finding {
        Finding {
            id: format!("{:016x}", line),
            rule: rule.into(),
            category: Category::Erosion,
            severity: sev,
            confidence: Confidence::High,
            file: "src/a.ts".into(),
            span: Span { start_line: line, start_col: 2, end_line: line, end_col: 9 },
            evidence: "console.log(1)".into(),
            fix: "Remove it".into(),
            related: vec!["src/b.ts".into()],
            owasp: None,
            cwe: Some("CWE-1".into()),
        }
    }

    fn rules() -> Vec<RuleMeta> {
        vec![
            RuleMeta { id: "leftover-debug".into(), description: "d1".into(), severity: Severity::High, category: Category::Erosion },
            RuleMeta { id: "dead-export".into(), description: "d2".into(), severity: Severity::Low, category: Category::Erosion },
        ]
    }

    #[test]
    fn renders_a_full_uncapped_sarif_log() {
        let many: Vec<Finding> = (0..15).map(|i| f("leftover-debug", Severity::High, i + 1)).collect();
        let v = Verdict::from_findings(many, 3);
        let doc: Value = serde_json::from_str(&render(&v, &rules(), "0.1.0")).unwrap();
        assert_eq!(doc["version"], "2.1.0");
        assert_eq!(doc["$schema"], "https://json.schemastore.org/sarif-2.1.0.json");
        let run = &doc["runs"][0];
        assert_eq!(run["tool"]["driver"]["name"], "locrin");
        assert_eq!(run["tool"]["driver"]["rules"].as_array().unwrap().len(), 2);
        assert_eq!(run["results"].as_array().unwrap().len(), 15, "never capped");
        assert_eq!(run["invocations"][0]["exitCode"], 1);
    }

    #[test]
    fn maps_levels_columns_fingerprints_and_properties() {
        let v = Verdict::from_findings(vec![f("dead-export", Severity::Low, 4), f("leftover-debug", Severity::Medium, 2)], 1);
        let doc: Value = serde_json::from_str(&render(&v, &rules(), "0.1.0")).unwrap();
        let results = doc["runs"][0]["results"].as_array().unwrap();
        let by_rule = |id: &str| results.iter().find(|r| r["ruleId"] == id).unwrap().clone();
        let low = by_rule("dead-export");
        assert_eq!(low["level"], "note");
        assert_eq!(low["ruleIndex"], 1);
        let region = &low["locations"][0]["physicalLocation"]["region"];
        assert_eq!((region["startLine"].as_u64(), region["startColumn"].as_u64(), region["endColumn"].as_u64()), (Some(4), Some(3), Some(10)));
        assert_eq!(low["locations"][0]["physicalLocation"]["artifactLocation"]["uriBaseId"], "%SRCROOT%");
        assert_eq!(low["partialFingerprints"]["locrin/id"], "0000000000000004");
        assert_eq!(low["properties"]["cwe"], "CWE-1");
        assert_eq!(low["properties"]["related"][0], "src/b.ts");
        assert!(low["properties"].get("owasp").is_none());
        assert_eq!(by_rule("leftover-debug")["level"], "warning", "level follows the finding's severity, not the rule default");
    }
}
```

Add `pub mod sarif;` to `crates/reporters/src/lib.rs`. Run `cargo test -p locrin-reporters` and expect PASS.

- [ ] **Step 2: Add `--sarif` to the CLI**

In `main.rs` `Cmd::Check` add:

```rust
        /// SARIF 2.1.0 on stdout, every finding, for code scanning uploads
        #[arg(long, conflicts_with = "json")]
        sarif: bool,
```

and in `real_main`'s `Check` arm, before the `if opts.json` branch:

```rust
            if sarif {
                let rules: Vec<locrin_reporters::sarif::RuleMeta> = locrin_rules::all_rules()
                    .iter()
                    .map(|r| locrin_reporters::sarif::RuleMeta {
                        id: r.id().to_string(),
                        description: r.description().to_string(),
                        severity: r.default_severity(),
                        category: r.category(),
                    })
                    .collect();
                println!("{}", locrin_reporters::sarif::render(&verdict, &rules, env!("CARGO_PKG_VERSION")));
                return Ok(verdict.exit_code());
            }
```

(`sarif` is a new field in the `Cmd::Check { .. }` pattern; add it there.)

- [ ] **Step 3: End-to-end test** (append to `crates/cli/tests/cli.rs`)

```rust
#[test]
fn sarif_output_lists_every_rule_and_every_finding() {
    let dir = copy_fixture();
    let out = locrin(dir.path()).args(["check", "--sarif"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let doc: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["version"], "2.1.0");
    let run = &doc["runs"][0];
    assert_eq!(run["tool"]["driver"]["rules"].as_array().unwrap().len(), 8);
    assert_eq!(run["results"].as_array().unwrap().len(), 2);
    assert_eq!(run["results"][0]["locations"][0]["physicalLocation"]["artifactLocation"]["uri"], "src/dirty.ts");
}
```

Run: `cargo test --workspace` and expect PASS.

- [ ] **Step 4: Commit**

```bash
cargo fmt --all
git add crates/reporters/src/sarif.rs crates/reporters/src/lib.rs crates/cli/src/main.rs crates/cli/tests/cli.rs
git commit -m "engine: SARIF 2.1.0 reporter behind check --sarif"
```

### Task 19: Warm-diff benchmark and the Part B pull request

**Files:**
- Modify: `crates/cli/tests/bench.rs`

- [ ] **Step 1: Add the spec 3.4 warm-diff benchmark**

Append to `bench.rs`:

```rust
/// Spec 3.4: a warm diff check on a typical pull request (30 files) under 1 s.
#[test]
#[ignore]
fn warm_thirty_file_check_under_one_second() {
    let _serial = serial();
    let cache = tempfile::tempdir().unwrap();
    locrin(cache.path()).arg("scan").assert().success();
    let dir = repo().join("app");
    let mut files: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().map(|x| x == "tsx" || x == "ts").unwrap_or(false))
        .map(|p| format!("app/{}", p.file_name().unwrap().to_string_lossy()))
        .collect();
    files.sort();
    files.truncate(30);
    assert!(files.len() >= 10, "bench repo has too few files under app/ to stand in for a PR: {}", files.len());
    let t = Instant::now();
    let out = locrin(cache.path()).arg("check").args(&files).output().unwrap();
    let ms = t.elapsed().as_millis();
    println!("warm {}-file check: {ms} ms", files.len());
    assert!(
        out.status.code() != Some(2),
        "check failed with exit 2, so the {ms} ms is not a real measurement: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(ms < 1_000, "warm diff check took {ms} ms");
}
```

If `app/` in the bench repo holds fewer than 10 source files at its top level, widen the listing to `app/**` one directory down rather than lowering the count.

- [ ] **Step 2: Run all four benchmarks in release mode**

Run: `cargo test --release -p locrin-cli -- --ignored --nocapture`
Expected: all four pass. Record the numbers in the commit body. The warm single-file check should now be faster than Part A's number, because a full-repository parse no longer happens on any warm run.

- [ ] **Step 3: Full workspace test, commit, pull request**

```bash
cargo test --workspace
cargo fmt --all
git add crates/cli/tests/bench.rs
git commit -m "engine: warm thirty-file benchmark for the spec diff target"
git push -u origin engine/incremental
gh pr create --base main --title "Locrin engine: findings cache, git scope, SARIF (plan 2 part B)" --body "Part B of plan 2: findings_cache serves unchanged files, --changed and named paths report graph findings for their neighbours, check --base and --since take the scope from git, check --sarif renders SARIF 2.1.0. Benchmarks (release): cold scan, warm single-file, warm 30-file, startup: see commit bodies."
```

If Part A has not merged yet, the PR base is `engine/graph`; retarget it to `main` before merging Part A with `--delete-branch`.

---

## Self-review notes (already applied)

- Spec 3.2 "reverse-import dependents are re-evaluated for cross-file rules only": implemented as `neighbours` (both directions) plus the `before` set in Task 16. Both directions because a dead export appears in the file that *lost* an importer (forward edge of the changed file), while an unresolved import appears in the file that *imports* the changed file (reverse edge); the spec names one, the engine needs both.
- Spec 3.2 lists `call` among edge kinds. No rule in version one reads call edges (they serve `find_existing` ranking and `already-exists`, plan 4 and release two), so this plan records import, re-export, dynamic, and require edges only; the `kind` column takes `call` without a schema change when the time comes.
- Spec 3.2 `findings_cache: file hash, rule id, serialized findings`: Task 1 table, Task 16 API; the config hash column is an addition the spec does not name but a severity override makes necessary.
- Spec 4.1 `dead-export` entry points "from package.json (main, exports, bin), framework conventions (Next.js app/ and pages/, Expo Router app/), and config": Task 7 and Task 8.
- Spec 7.5 config sections: `entry points` and `boundaries` land here; `blocking policy` and `framework hints` are plan 3 with the rules that read them.
- Spec 8.1 posts a PR comment and uploads SARIF from a GitHub Action: the binary side (`--base`, `--sarif`) is here; the Action itself is phase two.
- Spec 9 hook timeout and the debug log path: plan 4.
- Types: `Symbol.export_name: Option<String>` (Task 2) is what `symbols::exported` returns and `dead_export` reads; `Edge.to_rel: Option<String>` (Task 6) is what `edges::resolved`, `imported_names`, `neighbours`, and `boundary` read; `Rule::run -> anyhow::Result<Vec<Finding>>` from Task 9 onward, `run_rules` likewise; `RuleContext` has exactly four fields from Task 9 onward and every construction site in this plan names all four.
