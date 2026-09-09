# Error, Test, and Security Rules Implementation Plan (version one, plan 3 of 4)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship the rest of spec 4.1: `swallowed-error`, `test-no-assert`, `test-newly-skipped`, and the security pack (`secret-exposed`, `weak-crypto`, `injection-sink`, `html-injection`, `vulnerable-dependency`, the two Supabase rules, the three Express rules), each with must-flag / must-not-flag / edge fixtures and a precision check on the founder's repositories.

**Architecture:** Part A (branch `engine/erosion`) adds what the erosion and test rules need: a `[framework]` config section, `root`, `offline`, and a `Previous` snapshot on `RuleContext`, a shared test-case extractor in core, a `skipped_tests` index table so `--changed` and diff scopes can tell "newly skipped" from "already skipped", and three rules. Part B (branch `engine/security`) adds the security pack: a secret pattern table with an entropy check and a Supabase JWT role decoder, three AST rules, lockfile parsers and an OSV client with an on-disk cache (the engine's first and only network call, off under `--offline` or on any failure, never blocking on the network), and the framework rules. Every rule is a pure function over `RuleContext`; the two rules that must read non-source files (`supabase-table-without-rls` reads SQL migrations, `vulnerable-dependency` reads a lockfile) do so through `ctx.root`, which is the one documented exception to "rules never touch the filesystem".

**Tech Stack:** Rust 2021, tree-sitter 0.23, rusqlite 0.32, blake3, serde, toml, globset, rayon, clap 4 (all present). New: `regex = "1"` (secret patterns), `ureq = { version = "2", features = ["json"] }` (OSV), `base64 = "0.22"` (JWT payload decode). Tests are `#[test]` functions over fixture directories.

**Spec:** `docs/superpowers/specs/2026-09-05-agent-native-quality-gate-design.md`, sections 4.1 (the eleven rule lines this plan implements, with their OWASP and CWE ids), 4.3 (blocking policy; `secret-exposed` always blocks and cannot be downgraded), 7.1 (`owasp` and `cwe` on findings), 7.5 (`framework hints`, `blocking policy`), 9 (OSV unreachable: cached snapshot with age warning, else skip with a warning, never block on network), 10.1 and 10.2 (fixtures per rule, 85 percent precision gate). Plans 1 and 2 are the code this builds on; read `crates/rules/src/lib.rs`, `crates/cli/src/run.rs`, and `crates/core/src/index.rs` before Task 1.

**What this plan does not do.** `init`, hooks, and the MCP server are plan 4. Cross-function and cross-file taint, `test-mock-only`, and the framework packs for Next.js, Expo, and Laravel are release two (spec 4.2). PHP and Python are phase two.

## Global Constraints

- Product name Locrin; config `locrin.toml` (`core::config::CONFIG_FILE`); baseline `locrin-baseline.json`. Cache directory from `core::index::cache_path`'s parent (`<cache>/locrin/<repo key>/`).
- No LLM anywhere. Network access exists in exactly one place, `core::osv::fetch`, used only by `vulnerable-dependency`, only when a lockfile exists, and never under `--offline`; every network failure degrades to the cached snapshot or to a warning, never to exit 1 or exit 2 (spec 9).
- Rule contract as of plan 2 Part B: `Rule: Sync` with `id`, `description`, `scope`, `category`, `default_severity`, `confidence`, `enabled_by_default`, `run -> anyhow::Result<Vec<Finding>>`; `RuleContext { files, config, index: Option<&Index>, entries }` (Task 1 adds `root`, `offline`, `previous`). `finding_at(rule, rel, span, anchor, evidence, fix)` builds every finding; `owasp` and `cwe` are set by the rule after construction. Ids stay `blake3(rule + "\x1f" + rel + "\x1f" + anchor)[..16]`.
- New rules and defaults (category Security unless stated):
  - `swallowed-error` (Erosion): severity Medium; confidence High for an empty catch and a floating promise, Medium for a log-only catch.
  - `test-no-assert` (Erosion): severity Low, confidence Medium.
  - `test-newly-skipped` (Erosion): severity Medium, confidence High.
  - `secret-exposed`: severity High, confidence High, `locked() == true` (config cannot lower its severity or disable it). OWASP `A02:2021`, CWE `CWE-798`.
  - `weak-crypto`: severity High; confidence High when the credential context is explicit, Medium otherwise. `A02:2021`, `CWE-327` (MD5, SHA-1, static IV) or `CWE-338` (Math.random).
  - `injection-sink`: severity High; confidence High for eval/new Function with any non-literal and for template or concatenated command and SQL strings, Medium for a bare identifier reaching a SQL sink. `A03:2021`, `CWE-95` / `CWE-78` / `CWE-89`.
  - `html-injection`: severity High, confidence Medium. `A03:2021`, `CWE-79`.
  - `vulnerable-dependency`: severity from the advisory (CRITICAL or HIGH to High, MODERATE to Medium, LOW or unknown to Low); confidence High when the advisory names a fixed version and its severity is High, Medium otherwise. `A06:2021`, `CWE-1395`.
  - `supabase-service-role-in-client`: High / High. `A01:2021`, `CWE-284`.
  - `supabase-table-without-rls`: High / High. `A01:2021`, `CWE-284`.
  - `express-route-without-auth`: High / High, runs only when `framework.auth_middleware` is non-empty. `A01:2021`, `CWE-306`.
  - `express-cors-wildcard-on-authenticated`: High / High. `A05:2021`, `CWE-942` (the spec lists 306 and 614 for the Express group; 942 is the accurate id for a permissive cross-domain policy and is recorded here as a deviation).
  - `express-cookie-insecure`: High / High. `A05:2021`, `CWE-614` (missing `secure`) or `CWE-1004` (missing `httpOnly`).
- Precision gate (spec 10.2, plan 2 precedent): each measurable rule is sampled 20 findings on the corpus (FastLift, StrongSpan, teyji, autoqa, fastlift-admin; whichever produce findings). Under 17/20 the rule ships `enabled_by_default() == false` with the reason in its doc comment and in the precision report. A rule that produces no findings on the corpus is unmeasured and ships on fixture evidence, stated as such. `secret-exposed` is the exception: it is `locked`, so a miss there STOPS for the founder.
- Evidence never contains a secret: `secret-exposed` prints the provider and a mask (first four characters, an ellipsis spelled `...`, the length).
- Schema bumps once, to `"4"`, in Task 3 (`skipped_tests`, `osv_batch`, `osv_vulns`); spec 9 rebuilds on mismatch.
- Performance targets stay tests (cold under 5 s, warm single-file under 300 ms, warm 30-file under 1 s, startup under 50 ms); the security rules are file rules and run in the parallel pass; `vulnerable-dependency` is a graph rule that runs every run and must cost under 50 ms warm (cached), measured in Task 16.
- Git: Part A on `engine/erosion` off `main`; Part B on `engine/security` off `main` after A merges (stacked if A is in review). One commit per task, `cargo fmt --all` before every commit, plain `engine: ...` messages, no attribution trailers, never `git add -A`. No em dashes anywhere.
- `export PATH="$HOME/.cargo/bin:$PATH"` before any cargo command. Corpus checkouts: `<home>/fasting-app`, `<home>/strongspan`, `<home>/teyji`, `<home>/autoqa`, `<home>/fastlift-admin`. Never write into them; use a fresh `LOCRIN_CACHE_DIR` per measurement.
- Expected fixture line numbers were counted by hand; if a test fails only on a line number, recount against the fixture before touching the rule.

## Deviations recorded during execution

(Empty at planning time. The executor appends every ruling here with its reason, as plan 2 did.)

- **Task 3, the git source reads only test files.** The plan says a diff scope runs `show_at` and `parse_source` over *each* scope file. `Previous` holds nothing but `skipped_tests`, so for a file that is not a test file both paths produce the empty set, and reading the rest would fetch and parse every changed file a second time (they are all parsed at HEAD by the same run) to learn nothing: on a large pull request that doubles the parse cost of the `--base` gate. `run::skipped_at` therefore returns early for a non-test path, and its doc comment says that a later field needing the whole previous file widens it. Observably identical for every rule in this plan.

## File structure

Part A:
- `crates/core/src/config.rs` (modify): `[framework]` section (`auth_middleware`, `server_paths`).
- `crates/core/src/tests.rs` (create): test-case extractor (`TestCase`, `extract`, `is_test_file`).
- `crates/core/src/index.rs` (modify): schema "4", `skipped_tests`, `osv_batch`, `osv_vulns` tables; `replace_skipped_tests`, `skipped_tests`.
- `crates/core/src/indexer.rs` (modify): records skipped tests for test files.
- `crates/core/src/previous.rs` (create): `Previous` snapshot type.
- `crates/rules/src/lib.rs` (modify): `RuleContext { root, offline, previous }`, `Rule::locked`, `run_file_rules(rules, files, base)`.
- `crates/cli/src/git.rs` (modify): `show_at(root, rev, rel) -> Result<Option<String>>`.
- `crates/cli/src/run.rs`, `main.rs` (modify): `--offline`, `Previous` capture from the index and from git.
- `crates/rules/src/{swallowed_error,test_no_assert,test_newly_skipped}.rs` (create) with tests and fixtures.
- `crates/rules/tests/common/mod.rs` (modify): `run_on_with(rule, root, config, previous)`.

Part B:
- `crates/rules/src/secrets/{mod,patterns,entropy,jwt}.rs` (create): `secret-exposed`.
- `crates/rules/src/{weak_crypto,injection_sink,html_injection}.rs` (create).
- `crates/core/src/lockfile.rs` (create): npm, yarn v1, pnpm parsers.
- `crates/core/src/osv.rs` (create): batch query, vuln detail, cache in the index, offline behaviour.
- `crates/rules/src/vulnerable_dependency.rs` (create).
- `crates/rules/src/supabase/{mod,service_role,rls}.rs` (create).
- `crates/rules/src/express/{mod,route_auth,cors,cookie}.rs` (create).
- `crates/cli/tests/cli.rs` (modify): end-to-end tests; `docs/superpowers/plans/2026-09-09-error-test-and-security-precision.md` (create).

---

## Part A: context, test extractor, and three erosion rules (branch `engine/erosion`)

### Task 1: Config `[framework]`, `RuleContext` fields, `Rule::locked`, `--offline`

**Files:**
- Modify: `crates/core/src/config.rs`, `crates/core/src/lib.rs`, `crates/core/src/previous.rs` (create), `crates/rules/src/lib.rs`, `crates/rules/tests/common/mod.rs`, `crates/cli/src/run.rs`, `crates/cli/src/main.rs`, every existing `RuleContext { .. }` literal.

**Interfaces:**
```rust
// core::config
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Framework {
    /// Identifiers that mark a route as authenticated, e.g. ["requireAuth", "authenticate"].
    pub auth_middleware: Vec<String>,
    /// Globs for code that runs on a server and may hold a Supabase service-role key.
    pub server_paths: Vec<String>,
}
impl Default for Framework { /* auth_middleware: [], server_paths: ["supabase/functions/**", "server/**", "api/**", "scripts/**", "**/*.server.*", "**/*.test.*", "**/*.spec.*"] */ }
// Config gains: pub framework: Framework
// Config::severity_for and rule_enabled_or are unchanged; locking is applied in run_rules.

// core::previous
#[derive(Debug, Default, Clone)]
pub struct Previous {
    /// rel -> names of test cases that were skipped in the previous version of the file.
    /// A file absent from the map has no known previous version: everything in it is new.
    pub skipped_tests: HashMap<String, HashSet<String>>,
}

// rules
pub struct RuleContext<'a> {
    pub files: &'a [ParsedFile],
    pub config: &'a Config,
    pub index: Option<&'a Index>,
    pub entries: &'a EntryPoints,
    pub root: &'a Path,
    pub offline: bool,
    pub previous: &'a Previous,
}
// derive Clone, Copy on RuleContext
pub trait Rule: Sync {
    /* existing */
    /// A locked rule ignores config overrides: it cannot be disabled and its severity cannot be lowered (spec 4.3, secret-exposed).
    fn locked(&self) -> bool { false }
}
pub fn run_file_rules(rules: &[Box<dyn Rule>], files: &[ParsedFile], base: &RuleContext) -> anyhow::Result<Vec<Finding>>
// per-file context = RuleContext { files: slice::from_ref(file), index: None, ..*base }
```
`run_rules`: when `rule.locked()`, the rule is always enabled and keeps `default_severity()`; otherwise as today. `Options` gains `pub offline: bool`; `check` and `scan` take `--offline` ("Never touch the network; use the cached advisory snapshot or skip vulnerable-dependency with a warning"). `common::run_on(rule, root, config)` keeps its signature and passes `root = fixture root`, `offline = true`, `previous = &Previous::default()`; add `run_on_with(rule, root, config, previous: &Previous)`.

- [ ] Step 1: failing tests. In `config.rs` tests: `parses_framework_section` (toml with `[framework]\nauth_middleware = ["requireAuth"]\nserver_paths = ["backend/**"]`; assert both; assert `Framework::default().server_paths` contains `"supabase/functions/**"` and `"**/*.test.*"`) and `an_unknown_framework_key_is_an_error`. In `rules/src/lib.rs` tests: a `Locked` test rule (`locked() == true`, severity High) under a config that sets `enabled = false` and `severity = "low"` still produces one High finding; an unlocked rule under the same config produces none. A test that `run_file_rules` per-file contexts carry `root`, `offline`, `previous` from the base (a test rule that returns a finding whose evidence is `ctx.root.display()` and `ctx.offline`).
- [ ] Step 2: run, see them fail to compile.
- [ ] Step 3: implement. Add `pub mod previous;` to core. Update every `RuleContext` literal (`rules/src/lib.rs` tests, `tests/common/mod.rs`, `run.rs`) with the three new fields. In `run.rs` pass `root`, `opts.offline`, and (for now) `&Previous::default()`; `Options` literals in `scan`, `baseline_create`, `baseline_accept` get `offline: false`... `scan` takes an `offline` parameter from main. `main.rs`: `--offline` on `Check` and `Scan`.
- [ ] Step 4: `cargo test --workspace` green; commit `engine: framework config, rule locking, offline flag, and a previous-state slot on the rule context`.

### Task 2: Test-case extractor in core

**Files:** create `crates/core/src/tests.rs` (module name `tests` collides with nothing at crate level but shadows nothing either; name it `testcases` if `mod tests` inside files confuses the reader: use `crates/core/src/testcases.rs`, `pub mod testcases;`).

**Interfaces:**
```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestCase {
    pub name: String,      // first argument's literal text, or "<unnamed>"
    pub line: u32,         // line of the it/test call
    pub end_line: u32,
    pub skipped: bool,     // it.skip, test.skip, xit, xtest, it.todo, test.todo, or inside describe.skip / xdescribe
    pub assertions: u32,   // expect(...) / expect.x / assert(...) / assert.x / chai .should inside the callback, plus calls to same-file helpers that contain any of those (one level)
}
pub fn is_test_file(rel: &str) -> bool   // *.test.*, *.spec.*, /__tests__/ anywhere, e2e/**/*.ts? no: the first three only
pub fn extract(file: &ParsedFile) -> Vec<TestCase>
```
Detection rules (tree-sitter TSX/TS):
- A test call is a `call_expression` whose `function` is: identifier in {`it`, `test`, `xit`, `xtest`, `fit`, `ftest`}; or `member_expression` with object identifier in {`it`, `test`} and property in {`skip`, `only`, `todo`, `failing`, `concurrent`}; or a `call_expression` (the `it.each(table)` form) whose function is `member_expression` `it.each`/`test.each`, in which case the outer call carries the name and callback.
- Skipped: callee `xit`/`xtest`, property `skip` or `todo`, or any ancestor `call_expression` whose function is `describe.skip`, `xdescribe`, `context.skip`.
- Name: first argument `string` (unquoted) or `template_string` raw text between backticks; else `<unnamed>`.
- Assertions: walk the second argument (arrow/function) counting `call_expression` whose function is identifier `expect`/`assert` or member_expression whose object (recursively, take the leftmost identifier) is `expect`/`assert`; plus `member_expression` with property `should`; plus calls to identifiers that name a same-file function whose body contains such an assertion (compute the helper set first). `expect.assertions(n)` and `expect.hasAssertions()` count as 1.
- Tests (inline `#[cfg(test)]` on a TSX source): a file with `it`, `test.skip`, `xit`, `it.todo`, `it.each([...])(...)`, a `describe.skip` block containing an `it`, a helper `function checkShape(x) { expect(x).toBeDefined(); }` used by a case with no direct expect, a case with `assert.equal`, and a case with zero assertions; assert the vector of `(name, line, skipped, assertions)` exactly. `is_test_file` cases.
- Commit `engine: test-case extractor`.

### Task 3: Schema "4", skipped-test memory, `Previous` capture from the index and from git

**Files:** modify `crates/core/src/index.rs` (schema "4": `skipped_tests(rel TEXT, name TEXT, PRIMARY KEY(rel, name))`, `osv_batch(lock_hash TEXT PRIMARY KEY, fetched_at INTEGER, json TEXT)`, `osv_vulns(id TEXT PRIMARY KEY, fetched_at INTEGER, json TEXT)`; `PER_FILE_TABLES` gains `skipped_tests`; `replace_skipped_tests(rel, names)`, `skipped_tests(rel) -> Result<HashSet<String>>`), `crates/core/src/indexer.rs` (`record_with_stat` stores `testcases::extract(file)` skipped names when `testcases::is_test_file(&file.rel)`, else replaces with empty), `crates/cli/src/git.rs` (`pub fn show_at(root, rev, rel) -> Result<Option<String>>` running `git show --end-of-options <rev>:<path-from-top>` with `current_dir(top)`, `None` when git exits non-zero because the path did not exist at that rev), `crates/cli/src/run.rs`.

Pipeline rule (`run.rs`): build `Previous` before rules run.
- For every candidate that `is_changed` and had a `files` row before recording (`ix.file_hash(rel)` was `Some`), read `ix.skipped_tests(rel)` BEFORE `record_with_stat` and insert into `previous.skipped_tests`. This is the "index" source and it works for whole-repo, `--changed`, and named paths alike (the capture is per changed file, not per scope).
- For a diff scope (`--base`, `--since`), the git source overrides: for each scope file, `git::show_at(root, base_rev, rel)` where `base_rev` is the merge base for `--base` and `<ref>` for `--since`; parse with `parse_source`, extract, insert (an absent file at the base inserts an empty set: everything is new). Expose `git::base_rev(root, &DiffScope) -> Result<String>` (merge-base or the ref itself) so `changed_files` and this share it.
- Everything else (unchanged files served from cache, files never indexed) has no entry.
- Tests: index round trip for `replace_skipped_tests`/`skipped_tests`; `record_with_stat` stores skipped names for a `.test.ts` and nothing for a non-test file; `show_at` unit test in `cli.rs` (git init, commit, edit, `show_at(HEAD)` returns the committed text; a path absent at HEAD returns `None`). The `Previous` behaviour itself is covered by Task 6's e2e tests.
- Commit `engine: remember skipped tests per file and snapshot the previous version for changed files`.

### Task 4: `swallowed-error`

**Files:** create `crates/rules/src/swallowed_error.rs`, `crates/rules/tests/swallowed_error.rs`, fixtures `crates/rules/tests/fixtures/swallowed_error/{flag,clean,edge}/`; register.

Rule (File scope, Erosion, Medium):
1. **Empty catch** (confidence High): `catch_clause` whose `body` has no named children other than `comment`. Evidence: the trimmed `catch` line. Fix: `Handle the error, rethrow it, or log it with enough context to act on; an empty catch hides failures`.
2. **Log-only catch whose result is used** (confidence Medium): the catch body consists only of expression statements that are `console.<anything>(...)` calls, with no `throw` and no `return`; the enclosing function (nearest `function_declaration`, `method_definition`, or `variable_declarator` with an arrow/function value) has a name, and that name is called elsewhere in the same file in a position whose parent is not an `expression_statement` (its result is used). Evidence: `catch in <name> only logs; callers use its result`. Fix: `Return a failure value or rethrow; a caller that receives undefined cannot tell an error from an empty result`.
3. **Floating promise** (confidence High): an `expression_statement` whose expression is a `call_expression` (possibly wrapped in nothing else) whose callee is an identifier naming a same-file `async` function (`function_declaration` with the `async` keyword, or `variable_declarator` whose value is an `arrow_function`/`function_expression` with `async`), or a member call `this.<name>(...)`/`<obj>.<name>(...)` where `<name>` is a same-file async method or function; and the expression is not `await`-ed, not `void`-ed, and not followed by `.then(`/`.catch(`/`.finally(` in the same expression. Evidence: the statement text. Fix: `` await it, or attach .catch, or mark it `void` if the result is deliberately dropped ``.
Use `finding(self, file, line, ...)` and set confidence per form by overriding the field after construction (`f.confidence = Confidence::Medium` for form 2).

Fixtures (flag/a.ts, 8 findings expected in this order; count lines carefully):
```ts
import { readFile } from "node:fs/promises";

export async function loadConfig(path: string): Promise<string> {
  try {
    return await readFile(path, "utf8");
  } catch (e) {}
  return "";
}

export function parseCount(text: string): number {
  try {
    return JSON.parse(text).count;
  } catch (err) {
    console.error("bad json", err);
  }
}

export function total(): number {
  const n = parseCount("{}");
  return n + 1;
}

export async function refresh(): Promise<void> {
  await loadConfig("x");
}

export function boot(): void {
  refresh();
  loadConfig("y");
}
```
Expected: line 6 (empty catch, High), line 13 (log-only catch in `parseCount`, used by `total`, Medium), line 27 (`refresh();` floating, High), line 28 (`loadConfig("y");` floating, High). Assert `(line, confidence)` pairs exactly and the three evidence strings.
clean/b.ts: a catch that rethrows; a catch that logs AND returns null in a function whose callers use the result; a log-only catch in a function nobody calls with a used result (called as a statement); `await refresh()`, `void refresh()`, `refresh().catch(() => {})`, `refresh().then(done)`; a call to a non-async same-file function as a statement; a call to an imported function as a statement (unknown, not flagged).
edge/c.ts: an empty catch with `// locrin:allow` on the catch line (suppressed); a catch containing only a comment (`// ignore: best effort`) is still empty (flagged: comments are not handling; state this in the module doc); a `try/finally` with no catch (nothing); an async arrow `const ping = async () => {}` called as `ping();` (flagged). Expected edge: lines of the comment-only catch and the `ping();` statement.

- Register `swallowed-error` after `boundary-violation`; registry test lists nine ids. Commit `engine: swallowed-error rule`.

### Task 5: `test-no-assert`

**Files:** create `crates/rules/src/test_no_assert.rs`, test, fixtures `test_no_assert/{flag,clean,edge}/`; register.

Rule (File, Erosion, Low, Medium): for each `clean_files` file where `testcases::is_test_file(rel)`, for each `TestCase` with `assertions == 0` and `!skipped`: finding at `case.line`, anchor `"case\x1f{name}"`, evidence `` test "{name}" has no assertion ``, fix `Assert on the outcome, or mark the case as a smoke test with expect.assertions(0) so the intent is explicit`. A test file with no cases produces nothing.
Fixtures: `flag/a.test.ts` (three cases: one with no assertion, one with only `render(<X/>)`, one with an assertion; expect two findings with names); `clean/b.spec.tsx` (expect via helper, `assert.deepEqual`, chai `.should`, `expect.assertions(1)`, a skipped case with no assertion is not flagged, `it.todo`); `edge/__tests__/c.ts` (a case whose body calls a same-file helper that calls `expect` one level down: not flagged; a case calling a helper that calls another helper that asserts: flagged, two levels is beyond the extractor, state so in the doc). Commit `engine: test-no-assert rule`.

### Task 6: `test-newly-skipped`

**Files:** create `crates/rules/src/test_newly_skipped.rs`, test, fixtures `test_newly_skipped/{flag,clean}/`; register; two e2e tests in `crates/cli/tests/cli.rs`.

Rule (File, Erosion, Medium, High): for each test file, for each `TestCase` with `skipped`: `let was = ctx.previous.skipped_tests.get(rel)`; flagged unless `was.is_some_and(|s| s.contains(&name))`. Anchor `"skip\x1f{name}"`. Evidence `` test "{name}" is skipped `` (append ` (newly)` only when a previous version was known). Fix: `Re-enable the test or delete it; a skipped test is a decision that needs an owner`. Module doc states the three sources of "previous": the index (files edited since the last run), git (diff scopes), and none (first run or a never-indexed file: every skipped test is reported once and the baseline absorbs the legacy).
Rules-crate tests use `run_on_with` with a hand-built `Previous`: flag fixture with `Previous::default()` reports both skipped cases; the same fixture with a `Previous` that lists one of them reports only the other.
e2e (`cli.rs`): (a) `--changed`: fixture gets `src/a.test.ts` with an active test; `check`; edit to `it.skip`; `check --changed --json` reports `test-newly-skipped`; run `check --changed` again after touching an unrelated file: nothing (the skip is no longer new... careful: the second run's candidate `a.test.ts` is unchanged, served from cache, which holds the first finding; assert instead that after a second edit that keeps the skip, the finding is gone). (b) `--base`: git init, commit with an active test, change to `it.skip` uncommitted, `check --base HEAD --json` reports it; commit, `check --base HEAD` reports nothing. Commit `engine: test-newly-skipped rule`.

### Task 7: Part A precision check and pull request

Run the release binary over the corpus repos (fresh cache each), sample 20 findings per new rule, label by reading the code, write `docs/superpowers/plans/2026-09-09-error-test-and-security-precision.md` with one table per rule, totals, and the gate verdict; apply the gate (below 17/20 ships off by default, reason in the rule's doc comment). Re-run the four benchmarks three times (all must pass). `cargo test --workspace`. Commit, push `engine/erosion`, open the PR against `main` with the precision report as the body.

---

## Part B: the security pack (branch `engine/security`)

### Task 8: `secret-exposed`

**Files:** create `crates/rules/src/secrets/mod.rs`, `patterns.rs`, `entropy.rs`, `jwt.rs`; test `crates/rules/tests/secret_exposed.rs`; fixtures `secret_exposed/{flag,clean,edge}/`. Add `regex` and `base64` to the rules crate (workspace deps).

Design:
- `patterns.rs`: `pub struct Pattern { pub provider: &'static str, pub regex: &'static str }` and `pub const PATTERNS: &[Pattern]` with at least 100 entries, one per provider or token shape, compiled once into a `RegexSet` plus individual `Regex`es behind a `OnceLock`. Include at minimum: AWS access key (`AKIA[0-9A-Z]{16}`), AWS secret in assignment, GitHub `gh[pousr]_[A-Za-z0-9]{36,}`, GitHub fine-grained `github_pat_`, GitLab `glpat-`, Slack `xox[abprs]-`, Slack webhook, Stripe `sk_live_|rk_live_`, Stripe webhook `whsec_`, Google API `AIza[0-9A-Za-z_-]{35}`, Google OAuth `ya29\.`, Firebase server key, OpenAI `sk-(proj-)?[A-Za-z0-9_-]{20,}`, Anthropic `sk-ant-`, Hugging Face `hf_`, Twilio `AC[0-9a-f]{32}` with `SK`, SendGrid `SG\.`, Mailgun `key-[0-9a-z]{32}`, Mailchimp `-us[0-9]{1,2}`, Postmark, Resend `re_`, npm `npm_`, PyPI `pypi-AgEIcHlwaS5vcmc`, RubyGems `rubygems_`, NuGet, Docker Hub `dckr_pat_`, Heroku UUID in `HEROKU_API_KEY`, DigitalOcean `dop_v1_|doo_v1_|dor_v1_`, Linode, Vultr, Hetzner, Cloudflare API token (40 chars in `CLOUDFLARE_API_TOKEN`), Cloudflare global key, Vercel, Netlify `nfp_`, Railway, Render `rnd_`, Fly.io `fo1_`, Supabase service role (JWT with role service_role, via `jwt.rs`), Supabase access token `sbp_`, PlanetScale `pscale_tkn_`, Neon, Upstash, Redis URL with password, MongoDB URI with password (`mongodb(\+srv)?://[^:]+:[^@]+@`), Postgres URI with password, MySQL URI, Discord bot token, Discord webhook, Telegram bot `[0-9]{8,10}:AA[0-9A-Za-z_-]{33}`, Twitter bearer `AAAAAAAAAAAAAAAAAAAAA`, Facebook `EAA[0-9A-Za-z]{20,}`, Instagram, LinkedIn, Shopify `shpat_|shpss_|shpca_`, Square `sq0atp-|sq0csp-`, PayPal live client secret in assignment, Braintree, Adyen, Plaid `access-production-`, Coinbase, Kraken, Binance, Algolia admin key in assignment, Mapbox `sk\.eyJ`, Sentry `sntrys_`, Datadog `dd[a-z]+_`, New Relic `NRAK-`, Grafana `glc_|glsa_`, PagerDuty, Opsgenie, Airtable `pat[0-9A-Za-z]{14}\.`, Notion `secret_[A-Za-z0-9]{43}`, Linear `lin_api_`, Asana, Atlassian, Jira, Bitbucket, CircleCI, Buildkite `bkua_`, Travis, Snyk, Postman `PMAK-`, Pulumi `pul-`, Doppler `dp\.pt\.`, HashiCorp `hvs\.`, 1Password `ops_`, Okta `00[A-Za-z0-9_-]{40}`, Auth0 client secret in assignment, Azure storage `AccountKey=`, Azure client secret, Azure DevOps PAT, GCP service account JSON (`"private_key_id"`), Alibaba `LTAI`, Tencent, Yandex, private key blocks (`-----BEGIN (RSA |EC |OPENSSH |DSA |PGP )?PRIVATE KEY-----`), JWT generic (`eyJ[A-Za-z0-9_-]{10,}\.eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}`; only flagged when `jwt.rs` says the payload names a privileged role or the token appears in an assignment whose name matches the credential regex), Basic auth header `Basic [A-Za-z0-9+/=]{20,}` with a decodable `user:pass`, generic `password\s*[:=]\s*["'][^"']{8,}["']` (entropy-gated), generic `api[_-]?key\s*[:=]\s*["'][A-Za-z0-9_\-]{20,}["']` (entropy-gated). The exact list is the implementer's to complete to 100; every entry needs one must-flag and one must-not-flag line in the fixture (a synthetic example that matches the shape, never a real credential; use the provider's documented example format).
- `entropy.rs`: `pub fn shannon(s: &str) -> f64`; `pub fn looks_random(s: &str) -> bool` (length >= 20, at least three character classes among lower, upper, digit, symbol, entropy >= 3.5 bits per character, not a URL, not a path, not all one repeated group).
- `jwt.rs`: `pub fn role(token: &str) -> Option<String>`: base64url-decode the payload (second segment) and read `"role"`. `service_role` is a secret; `anon`/`authenticated` are not.
- `mod.rs`: rule `secret-exposed` (File, Security, High, High, `locked`). Scan every line of `file.source` (comments included; a secret in a comment is still a secret). For each pattern hit: skip placeholders (`(?i)(example|sample|placeholder|changeme|your[_-]|xxx+|<[^>]+>|\$\{|process\.env)` inside the match or the line, or the matched value all one character); for the entropy-gated generics also require `looks_random(value)`; for the JWT generic require `role == service_role` or a credential-named assignment on that line. Evidence `{provider} credential: {mask}` where mask = first four characters + `...` + `({len} chars)`. Fix: `Revoke the credential now, move it to an environment variable or secret store, and purge it from git history`. Anchor `"{provider}\x1f{blake3(value)[..8]}"`. Set `owasp = Some("A02:2021")`, `cwe = Some("CWE-798")`. One finding per line per provider.
- Fixtures: `flag/keys.ts` (one line per pattern shape, synthetic), `clean/config.ts` (env references, anon JWT, placeholders, URLs with tokens in query that are public, test fixtures with obviously fake values `sk_test_...`), `edge/mixed.ts` (a service-role JWT flagged; an anon JWT not; a line with `locrin:allow`; a secret inside a block comment flagged).
- Rules tests assert: count per provider in flag, zero in clean, the edge set, that no evidence string contains more than the first four characters of any value, and that a config with `[rules.secret-exposed]\nenabled = false\nseverity = "low"` still yields High findings (locking).
- Commit `engine: secret-exposed rule with a hundred provider patterns, an entropy gate, and a Supabase role decoder`.

### Task 9: `weak-crypto`

**Files:** create `crates/rules/src/weak_crypto.rs`, test, fixtures `weak_crypto/{flag,clean,edge}/`.

Rule (File, Security, High): three forms, each a finding with `owasp = A02:2021`.
1. `createHash("md5" | "sha1")` (callee identifier `createHash` or member `crypto.createHash`/`<x>.createHash`, first argument a string literal, case-insensitive): confidence High when the credential regex `(?i)(password|passwd|pwd|credential|secret|token|session)` matches the enclosing top-level symbol name or any identifier in the enclosing statement, Medium otherwise. Evidence `` {alg} used to hash {context} ``. Fix `Use bcrypt, scrypt, or argon2 for credentials; SHA-256 or better for integrity`. `cwe = CWE-327`.
2. `Math.random()` inside a function or statement whose name (enclosing top-level symbol, or the variable/property being assigned) matches `(?i)(token|secret|nonce|session|otp|password|salt|iv|api[_-]?key)` or whose enclosing symbol name matches `(?i)id$|Id$|uuid|guid` with a length of 6 or more: confidence High. Not flagged elsewhere. Evidence `Math.random() generates {name}`. Fix `Use crypto.randomBytes, crypto.randomUUID, or crypto.getRandomValues`. `cwe = CWE-338`.
3. `createCipheriv(alg, key, iv)` or `createDecipheriv` where the third argument is a string literal, a `Buffer.from(<string literal>...)`, `Buffer.alloc(n)` (all-zero IV), or an array literal: confidence High. Evidence `static IV passed to {callee}`. Fix `Generate a fresh random IV per message with crypto.randomBytes and store it beside the ciphertext`. `cwe = CWE-327`.
Fixtures: flag (six lines, two per form), clean (sha256, bcrypt, Math.random for animation jitter and array shuffle, `crypto.randomBytes(16)` as IV, `randomUUID()`), edge (md5 in a function named `etagFor` with no credential context: Medium confidence finding; `Math.random()` assigned to `const sessionId` inside a function named `render`: High). Commit `engine: weak-crypto rule`.

### Task 10: `injection-sink`

**Files:** create `crates/rules/src/injection_sink.rs`, test, fixtures `injection_sink/{flag,clean,edge}/`.

Rule (File, Security, High, `owasp = A03:2021`):
- `eval(X)`, `new Function(..., X)`, `setTimeout(X, ...)`/`setInterval(X, ...)` with a string-typed first argument that is non-literal: X is anything other than `string`/`template_string` without substitutions. Confidence High, `cwe = CWE-95`. Evidence `{callee} receives {kind}` where kind is `a variable` / `a template with substitutions` / `a concatenation`. Fix `Do not build code from data; use a lookup table or JSON.parse`.
- Command: callee `exec`, `execSync`, `child_process.exec`, `cp.exec`, `shell.exec`, or `spawn`/`spawnSync`/`execFile` with `{ shell: true }` in the options: first argument is a `template_string` with substitutions or a `binary_expression` `+` with any non-literal operand: High, `cwe = CWE-78`. Bare identifier: Medium. Evidence `shell command built from {kind}`. Fix `Pass arguments as an array to execFile or spawn without a shell, and validate or allow-list every piece`.
- SQL: callee property in {`query`, `raw`, `execute`, `exec`, `$queryRawUnsafe`, `$executeRawUnsafe`, `unsafe`} or identifier `sql` called as a plain function (NOT a tagged template: a `call_expression` whose `arguments` field is a `template_string` node is a tagged template and is safe): first argument is a `template_string` with substitutions, or a concatenation with a non-literal: High, `cwe = CWE-89`; a bare identifier whose same-file declaration is a template with substitutions or a concatenation: High; a bare identifier otherwise: Medium. Also an `object` first argument with a `text:` pair built the same way (pg style). Evidence `SQL built from {kind} reaches {callee}`. Fix `Use parameter placeholders ($1, ?) or a tagged template that parameterises, and pass values separately`.
Fixtures: flag (eval of a variable, new Function from concat, exec with template, execSync with concat, `db.query(\`select * from t where id = ${id}\`)`, `knex.raw("..." + name)`, `prisma.$queryRawUnsafe(q)` where `q` is a template with substitutions declared above, `setTimeout(code, 10)` with a string variable), clean (eval of a literal, `sql\`...\`` tagged, `prisma.$queryRaw\`...\``, `db.query("select 1")`, `db.query("select * from t where id = $1", [id])`, `execFile("ls", [dir])`, `spawn("git", ["status"])`, `setTimeout(() => {}, 10)`), edge (`exec("ls " + "-la")` two literals: not flagged; `db.query(q)` with `q` a plain string variable: Medium; `locrin:allow`). Commit `engine: injection-sink rule`.

### Task 11: `html-injection`

**Files:** create `crates/rules/src/html_injection.rs`, test, fixtures `html_injection/{flag,clean,edge}/` (TSX).

Rule (File, Security, High, Medium, `owasp = A03:2021`, `cwe = CWE-79`):
- JSX attribute `dangerouslySetInnerHTML={{ __html: X }}` where X is not a string literal and not a call whose callee text matches `(?i)(sanitize|purify|escape|clean|dompurify|xss)`.
- Assignment to a member whose property is `innerHTML` or `outerHTML` with a right side that is not a string literal and not a sanitizer call; `insertAdjacentHTML(pos, X)`; `document.write(X)`; `$(...).html(X)` (jQuery) with the same X test.
Evidence `{sink} receives {kind}`; fix `Render text through the framework (textContent, JSX children) or sanitise with DOMPurify before injecting`. Fixtures: flag (four sinks), clean (literal html, `DOMPurify.sanitize(x)`, `sanitizeHtml(x)`, `textContent = x`), edge (an `__html` value that is a template literal with no substitutions: not flagged; `locrin:allow`). Commit `engine: html-injection rule`.

### Task 12: Lockfile parsers and the OSV client (core)

**Files:** create `crates/core/src/lockfile.rs`, `crates/core/src/osv.rs`; add `ureq` to core (workspace dep `ureq = { version = "2", features = ["json"] }`); fixtures `crates/core/tests/fixtures/lockfiles/{package-lock.json,yarn.lock,pnpm-lock.yaml}` (small, hand-written, with a known-vulnerable `lodash 4.17.15` and a clean `left-pad 1.3.0` in each).

Interfaces:
```rust
// lockfile
pub struct Package { pub name: String, pub version: String, pub line: u32 }
pub struct Lockfile { pub rel: String, pub hash: String, pub packages: Vec<Package> }   // dedup by (name, version), sorted
pub fn read(root: &Path) -> anyhow::Result<Option<Lockfile>>   // package-lock.json (lockfileVersion 2 or 3: `packages` map, keys `node_modules/<name>` possibly nested `node_modules/a/node_modules/b`; take the segment after the last `node_modules/`; skip the "" root and entries with `link: true`), else yarn.lock v1 (a block header line `"<name>@<range>", <name>@<range>:` followed by an indented `version "<v>"`; the name is the header's first entry up to the last `@` that is not at index 0), else pnpm-lock.yaml (under the `packages:` key, entries `  /<name>@<version>:` (v6) or `  <name>@<version>:` (v9), with optional `(peer)` suffixes to strip; scoped names keep their leading `@`). Line = the line of the entry.
// osv
pub struct Advisory { pub id: String, pub summary: String, pub severity: String /* CRITICAL | HIGH | MODERATE | LOW | UNKNOWN */, pub fixed: Option<String>, pub aliases: Vec<String> }
pub struct Hit { pub package: Package, pub advisory: Advisory }
pub struct Outcome { pub hits: Vec<Hit>, pub warnings: Vec<String>, pub snapshot_age_days: Option<u64> }
pub fn check(ix: &Index, lock: &Lockfile, offline: bool, fetch: &dyn Fn(&str, Option<&str>) -> anyhow::Result<String>) -> anyhow::Result<Outcome>
pub fn http_fetch(url: &str, body: Option<&str>) -> anyhow::Result<String>   // ureq, 10 s timeout, User-Agent "locrin"
```
`check` algorithm: batch = `POST https://api.osv.dev/v1/querybatch` with `{"queries":[{"package":{"name","ecosystem":"npm"},"version"}...]}` in chunks of 1000; the response `results[i].vulns[].id`. Cache the batch response in `osv_batch` keyed by `lock.hash` with `fetched_at`; if `offline` or the fetch fails, use the cached row (warning `using cached advisory snapshot from N days ago` when older than 1 day; warning `no cached advisory snapshot; vulnerable-dependency skipped` and empty hits when none). For each unique vuln id, `GET https://api.osv.dev/v1/vulns/{id}` cached in `osv_vulns` by id (details never change enough to matter; refresh when older than 30 days and online). Parse: `database_specific.severity` (uppercase) else `UNKNOWN`; `summary`; `aliases`; `fixed` = the first `fixed` event in the `affected[]` entry whose `package.name` matches and whose `ranges[].events` introduce a range containing the installed version (simplify: take the first `fixed` in any range of the matching package, or `None`). Tests: parsers against the three fixtures (identical package lists); `check` with a fake `fetch` closure returning canned JSON (no network in tests), covering online, offline-with-snapshot (age warning), offline-without-snapshot (skip warning), fetch error online (falls back to snapshot). Commit `engine: lockfile parsers and an OSV client with an on-disk snapshot`.

### Task 13: `vulnerable-dependency`

**Files:** create `crates/rules/src/vulnerable_dependency.rs`, test with a canned-fetch seam (the rule calls `osv::check` with `osv::http_fetch` unless `ctx.offline`; for tests, seed the index's `osv_batch`/`osv_vulns` tables through `osv::check` with a fake fetch first, then run the rule offline).

Rule (Graph scope, Security): `lockfile::read(ctx.root)?` else nothing; `osv::check(ctx.index()?, &lock, ctx.offline, &osv::http_fetch)`; print each warning to stderr once; one finding per hit: file = lock.rel, span = the package's line, anchor `"{name}\x1f{advisory id}"`, severity by the mapping in Global Constraints, confidence High when `fixed.is_some() && severity == High` else Medium, evidence `{name} {version}: {id} ({severity}) {summary}`, fix `Upgrade {name} to {fixed}` or `No fixed version published; review {id} and pin or replace the package`. `owasp = A06:2021`, `cwe = CWE-1395`. Fixtures: the three lockfiles under `crates/rules/tests/fixtures/vulnerable_dependency/{npm,yarn,pnpm}/`; each yields the same lodash finding. Commit `engine: vulnerable-dependency rule over OSV`.

### Task 14: Supabase rules

**Files:** create `crates/rules/src/supabase/{mod,service_role,rls}.rs`, tests, fixtures `supabase/{flag,clean,edge}/` (including `supabase/migrations/*.sql` files).

- `supabase-service-role-in-client` (File, Security, High, High, `A01:2021`, `CWE-284`): for each clean file whose rel does not match any `config.framework.server_paths` glob: flag every line containing the identifier `SERVICE_ROLE` (case-insensitive, `service_role` too) or a JWT literal whose `secrets::jwt::role` is `service_role`. Evidence `service-role key referenced in client code ({what})`; fix `Move this call behind a server (edge function, API route) and use the anon key on the client; the service role bypasses row-level security`.
- `supabase-table-without-rls` (Graph, Security, High, High, `A01:2021`, `CWE-284`): read every `supabase/migrations/*.sql` under `ctx.root` (sorted); across all of them, collect `create table [if not exists] [public.]<name>` (regex, case-insensitive, name unquoted or double-quoted) and `alter table [only] [public.]<name> enable row level security`; every created table with no enable statement anywhere is a finding at the create line in the migration that created it: file = the migration's rel, anchor `"table\x1f{name}"`, evidence `table {name} is created without row-level security`, fix `Add ALTER TABLE {name} ENABLE ROW LEVEL SECURITY and policies in the same migration`. Tables in schemas other than `public` are ignored.
Fixtures: flag (a client file `app/(tabs)/home.tsx` with `process.env.SUPABASE_SERVICE_ROLE_KEY`; migrations with two tables, one without RLS), clean (the same env reference in `supabase/functions/x/index.ts`; migrations enabling RLS in a later file; an `auth.` table), edge (a test file mentioning service role: exempt by the default server_paths; a config overriding `server_paths`). Commit `engine: supabase service-role and row-level-security rules`.

### Task 15: Express rules

**Files:** create `crates/rules/src/express/{mod,route_auth,cors,cookie}.rs`, tests, fixtures `express/{flag,clean,edge}/`.

All File scope, Security, High / High; they run only in files that import `express` (an import or require with specifier `express`), except `express-cookie-insecure`, which also runs where `res.cookie(` appears.
- `express-route-without-auth` (`A01:2021`, `CWE-306`): only when `config.framework.auth_middleware` is non-empty. A route registration is a call whose callee is `<app|router|server|api|any identifier>.<get|post|put|patch|delete|all|use>` with a string first argument. Authenticated when any later argument is an identifier in the list, a call whose callee identifier is in the list, or a member whose property is in the list; or when the file has an earlier `<obj>.use(<auth>)` / `<obj>.use('<prefix>', <auth>)` whose prefix is a prefix of the route path. Skip `use` registrations themselves and paths matching `(?i)(health|status|ping|login|signin|signup|register|logout|webhook|callback|oauth|public|docs)`. Evidence `{method} {path} registered without {list}`; fix `Add the auth middleware to the route or mount it with app.use before the router`.
- `express-cors-wildcard-on-authenticated` (`A05:2021`, `CWE-942`): a `cors()` call with no arguments or with `origin: "*"` / `origin: true` that appears as a route argument alongside an auth middleware (same call), or as `app.use(cors(...))` in a file that registers at least one authenticated route. Evidence `wildcard CORS on an authenticated route`; fix `Set origin to the allowed origins list and credentials: true only for them`.
- `express-cookie-insecure` (`A05:2021`, `CWE-614` / `CWE-1004`): `res.cookie(name, value[, options])` where options is absent or an object literal lacking `httpOnly: true` (CWE-1004) or `secure: true` (CWE-614); one finding per missing flag; `res.clearCookie` ignored. Evidence `cookie {name} set without {flag}`; fix `Pass { httpOnly: true, secure: true, sameSite: "lax" }`.
Fixtures with a `locrin.toml` built in the test (`framework.auth_middleware = ["requireAuth"]`). Commit `engine: express route-auth, cors, and cookie rules`.

### Task 16: Part B precision check, benchmarks, pull request

Extend `docs/superpowers/plans/2026-09-09-error-test-and-security-precision.md` with the Part B rules (same method; Express rules are unmeasured on this corpus and say so; `vulnerable-dependency` is validated by hand-checking five advisories against OSV's website; `secret-exposed` misses STOP for the founder). Measure `vulnerable-dependency` warm cost (cached) on FastLift and record it (must be under 50 ms). Four benchmarks three times. Registry test lists all twenty-one ids. `cargo test --workspace`. Commit, push `engine/security`, open the PR with the precision report as the body.
