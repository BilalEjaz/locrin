# Engine Core Implementation Plan (version one, plan 1 of 4)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A single Rust binary that walks a TypeScript repository, indexes it incrementally in SQLite, runs a first set of deterministic rules over changed files, and prints a verdict for a human or an agent in under 300 ms on a warm single-file check.

**Architecture:** One Cargo workspace with four crates: `core` (walk, parse, index, symbols, finding contract), `rules` (a `Rule` trait, a registry, and three leftover rules), `reporters` (terminal and agent JSON), `cli` (the `gate` binary with `check`, `scan`, and `baseline`). Rules are pure functions over a `RuleContext`; the index is the only state and it lives in the user cache directory, never in the repo.

**Tech Stack:** Rust 2021 edition (stable, 1.80 or newer), tree-sitter 0.23 with tree-sitter-typescript 0.23, rusqlite 0.32 (bundled SQLite), blake3, serde and serde_json, clap 4, ignore and globset, toml. Tests are plain `#[test]` functions with fixture repositories under `crates/*/tests/fixtures`.

**Spec:** `docs/superpowers/specs/2026-09-05-agent-native-quality-gate-design.md`, sections 3 (architecture, index, performance targets, baseline), 4.1 (the three leftover rules only in this plan), 7 (output contract, config, baseline, exit codes), and 9 (error handling).

**Scope of this plan and what follows.** This plan delivers the vertical slice: walk, parse, index, incremental change detection, three rules that need no cross-file graph (`leftover-debug`, `leftover-commented-code`, `leftover-agent-marker`), two reporters, the config file, the baseline file, and the benchmark tests. Plan 2 adds import edges and the graph rules (`unused-import`, `dead-export`, `dead-file`, `unreachable`, `boundary-violation`). Plan 3 adds `swallowed-error`, the test rules, and the security pack. Plan 4 adds `init`, the Claude Code hooks, and the MCP server. `already-exists` is release two per the spec.

## Global Constraints

- Binary and config names are placeholders until the product is named: binary `gate`, config file `gate.toml`, baseline file `gate-baseline.json`. They are defined once each (in `crates/cli/Cargo.toml` `[[bin]]`, `core::config::CONFIG_FILE`, `core::baseline::BASELINE_FILE`) so a rename is three edits.
- No LLM anywhere. No network access anywhere in this plan.
- Languages in scope: TypeScript (`.ts`), TSX (`.tsx`), JavaScript (`.js`, `.jsx`, `.mjs`, `.cjs`). `.d.ts` files are skipped.
- Performance targets are tests, not aspirations: cold index of `<home>/fasting-app` (about 1,900 TypeScript files including tests) under 5 s in release mode; warm single-file `check` under 300 ms; binary start to first output under 50 ms. Benchmark tests are `#[ignore]` and run with `cargo test --release -- --ignored`.
- Index lives at `<cache dir>/gate/<blake3 of canonical repo path, first 16 hex>/index.db`; cache dir from the `dirs` crate (`dirs::cache_dir()`), falling back to `<repo>/.gate-cache` if unavailable (and that folder is then gitignored by `init` in plan 4; here the fallback is only used in tests via an env var `GATE_CACHE_DIR`).
- Every finding carries: `id`, `rule`, `category`, `severity`, `confidence`, `file`, `span` (start_line, start_col, end_line, end_col, 1-based lines, 0-based cols), `evidence` (one line), `fix` (one line), `related` (list of symbol refs, may be empty), and optional `owasp` and `cwe` (both `None` for every rule in this plan).
- Finding id is stable across line shifts: `blake3(rule_id + "\x1f" + repo-relative file path + "\x1f" + anchor)` truncated to 16 hex chars, where `anchor` is the enclosing top-level symbol name when there is one, otherwise the trimmed text of the flagged line.
- Exit codes: 0 pass or advisory, 1 block, 2 engine error. Only high-confidence findings block. `leftover-debug` is high confidence; `leftover-commented-code` and `leftover-agent-marker` are medium confidence and therefore advisory.
- Agent JSON output is capped at the ten highest-severity findings, verdict first, never includes file contents, and reports the truncated count.
- Parse failure in a file: file recorded as unparsed, one warning on stderr, no findings from that file, exit code unaffected. Engine panic: exit 2 with a one-line message. Never exit 1 on an engine bug.
- Git: branch `engine/core` off `main` once the spike branch has merged; until then off `spike/fingerprint`. One commit per task, plain messages, no attribution trailers, never `git add -A`. No em dashes in any text.
- Rust toolchain: not installed on the founder's machine as of 2026-09-05. Task 1 verifies `cargo --version` and stops with NEEDS_CONTEXT if absent; the founder installs rustup (https://rustup.rs, stable toolchain, MSVC target on Windows).

## File structure

```
Cargo.toml                          workspace: members crates/*
crates/core/Cargo.toml
crates/core/src/lib.rs              pub mod lang, walk, parse, index, symbols, finding, config, baseline
crates/core/src/lang.rs             Language enum, from_path, grammar lookup
crates/core/src/walk.rs             source file discovery honoring .gitignore and excludes
crates/core/src/parse.rs            ParsedFile, parse_file, parse_source
crates/core/src/index.rs            Index (rusqlite), schema, open, upsert_file, changed_files, symbols table access
crates/core/src/symbols.rs          top-level symbol extraction into the index
crates/core/src/finding.rs          Finding, Span, Severity, Confidence, Category, Verdict, make_id
crates/core/src/config.rs           Config, load or defaults, CONFIG_FILE
crates/core/src/baseline.rs         Baseline, load, save, accept, BASELINE_FILE
crates/core/tests/fixtures/mini/    a tiny repo used by core tests
crates/rules/Cargo.toml
crates/rules/src/lib.rs             Rule trait, RuleContext, all_rules()
crates/rules/src/leftover_debug.rs
crates/rules/src/leftover_commented.rs
crates/rules/src/leftover_marker.rs
crates/rules/tests/fixtures/<rule>/{flag,clean,edge}/*.ts
crates/reporters/Cargo.toml
crates/reporters/src/lib.rs         pub mod terminal, agent
crates/reporters/src/terminal.rs
crates/reporters/src/agent.rs
crates/cli/Cargo.toml               [[bin]] name = "gate"
crates/cli/src/main.rs              clap commands: check, scan, baseline
crates/cli/src/run.rs               the pipeline: config -> walk -> index -> rules -> baseline filter -> verdict
crates/cli/tests/cli.rs             end-to-end tests against fixture repos
crates/cli/tests/bench.rs           ignored benchmark tests against fasting-app
```

Shared types, defined once in `core` and imported everywhere:

```rust
// core::lang
pub enum Language { TypeScript, Tsx, JavaScript }

// core::parse
pub struct ParsedFile { pub path: PathBuf, pub rel: String, pub language: Language, pub source: String, pub tree: tree_sitter::Tree, pub has_error: bool }

// core::finding
pub enum Severity { High, Medium, Low }
pub enum Confidence { High, Medium }
pub enum Category { Erosion, Security }
pub struct Span { pub start_line: u32, pub start_col: u32, pub end_line: u32, pub end_col: u32 }
pub struct Finding { pub id: String, pub rule: String, pub category: Category, pub severity: Severity, pub confidence: Confidence, pub file: String, pub span: Span, pub evidence: String, pub fix: String, pub related: Vec<String>, pub owasp: Option<String>, pub cwe: Option<String> }
pub enum Status { Pass, Advisory, Block }
pub struct Verdict { pub status: Status, pub high: usize, pub medium: usize, pub low: usize, pub blocking: usize, pub duration_ms: u128, pub findings: Vec<Finding>, pub truncated: usize }

// rules
pub struct RuleContext<'a> { pub files: &'a [ParsedFile], pub config: &'a core::config::Config }
pub trait Rule { fn id(&self) -> &'static str; fn category(&self) -> Category; fn run(&self, ctx: &RuleContext) -> Vec<Finding>; }
```

---

### Task 1: Workspace scaffold and toolchain check

**Files:**
- Create: `Cargo.toml`
- Create: `crates/core/Cargo.toml`, `crates/core/src/lib.rs`
- Create: `crates/rules/Cargo.toml`, `crates/rules/src/lib.rs`
- Create: `crates/reporters/Cargo.toml`, `crates/reporters/src/lib.rs`
- Create: `crates/cli/Cargo.toml`, `crates/cli/src/main.rs`
- Modify: `.gitignore`

**Interfaces:**
- Produces: a building workspace; `cargo test` passes one smoke test in `core`.

- [ ] **Step 1: Verify the toolchain**

```bash
cargo --version && rustc --version
```
Expected: both print a version 1.80 or newer. If `cargo` is not found, stop and report NEEDS_CONTEXT: the founder installs rustup from https://rustup.rs (stable, MSVC on Windows) and re-dispatches.

- [ ] **Step 2: Write the workspace manifests**

`Cargo.toml`:
```toml
[workspace]
resolver = "2"
members = ["crates/core", "crates/rules", "crates/reporters", "crates/cli"]

[workspace.package]
edition = "2021"
version = "0.1.0"
license = "MIT"

[workspace.dependencies]
anyhow = "1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
blake3 = "1"
tree-sitter = "0.23"
tree-sitter-typescript = "0.23"
rusqlite = { version = "0.32", features = ["bundled"] }
ignore = "0.4"
globset = "0.4"
toml = "0.8"
dirs = "5"
clap = { version = "4", features = ["derive"] }
```

`crates/core/Cargo.toml`:
```toml
[package]
name = "core"
edition.workspace = true
version.workspace = true
license.workspace = true

[lib]
name = "gate_core"
path = "src/lib.rs"

[dependencies]
anyhow.workspace = true
serde.workspace = true
serde_json.workspace = true
blake3.workspace = true
tree-sitter.workspace = true
tree-sitter-typescript.workspace = true
rusqlite.workspace = true
ignore.workspace = true
globset.workspace = true
toml.workspace = true
dirs.workspace = true
```

`crates/rules/Cargo.toml`:
```toml
[package]
name = "rules"
edition.workspace = true
version.workspace = true
license.workspace = true

[lib]
name = "gate_rules"
path = "src/lib.rs"

[dependencies]
gate_core = { package = "core", path = "../core" }
tree-sitter.workspace = true
globset.workspace = true
```

`crates/reporters/Cargo.toml`:
```toml
[package]
name = "reporters"
edition.workspace = true
version.workspace = true
license.workspace = true

[lib]
name = "gate_reporters"
path = "src/lib.rs"

[dependencies]
gate_core = { package = "core", path = "../core" }
serde.workspace = true
serde_json.workspace = true
```

`crates/cli/Cargo.toml`:
```toml
[package]
name = "cli"
edition.workspace = true
version.workspace = true
license.workspace = true

[[bin]]
name = "gate"
path = "src/main.rs"

[dependencies]
gate_core = { package = "core", path = "../core" }
gate_rules = { package = "rules", path = "../rules" }
gate_reporters = { package = "reporters", path = "../reporters" }
anyhow.workspace = true
clap.workspace = true
serde_json.workspace = true

[dev-dependencies]
assert_cmd = "2"
tempfile = "3"
```

- [ ] **Step 3: Write the minimal crate roots and the smoke test**

`crates/core/src/lib.rs`:
```rust
pub const ENGINE_NAME: &str = "gate";

#[cfg(test)]
mod tests {
    #[test]
    fn engine_has_a_name() {
        assert_eq!(super::ENGINE_NAME, "gate");
    }
}
```

`crates/rules/src/lib.rs`:
```rust
pub use gate_core;
```

`crates/reporters/src/lib.rs`:
```rust
pub use gate_core;
```

`crates/cli/src/main.rs`:
```rust
fn main() {
    println!("{}", gate_core::ENGINE_NAME);
}
```

Append to `.gitignore`:
```
target/
.gate-cache/
```

- [ ] **Step 4: Build and run the smoke test**

```bash
cd <repo> && cargo test -p core -q 2>&1 | tail -3
```
Expected: `test result: ok. 1 passed`. The first build downloads and compiles tree-sitter and bundled SQLite; allow several minutes.

- [ ] **Step 5: Commit**

```bash
cd <repo> && git add Cargo.toml Cargo.lock .gitignore crates/core/Cargo.toml crates/core/src/lib.rs crates/rules/Cargo.toml crates/rules/src/lib.rs crates/reporters/Cargo.toml crates/reporters/src/lib.rs crates/cli/Cargo.toml crates/cli/src/main.rs && git commit -m "engine: cargo workspace scaffold"
```

---

### Task 2: Language detection and parsing

**Files:**
- Create: `crates/core/src/lang.rs`
- Create: `crates/core/src/parse.rs`
- Modify: `crates/core/src/lib.rs`

**Interfaces:**
- Produces: `lang::Language` (`TypeScript`, `Tsx`, `JavaScript`), `Language::from_path(&Path) -> Option<Language>` (None for `.d.ts` and unknown extensions), `Language::grammar(&self) -> tree_sitter::Language`; `parse::ParsedFile` (fields above), `parse::parse_source(path: &Path, rel: &str, source: String) -> Option<ParsedFile>` (None when the language is unsupported), `parse::parse_file(root: &Path, path: &Path) -> anyhow::Result<Option<ParsedFile>>` (reads the file, computes `rel` as the forward-slash path relative to `root`).

- [ ] **Step 1: Write the failing tests**

Append to `crates/core/src/lang.rs` (create the file with the tests first; the implementation goes above them in Step 3):
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn detects_supported_extensions() {
        assert_eq!(Language::from_path(Path::new("a/b.ts")), Some(Language::TypeScript));
        assert_eq!(Language::from_path(Path::new("a/b.tsx")), Some(Language::Tsx));
        assert_eq!(Language::from_path(Path::new("a/b.js")), Some(Language::JavaScript));
        assert_eq!(Language::from_path(Path::new("a/b.mjs")), Some(Language::JavaScript));
        assert_eq!(Language::from_path(Path::new("a/b.cjs")), Some(Language::JavaScript));
        assert_eq!(Language::from_path(Path::new("a/b.jsx")), Some(Language::JavaScript));
    }

    #[test]
    fn skips_declarations_and_unknown() {
        assert_eq!(Language::from_path(Path::new("a/b.d.ts")), None);
        assert_eq!(Language::from_path(Path::new("a/b.py")), None);
        assert_eq!(Language::from_path(Path::new("a/README.md")), None);
    }
}
```

`crates/core/src/parse.rs` tests (same pattern, tests at the bottom of the file):
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn parses_typescript_and_reports_no_error() {
        let src = "export function add(a: number, b: number): number { return a + b; }\n".to_string();
        let p = parse_source(Path::new("x/add.ts"), "x/add.ts", src).unwrap();
        assert_eq!(p.language, Language::TypeScript);
        assert!(!p.has_error);
        assert_eq!(p.tree.root_node().kind(), "program");
    }

    #[test]
    fn parses_tsx_with_jsx() {
        let src = "export function Row({ item }: { item: string }) { return <div className=\"r\">{item}</div>; }\n".to_string();
        let p = parse_source(Path::new("x/Row.tsx"), "x/Row.tsx", src).unwrap();
        assert_eq!(p.language, Language::Tsx);
        assert!(!p.has_error);
    }

    #[test]
    fn flags_syntax_errors_without_panicking() {
        let src = "export function broken( { return 1;\n".to_string();
        let p = parse_source(Path::new("x/broken.ts"), "x/broken.ts", src).unwrap();
        assert!(p.has_error);
    }

    #[test]
    fn unsupported_language_is_none() {
        assert!(parse_source(Path::new("x/a.py"), "x/a.py", "print(1)".to_string()).is_none());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd <repo> && cargo test -p core -q 2>&1 | tail -5
```
Expected: compile errors, `Language` and `parse_source` not found.

- [ ] **Step 3: Implement lang.rs**

Prepend to `crates/core/src/lang.rs`:
```rust
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    TypeScript,
    Tsx,
    JavaScript,
}

impl Language {
    pub fn from_path(path: &Path) -> Option<Language> {
        let name = path.file_name()?.to_str()?;
        if name.ends_with(".d.ts") || name.ends_with(".d.mts") || name.ends_with(".d.cts") {
            return None;
        }
        match path.extension()?.to_str()? {
            "ts" | "mts" | "cts" => Some(Language::TypeScript),
            "tsx" => Some(Language::Tsx),
            "js" | "jsx" | "mjs" | "cjs" => Some(Language::JavaScript),
            _ => None,
        }
    }

    pub fn grammar(&self) -> tree_sitter::Language {
        match self {
            Language::TypeScript | Language::JavaScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Language::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Language::TypeScript => "typescript",
            Language::Tsx => "tsx",
            Language::JavaScript => "javascript",
        }
    }
}
```

Note: JavaScript files are parsed with the TypeScript grammar on purpose. The TypeScript grammar is a superset for the constructs this plan's rules look at, and it avoids a third grammar crate. Plan 2 revisits this if JSX-in-`.js` files show parse errors in the corpus.

- [ ] **Step 4: Implement parse.rs**

Prepend to `crates/core/src/parse.rs`:
```rust
use std::path::{Path, PathBuf};

use anyhow::Context;
use tree_sitter::{Parser, Tree};

use crate::lang::Language;

#[derive(Debug)]
pub struct ParsedFile {
    pub path: PathBuf,
    pub rel: String,
    pub language: Language,
    pub source: String,
    pub tree: Tree,
    pub has_error: bool,
}

pub fn parse_source(path: &Path, rel: &str, source: String) -> Option<ParsedFile> {
    let language = Language::from_path(path)?;
    let mut parser = Parser::new();
    parser
        .set_language(&language.grammar())
        .expect("grammar version matches tree-sitter runtime");
    let tree = parser.parse(source.as_bytes(), None)?;
    let has_error = tree.root_node().has_error();
    Some(ParsedFile { path: path.to_path_buf(), rel: rel.to_string(), language, source, tree, has_error })
}

pub fn rel_path(root: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or(path);
    rel.to_string_lossy().replace('\\', "/")
}

pub fn parse_file(root: &Path, path: &Path) -> anyhow::Result<Option<ParsedFile>> {
    if Language::from_path(path).is_none() {
        return Ok(None);
    }
    let source = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(parse_source(path, &rel_path(root, path), source))
}
```

`crates/core/src/lib.rs` becomes:
```rust
pub mod lang;
pub mod parse;

pub const ENGINE_NAME: &str = "gate";

#[cfg(test)]
mod tests {
    #[test]
    fn engine_has_a_name() {
        assert_eq!(super::ENGINE_NAME, "gate");
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

```bash
cd <repo> && cargo test -p core -q 2>&1 | tail -3
```
Expected: `7 passed`. If `LANGUAGE_TYPESCRIPT` is not found, the installed grammar crate is older than 0.23; check `cargo tree -p core | grep tree-sitter-typescript` and, if it is 0.21 or 0.22, use `tree_sitter_typescript::language_typescript()` and `language_tsx()` instead and record the change in the report.

- [ ] **Step 6: Commit**

```bash
cd <repo> && git add crates/core/src/lang.rs crates/core/src/parse.rs crates/core/src/lib.rs && git commit -m "engine: language detection and tree-sitter parsing"
```

---

### Task 3: Source file discovery

**Files:**
- Create: `crates/core/src/walk.rs`
- Create: `crates/core/tests/fixtures/mini/` (files listed below)
- Modify: `crates/core/src/lib.rs`

**Interfaces:**
- Consumes: `lang::Language::from_path`.
- Produces: `walk::DEFAULT_EXCLUDES: &[&str]`, `walk::WalkOptions { pub excludes: Vec<String> }` (globs, repo-relative), `walk::source_files(root: &Path, opts: &WalkOptions) -> anyhow::Result<Vec<PathBuf>>` sorted, absolute, honoring `.gitignore` via the `ignore` crate, skipping excluded globs and unsupported languages.

- [ ] **Step 1: Write the fixture repo**

`crates/core/tests/fixtures/mini/package.json`:
```json
{ "name": "mini", "main": "src/index.ts" }
```

`crates/core/tests/fixtures/mini/src/index.ts`:
```ts
import { helper } from "./util";
export function main(): number {
  console.log("starting");
  return helper(2);
}
```

`crates/core/tests/fixtures/mini/src/util.ts`:
```ts
export function helper(n: number): number {
  return n * 2;
}
export function unusedHelper(): void {}
```

`crates/core/tests/fixtures/mini/src/types.d.ts`:
```ts
export type Id = string;
```

`crates/core/tests/fixtures/mini/node_modules/dep/index.js`:
```js
module.exports = { x: 1 };
```

`crates/core/tests/fixtures/mini/dist/index.js`:
```js
console.log("built");
```

`crates/core/tests/fixtures/mini/.gitignore`:
```
dist/
```

Because the repo's own `.gitignore` may hide `node_modules` fixtures, force-add them in the commit step.

- [ ] **Step 2: Write the failing tests**

`crates/core/src/walk.rs` (tests at the bottom):
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mini")
    }

    #[test]
    fn finds_source_files_and_skips_ignored_and_declarations() {
        let files = source_files(&fixture(), &WalkOptions::default()).unwrap();
        let names: Vec<String> = files
            .iter()
            .map(|p| p.strip_prefix(fixture()).unwrap().to_string_lossy().replace('\\', "/"))
            .collect();
        assert_eq!(names, vec!["src/index.ts", "src/util.ts"]);
    }

    #[test]
    fn extra_excludes_apply() {
        let opts = WalkOptions { excludes: vec!["src/util.ts".into()] };
        let files = source_files(&fixture(), &opts).unwrap();
        assert_eq!(files.len(), 1);
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

```bash
cd <repo> && cargo test -p core -q 2>&1 | tail -5
```
Expected: compile error, `source_files` not found.

- [ ] **Step 4: Implement walk.rs**

Prepend to `crates/core/src/walk.rs`:
```rust
use std::path::{Path, PathBuf};

use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::WalkBuilder;

use crate::lang::Language;

pub const DEFAULT_EXCLUDES: &[&str] = &[
    "**/node_modules/**",
    "**/dist/**",
    "**/build/**",
    "**/coverage/**",
    "**/.expo/**",
    "**/android/**",
    "**/ios/**",
    "**/.git/**",
    "**/.gate-cache/**",
];

#[derive(Debug, Default, Clone)]
pub struct WalkOptions {
    pub excludes: Vec<String>,
}

fn build_globset(extra: &[String]) -> anyhow::Result<GlobSet> {
    let mut b = GlobSetBuilder::new();
    for g in DEFAULT_EXCLUDES.iter().map(|s| s.to_string()).chain(extra.iter().cloned()) {
        b.add(Glob::new(&g)?);
    }
    Ok(b.build()?)
}

pub fn source_files(root: &Path, opts: &WalkOptions) -> anyhow::Result<Vec<PathBuf>> {
    let excludes = build_globset(&opts.excludes)?;
    let mut out = Vec::new();
    for entry in WalkBuilder::new(root).hidden(false).git_ignore(true).build() {
        let entry = entry?;
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let path = entry.path();
        let rel = path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/");
        if excludes.is_match(&rel) {
            continue;
        }
        if Language::from_path(path).is_none() {
            continue;
        }
        out.push(path.to_path_buf());
    }
    out.sort();
    Ok(out)
}
```

Add `pub mod walk;` to `crates/core/src/lib.rs`.

- [ ] **Step 5: Run tests to verify they pass**

```bash
cd <repo> && cargo test -p core -q 2>&1 | tail -3
```
Expected: `9 passed`. If `dist/index.js` shows up, the fixture `.gitignore` is not being honoured because the fixture directory is not itself a git repository; in that case add `.require_git(false)` to the `WalkBuilder` chain (the `ignore` crate reads `.gitignore` files only inside git repos by default).

- [ ] **Step 6: Commit**

```bash
cd <repo> && git add crates/core/src/walk.rs crates/core/src/lib.rs crates/core/tests/fixtures/mini/package.json crates/core/tests/fixtures/mini/src crates/core/tests/fixtures/mini/.gitignore crates/core/tests/fixtures/mini/dist && git add -f crates/core/tests/fixtures/mini/node_modules && git commit -m "engine: source file discovery with gitignore and excludes"
```

---

### Task 4: Index schema, file table, and change detection

**Files:**
- Create: `crates/core/src/index.rs`
- Modify: `crates/core/src/lib.rs`

**Interfaces:**
- Produces: `index::Index` with `Index::open(repo_root: &Path) -> anyhow::Result<Index>` (location rule from Global Constraints, honours env `GATE_CACHE_DIR`), `Index::open_in_memory() -> anyhow::Result<Index>` for tests, `Index::upsert_file(&mut self, rel: &str, language: &str, content_hash: &str, parse_status: &str) -> anyhow::Result<()>`, `Index::file_hash(&self, rel: &str) -> anyhow::Result<Option<String>>`, `Index::changed(&self, rel: &str, content_hash: &str) -> anyhow::Result<bool>` (true when absent or hash differs), `Index::remove_missing(&mut self, present: &[String]) -> anyhow::Result<usize>` (drops files not in the list and their symbols), `index::content_hash(source: &str) -> String` (blake3 hex), `index::cache_path(repo_root: &Path) -> PathBuf`.
- Schema (executed on open, idempotent):

```sql
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
```
`meta` holds `schema_version = 1`. On a version mismatch the database file is deleted and recreated (spec section 9: rebuild on schema mismatch).

- [ ] **Step 1: Write the failing tests**

`crates/core/src/index.rs` (tests at the bottom):
```rust
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
        std::env::remove_var("GATE_CACHE_DIR");
        let p = cache_path(std::path::Path::new("C:/repo/one"));
        let q = cache_path(std::path::Path::new("C:/repo/two"));
        assert_ne!(p, q);
        assert!(p.ends_with("index.db"));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd <repo> && cargo test -p core -q 2>&1 | tail -5
```
Expected: compile error, `Index` not found.

- [ ] **Step 3: Implement index.rs**

Prepend to `crates/core/src/index.rs`:
```rust
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

pub fn cache_path(repo_root: &Path) -> PathBuf {
    let canonical = std::fs::canonicalize(repo_root).unwrap_or_else(|_| repo_root.to_path_buf());
    let key = blake3::hash(canonical.to_string_lossy().as_bytes()).to_hex();
    let base = std::env::var_os("GATE_CACHE_DIR")
        .map(PathBuf::from)
        .or_else(|| dirs::cache_dir().map(|d| d.join("gate")))
        .unwrap_or_else(|| repo_root.join(".gate-cache"));
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
            let conn = Connection::open(&path)?;
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
            .query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name='meta'", [], |r| r.get::<_, i64>(0))
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

    pub fn upsert_file(&mut self, rel: &str, language: &str, content_hash: &str, parse_status: &str) -> anyhow::Result<()> {
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
```

Add `pub mod index;` to `crates/core/src/lib.rs`.

- [ ] **Step 4: Run tests to verify they pass**

```bash
cd <repo> && cargo test -p core -q 2>&1 | tail -3
```
Expected: `13 passed`.

- [ ] **Step 5: Commit**

```bash
cd <repo> && git add crates/core/src/index.rs crates/core/src/lib.rs && git commit -m "engine: sqlite index with file hashes and change detection"
```

---

### Task 5: Top-level symbol extraction

**Files:**
- Create: `crates/core/src/symbols.rs`
- Modify: `crates/core/src/lib.rs`

**Interfaces:**
- Consumes: `parse::ParsedFile`, `index::Index`.
- Produces: `symbols::Symbol { pub rel: String, pub kind: String, pub name: String, pub start_line: u32, pub start_col: u32, pub end_line: u32, pub end_col: u32, pub exported: bool }`, `symbols::extract(file: &ParsedFile) -> Vec<Symbol>` (top-level `function_declaration`, `class_declaration`, `abstract_class_declaration`, `interface_declaration`, `type_alias_declaration`, `enum_declaration`, and each `variable_declarator` of a top-level `lexical_declaration` or `variable_declaration`; `exported` when the parent is an `export_statement`), `symbols::store(index: &mut Index, file: &ParsedFile, syms: &[Symbol]) -> anyhow::Result<()>` (delete then insert for that `rel`), `symbols::enclosing_symbol(file: &ParsedFile, line: u32) -> Option<String>` (name of the top-level symbol whose span contains the 1-based line).

- [ ] **Step 1: Write the failing tests**

`crates/core/src/symbols.rs` (tests at the bottom):
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::Index;
    use crate::parse::parse_source;
    use std::path::Path;

    const SRC: &str = r#"import { x } from "./x";
export function main(): number {
  return 1;
}
function local(): void {}
export const answer = 42;
const hidden = 1, alsoHidden = 2;
export class Thing {}
export type Id = string;
interface Shape { w: number }
export enum Color { Red }
"#;

    fn parsed() -> crate::parse::ParsedFile {
        parse_source(Path::new("src/a.ts"), "src/a.ts", SRC.to_string()).unwrap()
    }

    #[test]
    fn extracts_top_level_symbols_with_export_flag() {
        let syms = extract(&parsed());
        let view: Vec<(String, String, bool)> = syms.iter().map(|s| (s.kind.clone(), s.name.clone(), s.exported)).collect();
        assert_eq!(
            view,
            vec![
                ("function".into(), "main".into(), true),
                ("function".into(), "local".into(), false),
                ("const".into(), "answer".into(), true),
                ("const".into(), "hidden".into(), false),
                ("const".into(), "alsoHidden".into(), false),
                ("class".into(), "Thing".into(), true),
                ("type".into(), "Id".into(), true),
                ("interface".into(), "Shape".into(), false),
                ("enum".into(), "Color".into(), true),
            ]
        );
        let main = &syms[0];
        assert_eq!((main.start_line, main.end_line), (2, 4));
    }

    #[test]
    fn enclosing_symbol_finds_the_function_around_a_line() {
        let p = parsed();
        assert_eq!(enclosing_symbol(&p, 3).as_deref(), Some("main"));
        assert_eq!(enclosing_symbol(&p, 1), None);
    }

    #[test]
    fn store_replaces_symbols_for_a_file() {
        let mut ix = Index::open_in_memory().unwrap();
        let p = parsed();
        let syms = extract(&p);
        store(&mut ix, &p, &syms).unwrap();
        store(&mut ix, &p, &syms).unwrap();
        let n: i64 = ix.conn().query_row("SELECT count(*) FROM symbols WHERE rel='src/a.ts'", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 9);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd <repo> && cargo test -p core -q 2>&1 | tail -5
```
Expected: compile error, `extract` not found.

- [ ] **Step 3: Implement symbols.rs**

Prepend to `crates/core/src/symbols.rs`:
```rust
use rusqlite::params;
use tree_sitter::Node;

use crate::index::Index;
use crate::parse::ParsedFile;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub rel: String,
    pub kind: String,
    pub name: String,
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
    pub exported: bool,
}

fn text<'a>(node: Node, src: &'a str) -> &'a str {
    node.utf8_text(src.as_bytes()).unwrap_or("")
}

fn span_of(node: Node) -> (u32, u32, u32, u32) {
    let s = node.start_position();
    let e = node.end_position();
    (s.row as u32 + 1, s.column as u32, e.row as u32 + 1, e.column as u32)
}

fn push(out: &mut Vec<Symbol>, rel: &str, kind: &str, name: &str, node: Node, exported: bool) {
    let (sl, sc, el, ec) = span_of(node);
    out.push(Symbol {
        rel: rel.to_string(),
        kind: kind.to_string(),
        name: name.to_string(),
        start_line: sl,
        start_col: sc,
        end_line: el,
        end_col: ec,
        exported,
    });
}

fn collect(node: Node, src: &str, rel: &str, exported: bool, out: &mut Vec<Symbol>) {
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
                            push(out, rel, "const", text(name, src), child, exported);
                        }
                    }
                }
            }
            return;
        }
        "export_statement" => {
            if let Some(decl) = node.child_by_field_name("declaration") {
                collect(decl, src, rel, true, out);
            }
            return;
        }
        _ => return,
    };
    if let Some(name) = node.child_by_field_name("name") {
        push(out, rel, kind, text(name, src), node, exported);
    }
}

pub fn extract(file: &ParsedFile) -> Vec<Symbol> {
    let mut out = Vec::new();
    let root = file.tree.root_node();
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        collect(child, &file.source, &file.rel, false, &mut out);
    }
    out
}

pub fn enclosing_symbol(file: &ParsedFile, line: u32) -> Option<String> {
    extract(file)
        .into_iter()
        .find(|s| s.start_line <= line && line <= s.end_line)
        .map(|s| s.name)
}

pub fn store(index: &mut Index, file: &ParsedFile, syms: &[Symbol]) -> anyhow::Result<()> {
    let conn = index.conn();
    conn.execute("DELETE FROM symbols WHERE rel = ?1", params![file.rel])?;
    let mut stmt = conn.prepare(
        "INSERT INTO symbols(rel, kind, name, start_line, start_col, end_line, end_col, exported)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )?;
    for s in syms {
        stmt.execute(params![s.rel, s.kind, s.name, s.start_line, s.start_col, s.end_line, s.end_col, s.exported as i64])?;
    }
    Ok(())
}
```

Add `pub mod symbols;` to `crates/core/src/lib.rs`.

- [ ] **Step 4: Run tests to verify they pass**

```bash
cd <repo> && cargo test -p core -q 2>&1 | tail -3
```
Expected: `16 passed`. If the `export_statement` node exposes its declaration as a plain child rather than the `declaration` field in this grammar version, iterate `node.children` and recurse into the first named child whose kind is one of the declaration kinds; record the change.

- [ ] **Step 5: Commit**

```bash
cd <repo> && git add crates/core/src/symbols.rs crates/core/src/lib.rs && git commit -m "engine: top-level symbol extraction into the index"
```

---

### Task 6: Finding and verdict contract

**Files:**
- Create: `crates/core/src/finding.rs`
- Modify: `crates/core/src/lib.rs`

**Interfaces:**
- Produces: the enums and structs listed under "Shared types" (all `Serialize`, `Deserialize`, `Clone`, `Debug`, `PartialEq`; enums serialise as lowercase strings), `finding::make_id(rule: &str, rel: &str, anchor: &str) -> String` (16 hex chars), `finding::Verdict::from_findings(findings: Vec<Finding>, duration_ms: u128) -> Verdict` (status Block if any finding has `confidence == High`, Advisory if any finding at all, else Pass; counts by severity; `blocking` = count of high-confidence findings; `truncated` 0), `Verdict::capped(self, n: usize) -> Verdict` (keeps the `n` highest by severity then rule then file then line, sets `truncated`), `Verdict::exit_code(&self) -> i32`.

- [ ] **Step 1: Write the failing tests**

`crates/core/src/finding.rs` (tests at the bottom):
```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn f(rule: &str, sev: Severity, conf: Confidence, line: u32) -> Finding {
        Finding {
            id: make_id(rule, "src/a.ts", "anchor"),
            rule: rule.into(),
            category: Category::Erosion,
            severity: sev,
            confidence: conf,
            file: "src/a.ts".into(),
            span: Span { start_line: line, start_col: 0, end_line: line, end_col: 5 },
            evidence: "e".into(),
            fix: "f".into(),
            related: vec![],
            owasp: None,
            cwe: None,
        }
    }

    #[test]
    fn id_is_stable_and_short() {
        let a = make_id("r", "src/a.ts", "main");
        assert_eq!(a.len(), 16);
        assert_eq!(a, make_id("r", "src/a.ts", "main"));
        assert_ne!(a, make_id("r", "src/a.ts", "other"));
        assert_ne!(a, make_id("r2", "src/a.ts", "main"));
    }

    #[test]
    fn verdict_status_follows_confidence() {
        let v = Verdict::from_findings(vec![], 1);
        assert_eq!(v.status, Status::Pass);
        assert_eq!(v.exit_code(), 0);
        let v = Verdict::from_findings(vec![f("r", Severity::Low, Confidence::Medium, 1)], 1);
        assert_eq!(v.status, Status::Advisory);
        assert_eq!(v.exit_code(), 0);
        let v = Verdict::from_findings(vec![f("r", Severity::High, Confidence::High, 1)], 1);
        assert_eq!(v.status, Status::Block);
        assert_eq!(v.exit_code(), 1);
        assert_eq!(v.blocking, 1);
    }

    #[test]
    fn capped_keeps_highest_severity_and_counts_truncated() {
        let mut fs = Vec::new();
        for i in 0..12 {
            fs.push(f("r", if i % 3 == 0 { Severity::High } else { Severity::Low }, Confidence::Medium, i));
        }
        let v = Verdict::from_findings(fs, 1).capped(10);
        assert_eq!(v.findings.len(), 10);
        assert_eq!(v.truncated, 2);
        assert_eq!(v.findings[0].severity, Severity::High);
        assert_eq!(v.high, 4); // counts reflect the full set, not the cap
    }

    #[test]
    fn serialises_enums_lowercase() {
        let v = Verdict::from_findings(vec![f("r", Severity::Medium, Confidence::High, 3)], 7);
        let s = serde_json::to_string(&v).unwrap();
        assert!(s.contains("\"status\":\"block\""));
        assert!(s.contains("\"severity\":\"medium\""));
        assert!(s.contains("\"confidence\":\"high\""));
        assert!(s.contains("\"category\":\"erosion\""));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd <repo> && cargo test -p core -q 2>&1 | tail -5
```
Expected: compile error, `Finding` not found.

- [ ] **Step 3: Implement finding.rs**

Prepend to `crates/core/src/finding.rs`:
```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    High,
    Medium,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Erosion,
    Security,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Pass,
    Advisory,
    Block,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub id: String,
    pub rule: String,
    pub category: Category,
    pub severity: Severity,
    pub confidence: Confidence,
    pub file: String,
    pub span: Span,
    pub evidence: String,
    pub fix: String,
    pub related: Vec<String>,
    pub owasp: Option<String>,
    pub cwe: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verdict {
    pub status: Status,
    pub high: usize,
    pub medium: usize,
    pub low: usize,
    pub blocking: usize,
    pub duration_ms: u128,
    pub findings: Vec<Finding>,
    pub truncated: usize,
}

pub fn make_id(rule: &str, rel: &str, anchor: &str) -> String {
    let mut h = blake3::Hasher::new();
    h.update(rule.as_bytes());
    h.update(b"\x1f");
    h.update(rel.as_bytes());
    h.update(b"\x1f");
    h.update(anchor.trim().as_bytes());
    h.finalize().to_hex()[..16].to_string()
}

impl Verdict {
    pub fn from_findings(mut findings: Vec<Finding>, duration_ms: u128) -> Verdict {
        findings.sort_by(|a, b| {
            a.severity
                .cmp(&b.severity)
                .then_with(|| a.rule.cmp(&b.rule))
                .then_with(|| a.file.cmp(&b.file))
                .then_with(|| a.span.start_line.cmp(&b.span.start_line))
        });
        let high = findings.iter().filter(|f| f.severity == Severity::High).count();
        let medium = findings.iter().filter(|f| f.severity == Severity::Medium).count();
        let low = findings.iter().filter(|f| f.severity == Severity::Low).count();
        let blocking = findings.iter().filter(|f| f.confidence == Confidence::High).count();
        let status = if blocking > 0 {
            Status::Block
        } else if !findings.is_empty() {
            Status::Advisory
        } else {
            Status::Pass
        };
        Verdict { status, high, medium, low, blocking, duration_ms, findings, truncated: 0 }
    }

    pub fn capped(mut self, n: usize) -> Verdict {
        if self.findings.len() > n {
            self.truncated = self.findings.len() - n;
            self.findings.truncate(n);
        }
        self
    }

    pub fn exit_code(&self) -> i32 {
        match self.status {
            Status::Block => 1,
            _ => 0,
        }
    }
}
```

Add `pub mod finding;` to `crates/core/src/lib.rs`.

- [ ] **Step 4: Run tests to verify they pass**

```bash
cd <repo> && cargo test -p core -q 2>&1 | tail -3
```
Expected: `20 passed`.

- [ ] **Step 5: Commit**

```bash
cd <repo> && git add crates/core/src/finding.rs crates/core/src/lib.rs && git commit -m "engine: finding and verdict contract"
```

---

### Task 7: Config file and baseline file

**Files:**
- Create: `crates/core/src/config.rs`
- Create: `crates/core/src/baseline.rs`
- Modify: `crates/core/src/lib.rs`

**Interfaces:**
- Produces: `config::CONFIG_FILE = "gate.toml"`, `config::Config { pub excludes: Vec<String>, pub debug_allowed: Vec<String>, pub rules: BTreeMap<String, RuleOverride> }` with `RuleOverride { pub enabled: Option<bool>, pub severity: Option<Severity> }`, `Config::default()` (empty excludes, `debug_allowed = ["**/scripts/**", "**/*.config.*", "**/bin/**"]`, no overrides), `Config::load(repo_root: &Path) -> anyhow::Result<Config>` (defaults when the file is absent; error with path when present but invalid), `Config::rule_enabled(&self, id: &str) -> bool`, `Config::severity_for(&self, id: &str, default: Severity) -> Severity`.
- Produces: `baseline::BASELINE_FILE = "gate-baseline.json"`, `baseline::Entry { pub id: String, pub rule: String, pub file: String, pub reason: String, pub author: String, pub date: String }`, `baseline::Baseline { pub entries: Vec<Entry> }`, `Baseline::load(repo_root) -> anyhow::Result<Baseline>` (empty when absent), `Baseline::save(&self, repo_root) -> anyhow::Result<()>` (pretty JSON, entries sorted by id), `Baseline::contains(&self, id: &str) -> bool`, `Baseline::accept(&mut self, f: &Finding, reason: &str, author: &str)` (date is today as `YYYY-MM-DD` from the system clock), `Baseline::filter(&self, findings: Vec<Finding>) -> Vec<Finding>` (drops baselined ids).

Config file format:
```toml
excludes = ["src/generated/**"]
debug_allowed = ["**/scripts/**"]

[rules.leftover-debug]
enabled = true
severity = "high"

[rules.leftover-agent-marker]
enabled = false
```

- [ ] **Step 1: Write the failing tests**

`crates/core/src/config.rs` (tests at the bottom):
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::finding::Severity;

    #[test]
    fn defaults_when_absent() {
        let dir = std::env::temp_dir().join(format!("gate-config-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let c = Config::load(&dir).unwrap();
        assert!(c.excludes.is_empty());
        assert!(c.debug_allowed.iter().any(|g| g.contains("scripts")));
        assert!(c.rule_enabled("leftover-debug"));
        assert_eq!(c.severity_for("leftover-debug", Severity::High), Severity::High);
    }

    #[test]
    fn parses_overrides() {
        let dir = std::env::temp_dir().join(format!("gate-config2-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(CONFIG_FILE),
            "excludes = [\"src/gen/**\"]\n[rules.leftover-debug]\nseverity = \"low\"\n[rules.leftover-agent-marker]\nenabled = false\n",
        )
        .unwrap();
        let c = Config::load(&dir).unwrap();
        assert_eq!(c.excludes, vec!["src/gen/**"]);
        assert_eq!(c.severity_for("leftover-debug", Severity::High), Severity::Low);
        assert!(!c.rule_enabled("leftover-agent-marker"));
        assert!(c.rule_enabled("leftover-commented-code"));
    }

    #[test]
    fn invalid_file_is_an_error_naming_the_path() {
        let dir = std::env::temp_dir().join(format!("gate-config3-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(CONFIG_FILE), "excludes = [").unwrap();
        let err = Config::load(&dir).unwrap_err().to_string();
        assert!(err.contains(CONFIG_FILE));
    }
}
```

`crates/core/src/baseline.rs` (tests at the bottom):
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::finding::{Category, Confidence, Finding, Severity, Span};

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
        let dir = std::env::temp_dir().join(format!("gate-baseline-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut b = Baseline::load(&dir).unwrap();
        assert!(b.entries.is_empty());
        b.accept(&f("b"), "legacy script", "tester");
        b.accept(&f("a"), "generated", "tester");
        b.save(&dir).unwrap();
        let b2 = Baseline::load(&dir).unwrap();
        assert_eq!(b2.entries.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), vec!["a", "b"]);
        assert!(b2.contains("a"));
        assert_eq!(b2.entries[0].date.len(), 10);
        let kept = b2.filter(vec![f("a"), f("c")]);
        assert_eq!(kept.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), vec!["c"]);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd <repo> && cargo test -p core -q 2>&1 | tail -5
```
Expected: compile error, `Config` and `Baseline` not found.

- [ ] **Step 3: Implement config.rs**

Prepend to `crates/core/src/config.rs`:
```rust
use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::finding::Severity;

pub const CONFIG_FILE: &str = "gate.toml";

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct RuleOverride {
    pub enabled: Option<bool>,
    pub severity: Option<Severity>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
    pub excludes: Vec<String>,
    pub debug_allowed: Vec<String>,
    pub rules: BTreeMap<String, RuleOverride>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            excludes: vec![],
            debug_allowed: vec!["**/scripts/**".into(), "**/*.config.*".into(), "**/bin/**".into()],
            rules: BTreeMap::new(),
        }
    }
}

impl Config {
    pub fn load(repo_root: &Path) -> anyhow::Result<Config> {
        let path = repo_root.join(CONFIG_FILE);
        if !path.exists() {
            return Ok(Config::default());
        }
        let text = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("invalid {}", path.display()))
    }

    pub fn rule_enabled(&self, id: &str) -> bool {
        self.rules.get(id).and_then(|r| r.enabled).unwrap_or(true)
    }

    pub fn severity_for(&self, id: &str, default: Severity) -> Severity {
        self.rules.get(id).and_then(|r| r.severity).unwrap_or(default)
    }
}
```

- [ ] **Step 4: Implement baseline.rs**

Prepend to `crates/core/src/baseline.rs`:
```rust
use std::collections::HashSet;
use std::path::Path;

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::finding::Finding;

pub const BASELINE_FILE: &str = "gate-baseline.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    pub rule: String,
    pub file: String,
    pub reason: String,
    pub author: String,
    pub date: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Baseline {
    pub entries: Vec<Entry>,
}

fn today() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // civil-from-days, Howard Hinnant's algorithm, no chrono dependency needed
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
    pub fn load(repo_root: &Path) -> anyhow::Result<Baseline> {
        let path = repo_root.join(BASELINE_FILE);
        if !path.exists() {
            return Ok(Baseline::default());
        }
        let text = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        serde_json::from_str(&text).with_context(|| format!("invalid {}", path.display()))
    }

    pub fn save(&self, repo_root: &Path) -> anyhow::Result<()> {
        let mut copy = self.clone();
        copy.entries.sort_by(|a, b| a.id.cmp(&b.id));
        let path = repo_root.join(BASELINE_FILE);
        std::fs::write(&path, serde_json::to_string_pretty(&copy)?).with_context(|| format!("writing {}", path.display()))
    }

    pub fn contains(&self, id: &str) -> bool {
        self.entries.iter().any(|e| e.id == id)
    }

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
        });
    }

    pub fn filter(&self, findings: Vec<Finding>) -> Vec<Finding> {
        let ids: HashSet<&str> = self.entries.iter().map(|e| e.id.as_str()).collect();
        findings.into_iter().filter(|f| !ids.contains(f.id.as_str())).collect()
    }
}
```

Add `pub mod config;` and `pub mod baseline;` to `crates/core/src/lib.rs`.

- [ ] **Step 5: Run tests to verify they pass**

```bash
cd <repo> && cargo test -p core -q 2>&1 | tail -3
```
Expected: `24 passed`.

- [ ] **Step 6: Commit**

```bash
cd <repo> && git add crates/core/src/config.rs crates/core/src/baseline.rs crates/core/src/lib.rs && git commit -m "engine: config file and baseline file"
```

---

### Task 8: Rule trait, context, and registry

**Files:**
- Modify: `crates/rules/src/lib.rs`

**Interfaces:**
- Consumes: `gate_core::parse::ParsedFile`, `gate_core::config::Config`, `gate_core::finding::*`.
- Produces: `RuleContext<'a> { pub files: &'a [ParsedFile], pub config: &'a Config }`, `trait Rule { fn id(&self) -> &'static str; fn category(&self) -> Category; fn default_severity(&self) -> Severity; fn confidence(&self) -> Confidence; fn run(&self, ctx: &RuleContext) -> Vec<Finding>; }`, `all_rules() -> Vec<Box<dyn Rule>>` (empty until Task 9 registers the first rule), `run_all(ctx: &RuleContext) -> Vec<Finding>` (skips rules disabled in config, applies severity overrides), and helper `line_text(file: &ParsedFile, line: u32) -> &str` and `anchor_for(file: &ParsedFile, line: u32) -> String` (enclosing symbol name or trimmed line text).

- [ ] **Step 1: Write the failing test**

Replace `crates/rules/src/lib.rs` with the tests first (implementation added in Step 3 above them):
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use gate_core::config::Config;
    use gate_core::finding::{Category, Confidence, Severity};
    use gate_core::parse::parse_source;
    use std::path::Path;

    struct Always;
    impl Rule for Always {
        fn id(&self) -> &'static str { "always" }
        fn category(&self) -> Category { Category::Erosion }
        fn default_severity(&self) -> Severity { Severity::Medium }
        fn confidence(&self) -> Confidence { Confidence::Medium }
        fn run(&self, ctx: &RuleContext) -> Vec<Finding> {
            ctx.files.iter().map(|f| finding(self, f, 1, "hit", "remove it")).collect()
        }
    }

    #[test]
    fn run_rules_applies_config_overrides() {
        let file = parse_source(Path::new("src/a.ts"), "src/a.ts", "export const a = 1;\n".into()).unwrap();
        let files = vec![file];
        let mut config = Config::default();
        let ctx = RuleContext { files: &files, config: &config };
        let out = run_rules(&[Box::new(Always)], &ctx);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].severity, Severity::Medium);
        assert_eq!(out[0].id.len(), 16);

        config.rules.insert("always".into(), gate_core::config::RuleOverride { enabled: None, severity: Some(Severity::Low) });
        let ctx = RuleContext { files: &files, config: &config };
        assert_eq!(run_rules(&[Box::new(Always)], &ctx)[0].severity, Severity::Low);

        config.rules.insert("always".into(), gate_core::config::RuleOverride { enabled: Some(false), severity: None });
        let ctx = RuleContext { files: &files, config: &config };
        assert!(run_rules(&[Box::new(Always)], &ctx).is_empty());
    }

    #[test]
    fn anchor_prefers_enclosing_symbol() {
        let file = parse_source(Path::new("src/a.ts"), "src/a.ts", "function f() {\n  console.log(1);\n}\nconsole.log(2);\n".into()).unwrap();
        assert_eq!(anchor_for(&file, 2), "f");
        assert_eq!(anchor_for(&file, 4), "console.log(2);");
        assert_eq!(line_text(&file, 4), "console.log(2);");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cd <repo> && cargo test -p rules -q 2>&1 | tail -5
```
Expected: compile error, `Rule` not found.

- [ ] **Step 3: Implement the trait, context, helpers, and registry**

Prepend to `crates/rules/src/lib.rs`:
```rust
use gate_core::config::Config;
use gate_core::finding::{make_id, Category, Confidence, Finding, Severity, Span};
use gate_core::parse::ParsedFile;
use gate_core::symbols::enclosing_symbol;

pub struct RuleContext<'a> {
    pub files: &'a [ParsedFile],
    pub config: &'a Config,
}

pub trait Rule {
    fn id(&self) -> &'static str;
    fn category(&self) -> Category;
    fn default_severity(&self) -> Severity;
    fn confidence(&self) -> Confidence;
    fn run(&self, ctx: &RuleContext) -> Vec<Finding>;
}

pub fn line_text(file: &ParsedFile, line: u32) -> &str {
    file.source.lines().nth(line.saturating_sub(1) as usize).unwrap_or("").trim()
}

pub fn anchor_for(file: &ParsedFile, line: u32) -> String {
    enclosing_symbol(file, line).unwrap_or_else(|| line_text(file, line).to_string())
}

pub fn finding(rule: &dyn Rule, file: &ParsedFile, line: u32, evidence: &str, fix: &str) -> Finding {
    let text = line_text(file, line);
    Finding {
        id: make_id(rule.id(), &file.rel, &anchor_for(file, line)),
        rule: rule.id().to_string(),
        category: rule.category(),
        severity: rule.default_severity(),
        confidence: rule.confidence(),
        file: file.rel.clone(),
        span: Span { start_line: line, start_col: 0, end_line: line, end_col: text.len() as u32 },
        evidence: evidence.to_string(),
        fix: fix.to_string(),
        related: vec![],
        owasp: None,
        cwe: None,
    }
}

pub fn run_rules(rules: &[Box<dyn Rule>], ctx: &RuleContext) -> Vec<Finding> {
    let mut out = Vec::new();
    for rule in rules {
        if !ctx.config.rule_enabled(rule.id()) {
            continue;
        }
        let severity = ctx.config.severity_for(rule.id(), rule.default_severity());
        for mut f in rule.run(ctx) {
            f.severity = severity;
            out.push(f);
        }
    }
    out
}

pub fn all_rules() -> Vec<Box<dyn Rule>> {
    vec![]
}

pub fn run_all(ctx: &RuleContext) -> Vec<Finding> {
    run_rules(&all_rules(), ctx)
}
```

Because `finding()` is used by the test's `Always` rule before any real rule exists, keep it public.

- [ ] **Step 4: Run tests to verify they pass**

```bash
cd <repo> && cargo test -p rules -q 2>&1 | tail -3
```
Expected: `2 passed`.

- [ ] **Step 5: Commit**

```bash
cd <repo> && git add crates/rules/src/lib.rs && git commit -m "engine: rule trait, context, and registry"
```

---

### Task 9: Rule leftover-debug

**Files:**
- Create: `crates/rules/src/leftover_debug.rs`
- Create: `crates/rules/tests/fixtures/leftover_debug/flag/a.ts`, `.../clean/b.ts`, `.../edge/c.ts`, `.../clean/scripts/build.ts`
- Create: `crates/rules/tests/common/mod.rs`
- Create: `crates/rules/tests/leftover_debug.rs`
- Modify: `crates/rules/src/lib.rs` (register)

**Rule definition:** flag a `call_expression` whose function is a `member_expression` with object `console` and property in `log`, `debug`, `trace`, `dir`, `table`, and any `debugger_statement`. Not flagged: `console.error`, `console.warn`, `console.info`; any file whose `rel` matches a `config.debug_allowed` glob; a `console.log` inside a line that carries the marker comment `// gate:allow`. Severity High, confidence High (blocks). Evidence: the trimmed line. Fix: "Remove the debug statement or route it through the project logger".

- [ ] **Step 1: Write fixtures and the shared test helper**

`crates/rules/tests/fixtures/leftover_debug/flag/a.ts`:
```ts
export function load(id: string) {
  console.log("loading", id);
  console.debug(id);
  debugger;
  return id;
}
```

`crates/rules/tests/fixtures/leftover_debug/clean/b.ts`:
```ts
export function load(id: string) {
  console.error("failed", id);
  console.warn("slow", id);
  logger.info("ok");
  return id;
}
```

`crates/rules/tests/fixtures/leftover_debug/clean/scripts/build.ts`:
```ts
console.log("building");
```

`crates/rules/tests/fixtures/leftover_debug/edge/c.ts`:
```ts
export function load(id: string) {
  console.log("kept on purpose"); // gate:allow
  const console = { log: (x: string) => x };
  console.log("shadowed local console");
  return id;
}
```

`crates/rules/tests/common/mod.rs`:
```rust
use std::path::{Path, PathBuf};

use gate_core::config::Config;
use gate_core::finding::Finding;
use gate_core::parse::{parse_file, ParsedFile};
use gate_core::walk::{source_files, WalkOptions};
use gate_rules::{run_rules, Rule, RuleContext};

pub fn fixture(rule: &str, bucket: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(rule).join(bucket)
}

pub fn parse_dir(root: &Path) -> Vec<ParsedFile> {
    source_files(root, &WalkOptions::default())
        .unwrap()
        .into_iter()
        .filter_map(|p| parse_file(root, &p).unwrap())
        .collect()
}

pub fn run_on(rule: Box<dyn Rule>, root: &Path, config: &Config) -> Vec<Finding> {
    let files = parse_dir(root);
    let ctx = RuleContext { files: &files, config };
    run_rules(&[rule], &ctx)
}
```

- [ ] **Step 2: Write the failing tests**

`crates/rules/tests/leftover_debug.rs`:
```rust
mod common;

use common::{fixture, run_on};
use gate_core::config::Config;
use gate_core::finding::{Confidence, Severity};
use gate_rules::leftover_debug::LeftoverDebug;

#[test]
fn flags_console_log_debug_and_debugger() {
    let out = run_on(Box::new(LeftoverDebug), &fixture("leftover_debug", "flag"), &Config::default());
    let lines: Vec<u32> = out.iter().map(|f| f.span.start_line).collect();
    assert_eq!(lines, vec![2, 3, 4]);
    assert!(out.iter().all(|f| f.severity == Severity::High && f.confidence == Confidence::High));
    assert_eq!(out[0].evidence, "console.log(\"loading\", id);");
    assert_eq!(out[0].rule, "leftover-debug");
}

#[test]
fn ignores_error_warn_and_allowed_paths() {
    let out = run_on(Box::new(LeftoverDebug), &fixture("leftover_debug", "clean"), &Config::default());
    assert!(out.is_empty(), "got {:?}", out);
}

#[test]
fn allow_comment_suppresses_and_shadowed_console_is_still_flagged() {
    let out = run_on(Box::new(LeftoverDebug), &fixture("leftover_debug", "edge"), &Config::default());
    let lines: Vec<u32> = out.iter().map(|f| f.span.start_line).collect();
    assert_eq!(lines, vec![4]);
}
```

The shadowed-console case is flagged on purpose: the rule is syntactic and a local variable named `console` is itself a leftover smell. Document this in the rule's doc comment.

- [ ] **Step 3: Run tests to verify they fail**

```bash
cd <repo> && cargo test -p rules --test leftover_debug -q 2>&1 | tail -5
```
Expected: compile error, module `leftover_debug` not found.

- [ ] **Step 4: Implement the rule**

`crates/rules/src/leftover_debug.rs`:
```rust
//! Flags console.log / console.debug / console.trace / console.dir / console.table
//! calls and `debugger` statements. Purely syntactic: a locally shadowed `console`
//! is still flagged, because a local variable called `console` is itself a leftover.

use gate_core::finding::{Category, Confidence, Finding, Severity};
use globset::{Glob, GlobSetBuilder};
use tree_sitter::Node;

use crate::{finding, line_text, Rule, RuleContext};

pub struct LeftoverDebug;

const FLAGGED: &[&str] = &["log", "debug", "trace", "dir", "table"];
const ALLOW_MARK: &str = "gate:allow";

fn is_debug_call(node: Node, src: &str) -> bool {
    if node.kind() != "call_expression" {
        return false;
    }
    let Some(func) = node.child_by_field_name("function") else { return false };
    if func.kind() != "member_expression" {
        return false;
    }
    let obj = func.child_by_field_name("object").map(|n| n.utf8_text(src.as_bytes()).unwrap_or(""));
    let prop = func.child_by_field_name("property").map(|n| n.utf8_text(src.as_bytes()).unwrap_or(""));
    obj == Some("console") && prop.map(|p| FLAGGED.contains(&p)).unwrap_or(false)
}

fn walk(node: Node, src: &str, hits: &mut Vec<u32>) {
    if node.kind() == "debugger_statement" || is_debug_call(node, src) {
        hits.push(node.start_position().row as u32 + 1);
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, src, hits);
    }
}

impl Rule for LeftoverDebug {
    fn id(&self) -> &'static str { "leftover-debug" }
    fn category(&self) -> Category { Category::Erosion }
    fn default_severity(&self) -> Severity { Severity::High }
    fn confidence(&self) -> Confidence { Confidence::High }

    fn run(&self, ctx: &RuleContext) -> Vec<Finding> {
        let mut b = GlobSetBuilder::new();
        for g in &ctx.config.debug_allowed {
            if let Ok(glob) = Glob::new(g) {
                b.add(glob);
            }
        }
        let allowed = b.build().unwrap_or_else(|_| GlobSetBuilder::new().build().unwrap());
        let mut out = Vec::new();
        for file in ctx.files {
            if file.has_error || allowed.is_match(&file.rel) {
                continue;
            }
            let mut hits = Vec::new();
            walk(file.tree.root_node(), &file.source, &mut hits);
            hits.sort_unstable();
            hits.dedup();
            for line in hits {
                let text = line_text(file, line);
                if text.contains(ALLOW_MARK) {
                    continue;
                }
                out.push(finding(self, file, line, text, "Remove the debug statement or route it through the project logger"));
            }
        }
        out
    }
}
```

In `crates/rules/src/lib.rs` add `pub mod leftover_debug;` at the top and change `all_rules` to:
```rust
pub fn all_rules() -> Vec<Box<dyn Rule>> {
    vec![Box::new(leftover_debug::LeftoverDebug)]
}
```

- [ ] **Step 5: Run tests to verify they pass**

```bash
cd <repo> && cargo test -p rules -q 2>&1 | tail -3
```
Expected: `5 passed` across the unit and integration tests. If the `scripts/build.ts` fixture is flagged, the default `debug_allowed` glob `**/scripts/**` did not match a `rel` like `scripts/build.ts` because `**/` requires a leading segment in globset; add `scripts/**` alongside in `Config::default()` and in the test expectations, and record it.

- [ ] **Step 6: Commit**

```bash
cd <repo> && git add crates/rules/src/leftover_debug.rs crates/rules/src/lib.rs crates/rules/tests/common/mod.rs crates/rules/tests/leftover_debug.rs crates/rules/tests/fixtures/leftover_debug && git commit -m "engine: leftover-debug rule"
```

---

### Task 10: Rules leftover-commented-code and leftover-agent-marker

**Files:**
- Create: `crates/rules/src/leftover_commented.rs`
- Create: `crates/rules/src/leftover_marker.rs`
- Create: `crates/rules/tests/fixtures/leftover_commented/{flag,clean,edge}/*.ts`
- Create: `crates/rules/tests/fixtures/leftover_marker/{flag,clean}/*.ts`
- Create: `crates/rules/tests/leftover_commented.rs`, `crates/rules/tests/leftover_marker.rs`
- Modify: `crates/rules/src/lib.rs` (register both)

**Rule definitions:**
- `leftover-commented-code`: a run of three or more consecutive `//` comment lines (or one block comment spanning three or more lines) where at least two lines look like code: the trimmed comment body ends with `;`, `{`, `}`, `)`, or `,`, or starts with `const `, `let `, `var `, `return `, `if (`, `for (`, `import `, `export `. Reported once per run at its first line. Severity Medium, confidence Medium (advisory). Fix: "Delete the commented-out block; version control keeps the history". Not flagged: license headers (runs where every line starts with `*` or contains `Copyright` or `SPDX`), JSDoc blocks (`/**`), and runs shorter than three lines.
- `leftover-agent-marker`: a comment containing `TODO`, `FIXME`, `HACK`, or `XXX` (word boundary, case-sensitive) that does not also contain an issue reference matching `#\d+`, `[A-Z]{2,}-\d+`, or a URL (`http`). Severity Low, confidence Medium (advisory). Fix: "Link the marker to an issue or resolve it now". Reported per comment line.

- [ ] **Step 1: Write fixtures**

`crates/rules/tests/fixtures/leftover_commented/flag/a.ts`:
```ts
export function keep(): number {
  // const old = compute();
  // if (old > 1) {
  //   return old;
  // }
  return 1;
}
/*
const legacy = 1;
const more = 2;
export default legacy;
*/
```

`crates/rules/tests/fixtures/leftover_commented/clean/b.ts`:
```ts
/**
 * Loads the thing.
 * Returns a number.
 * Never throws.
 */
export function load(): number {
  // This is prose explaining the next line.
  // It continues here.
  // And it ends here.
  return 1;
}
// Copyright 2026 Example Ltd
// SPDX-License-Identifier: MIT
// All rights reserved;
```

`crates/rules/tests/fixtures/leftover_commented/edge/c.ts`:
```ts
export function two(): number {
  // const a = 1;
  // const b = 2;
  return 3;
}
```

`crates/rules/tests/fixtures/leftover_marker/flag/a.ts`:
```ts
// TODO handle the empty case
export function f(): number {
  return 1; // FIXME wrong for negatives
}
/* HACK until the API settles */
```

`crates/rules/tests/fixtures/leftover_marker/clean/b.ts`:
```ts
// TODO(#123) handle the empty case
export function f(): number {
  return 1; // FIXME FL-42 wrong for negatives
}
// see https://example.com/todo for context, TODO tracked there
// todo in lowercase is prose, not a marker
```

- [ ] **Step 2: Write the failing tests**

`crates/rules/tests/leftover_commented.rs`:
```rust
mod common;

use common::{fixture, run_on};
use gate_core::config::Config;
use gate_core::finding::{Confidence, Severity};
use gate_rules::leftover_commented::LeftoverCommented;

#[test]
fn flags_line_runs_and_block_comments_that_look_like_code() {
    let out = run_on(Box::new(LeftoverCommented), &fixture("leftover_commented", "flag"), &Config::default());
    let lines: Vec<u32> = out.iter().map(|f| f.span.start_line).collect();
    assert_eq!(lines, vec![2, 8]);
    assert!(out.iter().all(|f| f.severity == Severity::Medium && f.confidence == Confidence::Medium));
}

#[test]
fn ignores_prose_jsdoc_and_license_headers() {
    let out = run_on(Box::new(LeftoverCommented), &fixture("leftover_commented", "clean"), &Config::default());
    assert!(out.is_empty(), "got {:?}", out);
}

#[test]
fn two_lines_is_not_a_run() {
    let out = run_on(Box::new(LeftoverCommented), &fixture("leftover_commented", "edge"), &Config::default());
    assert!(out.is_empty());
}
```

`crates/rules/tests/leftover_marker.rs`:
```rust
mod common;

use common::{fixture, run_on};
use gate_core::config::Config;
use gate_core::finding::{Confidence, Severity};
use gate_rules::leftover_marker::LeftoverMarker;

#[test]
fn flags_markers_without_issue_references() {
    let out = run_on(Box::new(LeftoverMarker), &fixture("leftover_marker", "flag"), &Config::default());
    let lines: Vec<u32> = out.iter().map(|f| f.span.start_line).collect();
    assert_eq!(lines, vec![1, 3, 5]);
    assert!(out.iter().all(|f| f.severity == Severity::Low && f.confidence == Confidence::Medium));
}

#[test]
fn ignores_markers_with_references_urls_and_lowercase_prose() {
    let out = run_on(Box::new(LeftoverMarker), &fixture("leftover_marker", "clean"), &Config::default());
    assert!(out.is_empty(), "got {:?}", out);
}
```

- [ ] **Step 3: Run tests to verify they fail**

```bash
cd <repo> && cargo test -p rules -q 2>&1 | tail -5
```
Expected: compile errors, modules not found.

- [ ] **Step 4: Implement both rules**

`crates/rules/src/leftover_commented.rs`:
```rust
//! Flags runs of commented-out code. Heuristic and advisory by design.

use gate_core::finding::{Category, Confidence, Finding, Severity};
use gate_core::parse::ParsedFile;
use tree_sitter::Node;

use crate::{finding, Rule, RuleContext};

pub struct LeftoverCommented;

const CODE_ENDINGS: &[char] = &[';', '{', '}', ')', ','];
const CODE_STARTS: &[&str] = &["const ", "let ", "var ", "return ", "if (", "for (", "import ", "export "];

fn looks_like_code(line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() {
        return false;
    }
    t.ends_with(CODE_ENDINGS) || CODE_STARTS.iter().any(|s| t.starts_with(s))
}

fn is_license_or_doc(lines: &[&str]) -> bool {
    lines.iter().any(|l| l.contains("Copyright") || l.contains("SPDX")) || lines.iter().all(|l| l.trim().starts_with('*'))
}

fn strip_line_comment(text: &str) -> &str {
    text.trim().trim_start_matches("//").trim()
}

fn comments(node: Node, out: &mut Vec<Node>) {
    if node.kind() == "comment" {
        out.push(node);
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        comments(child, out);
    }
}

fn runs(file: &ParsedFile) -> Vec<u32> {
    let src = file.source.as_bytes();
    let mut nodes = Vec::new();
    comments(file.tree.root_node(), &mut nodes);
    nodes.sort_by_key(|n| n.start_byte());

    let mut hits = Vec::new();
    let mut run: Vec<(u32, String)> = Vec::new();
    let flush = |run: &mut Vec<(u32, String)>, hits: &mut Vec<u32>| {
        if run.len() >= 3 {
            let bodies: Vec<&str> = run.iter().map(|(_, s)| s.as_str()).collect();
            let code_lines = bodies.iter().filter(|l| looks_like_code(l)).count();
            if code_lines >= 2 && !is_license_or_doc(&bodies) {
                hits.push(run[0].0);
            }
        }
        run.clear();
    };

    let mut last_line: Option<u32> = None;
    for n in nodes {
        let text = n.utf8_text(src).unwrap_or("");
        let line = n.start_position().row as u32 + 1;
        if text.starts_with("//") {
            if last_line.map(|l| l + 1 != line).unwrap_or(false) {
                flush(&mut run, &mut hits);
            }
            run.push((line, strip_line_comment(text).to_string()));
            last_line = Some(line);
        } else {
            flush(&mut run, &mut hits);
            last_line = None;
            if text.starts_with("/**") {
                continue;
            }
            let inner: Vec<&str> = text
                .trim_start_matches("/*")
                .trim_end_matches("*/")
                .lines()
                .map(|l| l.trim().trim_start_matches('*').trim())
                .filter(|l| !l.is_empty())
                .collect();
            if inner.len() >= 3 && inner.iter().filter(|l| looks_like_code(l)).count() >= 2 && !is_license_or_doc(&inner) {
                hits.push(line);
            }
        }
    }
    flush(&mut run, &mut hits);
    hits
}

impl Rule for LeftoverCommented {
    fn id(&self) -> &'static str { "leftover-commented-code" }
    fn category(&self) -> Category { Category::Erosion }
    fn default_severity(&self) -> Severity { Severity::Medium }
    fn confidence(&self) -> Confidence { Confidence::Medium }

    fn run(&self, ctx: &RuleContext) -> Vec<Finding> {
        let mut out = Vec::new();
        for file in ctx.files {
            for line in runs(file) {
                out.push(finding(self, file, line, "commented-out code block", "Delete the commented-out block; version control keeps the history"));
            }
        }
        out
    }
}
```

`crates/rules/src/leftover_marker.rs`:
```rust
//! Flags TODO / FIXME / HACK / XXX comments that carry no issue reference.

use gate_core::finding::{Category, Confidence, Finding, Severity};
use tree_sitter::Node;

use crate::{finding, Rule, RuleContext};

pub struct LeftoverMarker;

const MARKERS: &[&str] = &["TODO", "FIXME", "HACK", "XXX"];

fn has_marker(text: &str) -> bool {
    MARKERS.iter().any(|m| {
        text.match_indices(m).any(|(i, _)| {
            let before = text[..i].chars().last().map(|c| !c.is_alphanumeric()).unwrap_or(true);
            let after = text[i + m.len()..].chars().next().map(|c| !c.is_alphanumeric()).unwrap_or(true);
            before && after
        })
    })
}

fn has_reference(text: &str) -> bool {
    if text.contains("http") {
        return true;
    }
    let bytes = text.as_bytes();
    // #123
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'#' && bytes.get(i + 1).map(|c| c.is_ascii_digit()).unwrap_or(false) {
            return true;
        }
    }
    // ABC-123
    let mut i = 0;
    while i < bytes.len() {
        let start = i;
        while i < bytes.len() && bytes[i].is_ascii_uppercase() {
            i += 1;
        }
        if i - start >= 2 && i < bytes.len() && bytes[i] == b'-' && bytes.get(i + 1).map(|c| c.is_ascii_digit()).unwrap_or(false) {
            return true;
        }
        i += 1;
    }
    false
}

fn comment_lines(node: Node, src: &str, out: &mut Vec<(u32, String)>) {
    if node.kind() == "comment" {
        let text = node.utf8_text(src.as_bytes()).unwrap_or("");
        let base = node.start_position().row as u32 + 1;
        for (i, l) in text.lines().enumerate() {
            out.push((base + i as u32, l.to_string()));
        }
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        comment_lines(child, src, out);
    }
}

impl Rule for LeftoverMarker {
    fn id(&self) -> &'static str { "leftover-agent-marker" }
    fn category(&self) -> Category { Category::Erosion }
    fn default_severity(&self) -> Severity { Severity::Low }
    fn confidence(&self) -> Confidence { Confidence::Medium }

    fn run(&self, ctx: &RuleContext) -> Vec<Finding> {
        let mut out = Vec::new();
        for file in ctx.files {
            let mut lines = Vec::new();
            comment_lines(file.tree.root_node(), &file.source, &mut lines);
            for (line, text) in lines {
                if has_marker(&text) && !has_reference(&text) {
                    out.push(finding(self, file, line, text.trim(), "Link the marker to an issue or resolve it now"));
                }
            }
        }
        out
    }
}
```

Register both in `crates/rules/src/lib.rs`: add `pub mod leftover_commented; pub mod leftover_marker;` and
```rust
pub fn all_rules() -> Vec<Box<dyn Rule>> {
    vec![
        Box::new(leftover_debug::LeftoverDebug),
        Box::new(leftover_commented::LeftoverCommented),
        Box::new(leftover_marker::LeftoverMarker),
    ]
}
```

- [ ] **Step 5: Run tests to verify they pass**

```bash
cd <repo> && cargo test -p rules -q 2>&1 | tail -3
```
Expected: `10 passed` across all rules tests. The block-comment case in `flag/a.ts` should report line 8 (where `/*` starts); if the grammar reports the comment node one line earlier or later, adjust the fixture expectation only after printing the node's start row, and record it.

- [ ] **Step 6: Commit**

```bash
cd <repo> && git add crates/rules/src/leftover_commented.rs crates/rules/src/leftover_marker.rs crates/rules/src/lib.rs crates/rules/tests/leftover_commented.rs crates/rules/tests/leftover_marker.rs crates/rules/tests/fixtures/leftover_commented crates/rules/tests/fixtures/leftover_marker && git commit -m "engine: leftover-commented-code and leftover-agent-marker rules"
```

---

### Task 11: Terminal and agent JSON reporters

**Files:**
- Create: `crates/reporters/src/terminal.rs`
- Create: `crates/reporters/src/agent.rs`
- Modify: `crates/reporters/src/lib.rs`

**Interfaces:**
- Consumes: `gate_core::finding::{Verdict, Finding, Status}`.
- Produces: `terminal::render(v: &Verdict) -> String` (grouped by file, one line per finding: `  L{line}  {rule}  {severity}/{confidence}  {evidence}` then `        fix: {fix}`; header line `{STATUS}  {n} finding(s): {high} high, {medium} medium, {low} low  ({duration_ms} ms)`; footer `... and {truncated} more` when truncated; the word `PASS`, `ADVISORY`, or `BLOCK` in upper case), `agent::render(v: &Verdict) -> String` (compact JSON of `Verdict::capped(10)` with the full counts, no file contents).

- [ ] **Step 1: Write the failing tests**

`crates/reporters/src/terminal.rs` (tests at the bottom):
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use gate_core::finding::*;

    fn f(file: &str, line: u32) -> Finding {
        Finding {
            id: "0123456789abcdef".into(),
            rule: "leftover-debug".into(),
            category: Category::Erosion,
            severity: Severity::High,
            confidence: Confidence::High,
            file: file.into(),
            span: Span { start_line: line, start_col: 0, end_line: line, end_col: 10 },
            evidence: "console.log(1)".into(),
            fix: "Remove it".into(),
            related: vec![],
            owasp: None,
            cwe: None,
        }
    }

    #[test]
    fn renders_header_groups_and_fix() {
        let v = Verdict::from_findings(vec![f("src/b.ts", 4), f("src/a.ts", 9)], 12);
        let s = render(&v);
        assert!(s.starts_with("BLOCK  2 finding(s): 2 high, 0 medium, 0 low  (12 ms)"));
        let a = s.find("src/a.ts").unwrap();
        let b = s.find("src/b.ts").unwrap();
        assert!(a < b);
        assert!(s.contains("  L9  leftover-debug  high/high  console.log(1)"));
        assert!(s.contains("        fix: Remove it"));
    }

    #[test]
    fn renders_pass_and_truncation() {
        assert!(render(&Verdict::from_findings(vec![], 3)).starts_with("PASS  0 finding(s)"));
        let many: Vec<Finding> = (0..12).map(|i| f("src/a.ts", i)).collect();
        let s = render(&Verdict::from_findings(many, 3).capped(10));
        assert!(s.contains("... and 2 more"));
    }
}
```

`crates/reporters/src/agent.rs` (tests at the bottom):
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use gate_core::finding::*;

    fn f(line: u32) -> Finding {
        Finding {
            id: format!("{:016x}", line),
            rule: "leftover-debug".into(),
            category: Category::Erosion,
            severity: Severity::High,
            confidence: Confidence::High,
            file: "src/a.ts".into(),
            span: Span { start_line: line, start_col: 0, end_line: line, end_col: 1 },
            evidence: "e".into(),
            fix: "f".into(),
            related: vec![],
            owasp: None,
            cwe: None,
        }
    }

    #[test]
    fn caps_at_ten_and_keeps_counts() {
        let v = Verdict::from_findings((0..15).map(f).collect(), 5);
        let s = render(&v);
        let parsed: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed["status"], "block");
        assert_eq!(parsed["high"], 15);
        assert_eq!(parsed["findings"].as_array().unwrap().len(), 10);
        assert_eq!(parsed["truncated"], 5);
        assert!(!s.contains('\n'));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd <repo> && cargo test -p reporters -q 2>&1 | tail -5
```
Expected: compile error, `render` not found.

- [ ] **Step 3: Implement both reporters**

`crates/reporters/src/terminal.rs` (prepend):
```rust
use std::collections::BTreeMap;

use gate_core::finding::{Finding, Status, Verdict};

fn status_word(s: Status) -> &'static str {
    match s {
        Status::Pass => "PASS",
        Status::Advisory => "ADVISORY",
        Status::Block => "BLOCK",
    }
}

fn sev(f: &Finding) -> String {
    format!("{:?}/{:?}", f.severity, f.confidence).to_lowercase()
}

pub fn render(v: &Verdict) -> String {
    let total = v.high + v.medium + v.low;
    let mut out = format!(
        "{}  {} finding(s): {} high, {} medium, {} low  ({} ms)\n",
        status_word(v.status), total, v.high, v.medium, v.low, v.duration_ms
    );
    let mut by_file: BTreeMap<&str, Vec<&Finding>> = BTreeMap::new();
    for f in &v.findings {
        by_file.entry(f.file.as_str()).or_default().push(f);
    }
    for (file, fs) in by_file {
        out.push_str(file);
        out.push('\n');
        for f in fs {
            out.push_str(&format!("  L{}  {}  {}  {}\n", f.span.start_line, f.rule, sev(f), f.evidence));
            out.push_str(&format!("        fix: {}\n", f.fix));
        }
    }
    if v.truncated > 0 {
        out.push_str(&format!("... and {} more\n", v.truncated));
    }
    out
}
```

`crates/reporters/src/agent.rs` (prepend):
```rust
use gate_core::finding::Verdict;

pub const CAP: usize = 10;

pub fn render(v: &Verdict) -> String {
    let capped = v.clone().capped(CAP);
    serde_json::to_string(&capped).expect("verdict serialises")
}
```

`crates/reporters/src/lib.rs`:
```rust
pub mod agent;
pub mod terminal;
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cd <repo> && cargo test -p reporters -q 2>&1 | tail -3
```
Expected: `3 passed`.

- [ ] **Step 5: Commit**

```bash
cd <repo> && git add crates/reporters/src/terminal.rs crates/reporters/src/agent.rs crates/reporters/src/lib.rs && git commit -m "engine: terminal and agent JSON reporters"
```

---

### Task 12: The pipeline and the CLI

**Files:**
- Create: `crates/cli/src/run.rs`
- Modify: `crates/cli/src/main.rs`
- Create: `crates/cli/tests/cli.rs`
- Create: `crates/cli/tests/fixtures/repo/` (files below)

**Interfaces:**
- Consumes: everything above.
- Produces: `run::Options { pub root: PathBuf, pub paths: Vec<PathBuf>, pub changed_only: bool, pub json: bool }`, `run::check(opts: &Options) -> anyhow::Result<Verdict>`: load config, walk (or use `paths` when given, filtered to supported languages), open index, for each file compute hash, decide changed, parse only files that are changed or explicitly requested (unchanged files are still counted as present), upsert file row and symbols for changed files, run rules over the files in scope (all walked files when `changed_only` is false; only changed files when true), apply the baseline filter, build the verdict with elapsed time. Prints one stderr warning per file with parse errors. `run::scan(root) -> anyhow::Result<(usize, usize)>` indexes every file and returns (files, changed). `run::baseline_create(root) -> anyhow::Result<usize>` runs a full check ignoring the existing baseline and writes every finding into it, returning the count. `run::baseline_accept(root, id, reason) -> anyhow::Result<bool>` finds the finding by id in a full check and accepts it.
- CLI (clap derive): `gate check [PATHS...] [--changed] [--json]`, `gate scan`, `gate baseline create`, `gate baseline accept <ID> --reason <TEXT>`. `--root` optional on every command, defaulting to the current directory. Exit codes from `Verdict::exit_code`; any `Err` prints `error: {message}` to stderr and exits 2.

- [ ] **Step 1: Write the fixture repo**

`crates/cli/tests/fixtures/repo/package.json`:
```json
{ "name": "repo" }
```

`crates/cli/tests/fixtures/repo/src/clean.ts`:
```ts
export function ok(): number {
  return 1;
}
```

`crates/cli/tests/fixtures/repo/src/dirty.ts`:
```ts
export function bad(): number {
  console.log("debug");
  // TODO clean this up
  return 2;
}
```

- [ ] **Step 2: Write the failing end-to-end tests**

`crates/cli/tests/cli.rs`:
```rust
use std::path::PathBuf;
use std::process::Command;

use assert_cmd::prelude::*;

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/repo")
}

fn copy_fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for entry in walkdir(&fixture()) {
        let rel = entry.strip_prefix(fixture()).unwrap();
        let dest = dir.path().join(rel);
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        std::fs::copy(&entry, &dest).unwrap();
    }
    dir
}

fn walkdir(root: &std::path::Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(root).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            out.extend(walkdir(&p));
        } else {
            out.push(p);
        }
    }
    out
}

fn gate(dir: &std::path::Path) -> Command {
    let mut c = Command::cargo_bin("gate").unwrap();
    c.current_dir(dir).env("GATE_CACHE_DIR", dir.join(".cache"));
    c
}

#[test]
fn check_blocks_on_debug_and_reports_marker() {
    let dir = copy_fixture();
    let out = gate(dir.path()).arg("check").output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.starts_with("BLOCK  2 finding(s)"), "{text}");
    assert!(text.contains("src/dirty.ts"));
    assert!(text.contains("leftover-debug"));
    assert!(text.contains("leftover-agent-marker"));
}

#[test]
fn json_output_is_compact_and_capped() {
    let dir = copy_fixture();
    let out = gate(dir.path()).args(["check", "--json"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["status"], "block");
    assert_eq!(v["blocking"], 1);
    assert_eq!(v["findings"][0]["rule"], "leftover-debug");
    assert!(v["findings"][0].get("source").is_none());
}

#[test]
fn baseline_create_then_check_passes_and_changed_only_sees_edits() {
    let dir = copy_fixture();
    gate(dir.path()).args(["baseline", "create"]).assert().success();
    assert!(dir.path().join("gate-baseline.json").exists());
    gate(dir.path()).arg("check").assert().code(0);

    // no edits since the last index: --changed finds nothing to check
    let out = gate(dir.path()).args(["check", "--changed"]).output().unwrap();
    assert!(String::from_utf8(out.stdout).unwrap().starts_with("PASS  0 finding(s)"));

    // introduce a new debug line in clean.ts; only that file is in scope and it blocks
    std::fs::write(dir.path().join("src/clean.ts"), "export function ok(): number {\n  console.log(\"new\");\n  return 1;\n}\n").unwrap();
    let out = gate(dir.path()).args(["check", "--changed"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("src/clean.ts"));
    assert!(!text.contains("src/dirty.ts"));
}

#[test]
fn explicit_path_limits_scope() {
    let dir = copy_fixture();
    gate(dir.path()).args(["check", "src/clean.ts"]).assert().code(0);
}

#[test]
fn baseline_accept_by_id() {
    let dir = copy_fixture();
    let out = gate(dir.path()).args(["check", "--json"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let id = v["findings"][0]["id"].as_str().unwrap().to_string();
    gate(dir.path()).args(["baseline", "accept", &id, "--reason", "legacy"]).assert().success();
    let out = gate(dir.path()).args(["check", "--json"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["blocking"], 0);
    assert_eq!(v["status"], "advisory");
}

#[test]
fn engine_error_exits_two() {
    let dir = copy_fixture();
    std::fs::write(dir.path().join("gate.toml"), "excludes = [").unwrap();
    let out = gate(dir.path()).arg("check").output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8(out.stderr).unwrap().starts_with("error:"));
}
```

- [ ] **Step 3: Run tests to verify they fail**

```bash
cd <repo> && cargo test -p cli -q 2>&1 | tail -5
```
Expected: failures (the binary prints the engine name and exits 0, so every assertion fails).

- [ ] **Step 4: Implement run.rs**

`crates/cli/src/run.rs`:
```rust
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::Context;
use gate_core::baseline::Baseline;
use gate_core::config::Config;
use gate_core::finding::{Finding, Verdict};
use gate_core::index::{content_hash, Index};
use gate_core::lang::Language;
use gate_core::parse::{parse_source, rel_path, ParsedFile};
use gate_core::symbols;
use gate_core::walk::{source_files, WalkOptions};
use gate_rules::{run_all, RuleContext};

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

fn candidate_files(root: &Path, paths: &[PathBuf], config: &Config) -> anyhow::Result<Vec<PathBuf>> {
    if paths.is_empty() {
        let opts = WalkOptions { excludes: config.excludes.clone() };
        return source_files(root, &opts);
    }
    let mut out = Vec::new();
    for p in paths {
        let abs = if p.is_absolute() { p.clone() } else { root.join(p) };
        if abs.is_dir() {
            let opts = WalkOptions { excludes: config.excludes.clone() };
            out.extend(source_files(&abs, &opts)?);
        } else if Language::from_path(&abs).is_some() {
            out.push(abs);
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

/// Reads and hashes every candidate, parses the changed ones (or all when `parse_all`),
/// updates the index, and returns the parsed files that are in scope.
fn index_files(root: &Path, candidates: &[PathBuf], parse_all: bool, ix: &mut Index) -> anyhow::Result<Indexed> {
    let mut files = Vec::new();
    let mut present = Vec::new();
    let mut changed = 0;
    for path in candidates {
        let rel = rel_path(root, path);
        let source = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let hash = content_hash(&source);
        let is_changed = ix.changed(&rel, &hash)?;
        present.push(rel.clone());
        if !is_changed && !parse_all {
            continue;
        }
        let Some(parsed) = parse_source(path, &rel, source) else { continue };
        let status = if parsed.has_error { "error" } else { "ok" };
        if parsed.has_error {
            eprintln!("warning: parse errors in {rel}; skipped");
        }
        if is_changed {
            changed += 1;
            ix.upsert_file(&rel, parsed.language.as_str(), &hash, status)?;
            let syms = symbols::extract(&parsed);
            symbols::store(ix, &parsed, &syms)?;
        }
        if is_changed || parse_all {
            files.push(parsed);
        }
    }
    if candidates.len() > 1 {
        // only prune when we walked the whole repo; explicit paths must not delete other files' rows
        let _ = present;
    }
    Ok(Indexed { files, changed })
}

fn full_findings(root: &Path, opts: &Options) -> anyhow::Result<(Vec<Finding>, Config)> {
    let config = Config::load(root)?;
    let candidates = candidate_files(root, &opts.paths, &config)?;
    let mut ix = Index::open(root)?;
    let parse_all = !opts.changed_only;
    let indexed = index_files(root, &candidates, parse_all, &mut ix)?;
    if opts.paths.is_empty() && !opts.changed_only {
        let present: Vec<String> = candidates.iter().map(|p| rel_path(root, p)).collect();
        ix.remove_missing(&present)?;
    }
    let ctx = RuleContext { files: &indexed.files, config: &config };
    Ok((run_all(&ctx), config))
}

pub fn check(opts: &Options) -> anyhow::Result<Verdict> {
    let started = Instant::now();
    let root = opts.root.clone();
    let (findings, _) = full_findings(&root, opts)?;
    let baseline = Baseline::load(&root)?;
    let findings = baseline.filter(findings);
    Ok(Verdict::from_findings(findings, started.elapsed().as_millis()))
}

pub fn scan(root: &Path) -> anyhow::Result<(usize, usize)> {
    let config = Config::load(root)?;
    let candidates = candidate_files(root, &[], &config)?;
    let mut ix = Index::open(root)?;
    let indexed = index_files(root, &candidates, false, &mut ix)?;
    let present: Vec<String> = candidates.iter().map(|p| rel_path(root, p)).collect();
    ix.remove_missing(&present)?;
    Ok((candidates.len(), indexed.changed))
}

pub fn baseline_create(root: &Path) -> anyhow::Result<usize> {
    let opts = Options { root: root.to_path_buf(), paths: vec![], changed_only: false, json: false };
    let (findings, _) = full_findings(root, &opts)?;
    let mut b = Baseline::default();
    for f in &findings {
        b.accept(f, "baseline", "gate");
    }
    b.save(root)?;
    Ok(findings.len())
}

pub fn baseline_accept(root: &Path, id: &str, reason: &str) -> anyhow::Result<bool> {
    let opts = Options { root: root.to_path_buf(), paths: vec![], changed_only: false, json: false };
    let (findings, _) = full_findings(root, &opts)?;
    let mut b = Baseline::load(root)?;
    let Some(f) = findings.iter().find(|f| f.id == id) else { return Ok(false) };
    let author = std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_else(|_| "unknown".into());
    b.accept(f, reason, &author);
    b.save(root)?;
    Ok(true)
}
```

- [ ] **Step 5: Implement main.rs**

`crates/cli/src/main.rs`:
```rust
mod run;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "gate", about = "Deterministic quality gate for code written by people and agents")]
struct Cli {
    /// Repository root (defaults to the current directory)
    #[arg(long, global = true)]
    root: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Check files (all by default) and print a verdict
    Check {
        paths: Vec<PathBuf>,
        /// Only files whose content changed since the last index
        #[arg(long)]
        changed: bool,
        /// Compact JSON for agents (capped at ten findings)
        #[arg(long)]
        json: bool,
    },
    /// Index the repository without running rules
    Scan,
    /// Manage the baseline of accepted findings
    Baseline {
        #[command(subcommand)]
        cmd: BaselineCmd,
    },
}

#[derive(Subcommand)]
enum BaselineCmd {
    /// Snapshot every current finding into the baseline
    Create,
    /// Accept one finding by id with a reason
    Accept {
        id: String,
        #[arg(long)]
        reason: String,
    },
}

fn real_main() -> anyhow::Result<i32> {
    let cli = Cli::parse();
    let root = match cli.root {
        Some(r) => r,
        None => std::env::current_dir()?,
    };
    match cli.cmd {
        Cmd::Check { paths, changed, json } => {
            let opts = run::Options { root, paths, changed_only: changed, json };
            let verdict = run::check(&opts)?;
            if json {
                println!("{}", gate_reporters::agent::render(&verdict));
            } else {
                print!("{}", gate_reporters::terminal::render(&verdict));
            }
            Ok(verdict.exit_code())
        }
        Cmd::Scan => {
            let (files, changed) = run::scan(&root)?;
            println!("indexed {files} file(s), {changed} changed");
            Ok(0)
        }
        Cmd::Baseline { cmd: BaselineCmd::Create } => {
            let n = run::baseline_create(&root)?;
            println!("baseline written with {n} finding(s)");
            Ok(0)
        }
        Cmd::Baseline { cmd: BaselineCmd::Accept { id, reason } } => {
            if run::baseline_accept(&root, &id, &reason)? {
                println!("accepted {id}");
                Ok(0)
            } else {
                eprintln!("error: no current finding with id {id}");
                Ok(2)
            }
        }
    }
}

fn main() -> ExitCode {
    let result = std::panic::catch_unwind(real_main);
    match result {
        Ok(Ok(code)) => ExitCode::from(code as u8),
        Ok(Err(e)) => {
            eprintln!("error: {e:#}");
            ExitCode::from(2)
        }
        Err(_) => {
            eprintln!("error: internal engine failure");
            ExitCode::from(2)
        }
    }
}
```

Add `serde_json` is already a dependency of `cli`; `assert_cmd` and `tempfile` are dev-dependencies from Task 1.

- [ ] **Step 6: Run tests to verify they pass**

```bash
cd <repo> && cargo test -p cli -q 2>&1 | tail -5
```
Expected: `6 passed`. Two likely trips: (a) `check_blocks_on_debug_and_reports_marker` expects exactly 2 findings; if `leftover-commented-code` fires on the fixture, the fixture has no three-line comment run so investigate the rule rather than the fixture; (b) on Windows `USERNAME` is set, on other hosts `USER`, both handled.

- [ ] **Step 7: Run the whole workspace once**

```bash
cd <repo> && cargo test -q 2>&1 | grep -E "test result|error" | head
```
Expected: every crate reports `ok`.

- [ ] **Step 8: Commit**

```bash
cd <repo> && git add crates/cli/src/run.rs crates/cli/src/main.rs crates/cli/tests/cli.rs crates/cli/tests/fixtures/repo && git commit -m "engine: check, scan, and baseline commands"
```

---

### Task 13: Benchmark tests against the spec targets

**Files:**
- Create: `crates/cli/tests/bench.rs`
- Modify: `crates/cli/Cargo.toml` (add `[profile.release] lto = "thin"` at the workspace root `Cargo.toml` instead)

**Interfaces:**
- Consumes: the `gate` binary.
- Produces: three `#[ignore]` tests that fail when a spec target is missed, run with `cargo test --release -p cli -- --ignored`.

- [ ] **Step 1: Write the benchmark tests**

`crates/cli/tests/bench.rs`:
```rust
//! Spec section 3.4 targets. Run: cargo test --release -p cli -- --ignored --nocapture
//! Requires the founder's FastLift checkout at <home>/fasting-app (override with GATE_BENCH_REPO).

use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;

use assert_cmd::prelude::*;

fn repo() -> PathBuf {
    PathBuf::from(std::env::var("GATE_BENCH_REPO").unwrap_or_else(|_| "<home>/fasting-app".into()))
}

fn gate(cache: &std::path::Path) -> Command {
    let mut c = Command::cargo_bin("gate").unwrap();
    c.current_dir(repo()).env("GATE_CACHE_DIR", cache);
    c
}

#[test]
#[ignore]
fn cold_index_under_five_seconds() {
    let cache = tempfile::tempdir().unwrap();
    let t = Instant::now();
    gate(cache.path()).arg("scan").assert().success();
    let ms = t.elapsed().as_millis();
    println!("cold scan: {ms} ms");
    assert!(ms < 5_000, "cold index took {ms} ms");
}

#[test]
#[ignore]
fn warm_single_file_check_under_300ms() {
    let cache = tempfile::tempdir().unwrap();
    gate(cache.path()).arg("scan").assert().success();
    let file = "app/_layout.tsx";
    let t = Instant::now();
    let _ = gate(cache.path()).args(["check", file]).output().unwrap();
    let ms = t.elapsed().as_millis();
    println!("warm single-file check: {ms} ms");
    assert!(ms < 300, "warm check took {ms} ms");
}

#[test]
#[ignore]
fn startup_under_50ms() {
    let t = Instant::now();
    let _ = Command::cargo_bin("gate").unwrap().arg("--help").output().unwrap();
    let ms = t.elapsed().as_millis();
    println!("startup: {ms} ms");
    assert!(ms < 50, "startup took {ms} ms");
}
```

Add to the workspace `Cargo.toml`:
```toml
[profile.release]
lto = "thin"
codegen-units = 1
```

- [ ] **Step 2: Run the benchmarks in release mode**

```bash
cd <repo> && cargo test --release -p cli -- --ignored --nocapture 2>&1 | grep -E "ms|test result"
```
Expected: three timings printed and `3 passed`. If `cold_index_under_five_seconds` fails, the likely cost is parsing every file on the first scan; the fix is to parallelise `index_files` across files with `std::thread::scope` chunks (rules still run single-threaded), not to relax the target. Record the numbers in the task report either way.

- [ ] **Step 3: Commit**

```bash
cd <repo> && git add crates/cli/tests/bench.rs Cargo.toml && git commit -m "engine: benchmark tests for the spec performance targets"
```

---

## Self-review

**Spec coverage.** Section 3 architecture: four crates, one binary (Task 1). 3.1 parsing with degraded unparsed files (Tasks 2, 12). 3.2 index in the cache dir with files, symbols, meta; incremental by content hash; schema rebuild on mismatch (Tasks 4, 5, 12). Edges, fingerprints and findings_cache tables are deferred to plan 2 with the graph rules; the schema version bump handles the migration. 3.4 performance targets as failing tests (Task 13). 3.5 baseline (Tasks 7, 12). 4.1 rules in this plan: `leftover-debug`, `leftover-commented-code`, `leftover-agent-marker` (Tasks 9, 10); the rest are named for plans 2 and 3. 4.3 blocking policy: only high confidence blocks (Task 6); per-rule overrides (Task 7); the `secret-exposed` exception belongs to plan 3 with that rule. Section 7 output contract: finding record fields, stable id, verdict, agent JSON capped at ten, terminal reporter, exit codes (Tasks 6, 11, 12); SARIF is plan 2 with the CI action per spec 8.1. Config file and baseline file formats (Task 7). Section 9 error handling: parse failure warning without findings (Task 12), schema mismatch rebuild (Task 4), panic to exit 2 (Task 12). The hook timeout rule (2 s returns pass) belongs with the hooks in plan 4.

**Placeholder scan.** No TBD or TODO outside the fixture files that deliberately contain the marker text under test. Every code step carries the code.

**Type consistency.** `Finding`, `Span`, `Severity`, `Confidence`, `Category`, `Status`, `Verdict` are defined once in Task 6 and used with the same field names in Tasks 7, 8, 9, 10, 11, 12. `ParsedFile` fields (`path`, `rel`, `language`, `source`, `tree`, `has_error`) defined in Task 2 and used in Tasks 5, 8, 9, 10, 12. `Index` methods `open`, `open_in_memory`, `conn`, `upsert_file`, `file_hash`, `changed`, `remove_missing` defined in Task 4 and used in Tasks 5 and 12. `symbols::{extract, store, enclosing_symbol}` defined in Task 5, used in Tasks 8 and 12. `Config::{load, rule_enabled, severity_for}` and `debug_allowed` defined in Task 7, used in Tasks 8, 9, 12. `Baseline::{load, save, accept, filter, contains}` defined in Task 7, used in Task 12. `run_rules`, `run_all`, `finding`, `line_text`, `anchor_for` defined in Task 8, used in Tasks 9, 10, 12 and the test helper. `Language::as_str` defined in Task 2, used in Task 12. Reporter functions `terminal::render` and `agent::render` defined in Task 11, used in Task 12. Crate library names `gate_core`, `gate_rules`, `gate_reporters` are set in Task 1 and used as import roots throughout.

**Known judgment calls for the executor.** JavaScript is parsed with the TypeScript grammar (Task 2 note). `index_files` prunes missing files only on a full walk, never on an explicit-path check (Task 12 code). The commented-code rule is heuristic and advisory; its fixtures pin the intended behaviour, not a formal definition.
