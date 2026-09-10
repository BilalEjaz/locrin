# Dogfood Cleanup Implementation Plan (version one, follow-up to plan 4)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the four engine defects the first real `locrin init` on FastLift surfaced (2026-09-10, main `95c1deb`): finding ids that collide, two upstream grammar limits that silently exempt whole files from every rule, and an `init` that parses the repository twice.

**Architecture:** Four small, independent changes, each with a test that reproduces the FastLift symptom on a fixture. Ids gain the information that distinguished the colliding findings (the package version for advisories; the line's own text and its ordinal within the enclosing symbol for line rules), which changes every id and therefore every baseline, stated up front. The parser feeds tree-sitter a NUL-free copy of the source while every reader keeps the bytes as written. A file whose only parse errors are the JSX-text scanner refusing a bare `&` is treated as parsed, because the rest of its tree is intact. `init` writes its baseline from a recording pass so the scan it just ran is not repeated cold.

**Tech Stack:** Rust 2021, tree-sitter 0.23 (grammar pinned at tree-sitter-typescript 0.23.2, the newest on crates.io; both grammar bugs are open upstream: tree-sitter-javascript #366, tree-sitter-typescript #322), blake3, rusqlite (all present). No new crates.

**Spec:** `docs/superpowers/specs/2026-09-05-agent-native-quality-gate-design.md`, sections 7.1 (`id` is "stable across line shifts"; a hash of rule id, symbol id or normalised span, and file path), 9 (parse failure: file marked unparsed, one warning, no findings from that file), 3.5 and 7.6 (baseline entries keyed by id). Evidence: the dogfood record in `docs/superpowers/plans/2026-09-10-init-hooks-and-mcp-measurement.md` and the FastLift tracker entries D9 to D11 and C13 in `<home>/fasting-app/.planning/ROADMAP-SMART-2026-08-25.md`. Read `crates/core/src/finding.rs` (`make_id`), `crates/rules/src/lib.rs` (`anchor_for`, `finding`, `finding_at`), `crates/rules/src/vulnerable_dependency.rs` (the anchor at line 134), `crates/core/src/parse.rs`, `crates/cli/src/run.rs` (`baseline_create`, `full_findings`) and `crates/cli/src/init.rs` before Task 1.

**What the dogfood showed, exactly.**
- `init` reported 778 findings and wrote 756 baseline entries: 13 ids shared by 33 `vulnerable-dependency` findings (three installed versions of `@xmldom/xmldom` share one advisory, and the anchor is `name\x1fadvisory`, no version) and one id shared by 2 `leftover-commented-code` findings (two identical commented lines inside one function; the line-rule anchor is the enclosing symbol's name). `Baseline::accept` dedupes by id, so accepting one instance silently accepts the others, and fixing one version leaves the entry suppressing the rest.
- Five FastLift files are excluded from every rule with `warning: parse errors`. None is a syntax error (`tsc` is clean). `src/domain/food/collapseFoodDuplicates.ts:43` and `src/domain/sync/quarantineNotice.ts:75,86` hold a literal NUL byte (0x00) inside a template literal as a composite-key separator; tree-sitter's lexer reserves byte 0 as end of input. `app/settings.tsx:1546,1992` and `src/features/today/YourNumbersPanel.tsx:23` hold `BODY & NUTRITION` as JSX text; the external scanner's `html_character_reference` stops at any `&` that does not start a valid entity (tree-sitter-javascript #366, open). `src/theme/historyPort.guard.test.tsx:393` has `as import('../domain/signals/types').SleepStageSegment[]`; an import-type member cannot take an array suffix in the grammar (tree-sitter-typescript #322, open). The first four are fixable engine-side; the fifth is not, and is documented.
- `init` printed every parse warning twice and took 13.9 s: the scan (recording) parsed 1846 files, then `baseline_create` ran a non-recording pass on an in-memory index and parsed all 1846 again with no findings cache.

## Global Constraints

- Ids stay `blake3(rule + "\x1f" + rel + "\x1f" + anchor)[..16]` via `make_id`; only what goes into the anchor changes. Every existing baseline is invalidated by Task 1 and must be recreated with `locrin baseline create`; the README says so, the measurement doc says so, and the FastLift tracker entry D9 gets a line (the FastLift baseline is uncommitted, so nothing shipped breaks).
- Spec 7.1's "stable across line shifts" holds after Task 1: an anchor never contains a line number. Two findings in one file get distinct ids unless they are byte-identical lines inside the same symbol AND the same ordinal, which is impossible.
- `ParsedFile.source` is always the file as written; only the bytes handed to the parser are sanitised (Task 2). Evidence lines, `line_text`, `anchor_for` and `symbols::extract` read the original.
- A file is "parsed" when the tree has no ERROR or MISSING node outside the tolerated class (Task 3). The tolerated class is exactly: an ERROR node whose parent is `jsx_element` or `jsx_fragment`, whose text contains neither `<` nor `{` nor `}`, and which sits between `jsx_text` siblings or at the edge of the element's children. Anything else keeps the file excluded, with the existing single warning.
- `baseline_create(root, offline, record)`: `init` passes `record = true` (it has just scanned); the CLI's `baseline create` keeps `false` (plan 2's watermark design, same as `baseline accept`). Doc comments state the difference.
- Performance targets unchanged (cold under 5 s, warm single-file under 300 ms, hook under 300 ms, warm 30-file under 1 s, startup under 50 ms); Task 5 re-runs the ignored set once and records the power state beside the numbers.
- Git: branch `engine/cleanup` off `main`. One commit per task, `cargo fmt --all` before every commit, plain `engine: ...` / `docs: ...` messages, no attribution trailers, never `git add -A`. No em dashes anywhere. TDD with RED and GREEN evidence per task.
- `export PATH="$HOME/.cargo/bin:$PATH"` before any cargo command. Corpus checkouts are never written into; FastLift's five files are read-only evidence and the fixtures reproduce their constructs.

## Deviations recorded during execution

(Empty at planning time. The executor appends every ruling here with its reason.)

- **Task 1, Task 5 bumps the workspace version to 0.2.0.** Ids changed, and the
  findings cache key mixes in `CARGO_PKG_VERSION`, so every warm row an older
  binary wrote must become a miss rather than a hit serving a stale id. The cost
  if the ruling is wrong is one cold pass per repository after the upgrade. No
  test pins the crate version: the `0.1.0` strings in `crates/reporters` and
  `crates/mcp` are arguments those tests pass in themselves, not reads of
  `CARGO_PKG_VERSION`, so the bump changed no assertion.

- **Task 2, the NUL fixture bucket is named `nul_byte` and the `line_text` half
  of the unit assertion lives in the rules e2e.** `NUL` is a reserved device name
  on Windows and a directory by that name fails with os error 1, and a unit test
  in `crates/core` cannot reach the rules crate without a cycle.
  `.gitattributes` pins the fixture as binary so no tool rewrites the byte.

- **Task 3, the tolerated shape follows the tree the parser actually produces.**
  The plan described an ERROR node between `jsx_text` siblings; probing showed an
  ERROR child of `jsx_element` whose named children are identifiers. The
  implementation tolerates that shape with "no child of the ERROR has an error of
  its own" in place of the plan's `jsx_text` clause, keeping the `<`, `{` and `}`
  text guard as the discriminator. The cost if the ruling is wrong is that a
  text-run refusal which is not an ampersand is tolerated too, which is the same
  class of error and the same recovery.

- **Task 5, the pull request is opened by the controller after the whole-branch
  review, not by this task.** The brief's step 4 `gh pr create` was held back
  deliberately. What Task 5 delivers is the version bump, the documentation, the
  benchmarks and the read-only FastLift evidence.

- **Task 5, the cold benchmark is reported as failed and attributed rather than
  re-run until it passes.** The four mandated runs read 46561, 10032, 10033 and
  9857 ms against a 5000 ms target. Instead of tuning or moving the target, three
  earlier commits were built and measured beside the branch head, which put every
  binary including `main` at 5.4 to 6.3 s and the four mandated runs' plateau
  down to the machine's state at the time. The gate is missed either way and it
  is missed by `main` too, so it is recorded as a concern and handed to the
  controller.

- **Task 5, the FastLift tracker entry D9 line was written by the controller, not
  by this task.** The Global Constraint above asks for a line in
  `<home>/fasting-app/.planning/ROADMAP-SMART-2026-08-25.md`; the
  controller's read-only amendment forbade this task writing anything into that
  checkout beyond moving the baseline out and back, so the controller wrote the
  tracker line itself. The constraint is met, by the controller's hand rather
  than this task's.

## File structure

- `crates/rules/src/lib.rs` (modify): `anchor_for` includes the line's trimmed text and its ordinal within the enclosing symbol.
- `crates/rules/src/vulnerable_dependency.rs` (modify): anchor includes the installed version.
- `crates/core/src/parse.rs` (modify): NUL sanitisation for the parser; tolerated JSX-text errors.
- `crates/core/src/tree.rs` (modify, if a walker helper is needed): `error_nodes(root) -> Vec<Node>`.
- `crates/cli/src/run.rs` (modify): `baseline_create(root, offline, record)`.
- `crates/cli/src/init.rs` (modify): passes `record = true`.
- `crates/cli/tests/init.rs`, `crates/rules/tests/`, `crates/core/src/parse.rs` tests (modify/create).
- `README.md`, `docs/superpowers/plans/2026-09-10-init-hooks-and-mcp-measurement.md` (modify): id change, re-baseline, the fifth file's documented limit, benchmark re-run.

---

### Task 1: Finding ids that do not collide

**Files:**
- Modify: `crates/rules/src/lib.rs` (`anchor_for`), `crates/rules/src/vulnerable_dependency.rs` (line 134)
- Test: `crates/rules/src/lib.rs` (unit), `crates/rules/tests/vulnerable_dependency.rs`, one line-rule fixture test (`crates/rules/tests/` for `leftover-debug`)

**Interfaces:**
```rust
// crates/rules/src/lib.rs
/// The part of a finding's identity that survives a line shift: the enclosing
/// symbol (or the line's own text when there is none), the line's trimmed text,
/// and the ordinal of this line among the identical lines inside that symbol.
/// Two identical console.log lines in one function therefore get two ids, and
/// moving the function down the file keeps both.
pub fn anchor_for(file: &ParsedFile, line: u32) -> String;
// Format: format!("{symbol}\x1f{text}\x1f{ordinal}") where symbol = enclosing_symbol(file, line)
// .unwrap_or_default(), text = line_text(file, line).trim(), ordinal = number of lines before
// `line` whose enclosing symbol is the same Option<String> and whose trimmed text equals `text`.
```
`vulnerable_dependency.rs`: `let anchor = format!("{}\x1f{}\x1f{}", package.name, package.version, advisory.id);` with the comment updated to say why the version is there (three installed versions of one package are three findings with three fixes).

- [ ] Step 1: failing tests. Unit in `lib.rs`: `two_identical_lines_in_one_function_get_two_ids` (a source with `function f() {\n  console.log("a");\n  console.log("a");\n}`; `anchor_for(file, 2) != anchor_for(file, 3)`); `an_anchor_survives_a_line_shift` (the same function with two blank lines inserted above it: both anchors equal the originals); `identical_lines_in_different_functions_differ`. In `tests/vulnerable_dependency.rs`: a lockfile fixture with two versions of one package both matching one advisory (the OSV snapshot fixture already used by that test file; add a second version entry) produces two findings with distinct ids. Line-rule e2e: the `leftover-debug` fixture gains a file with two identical `console.log` lines in one function and the test asserts two distinct ids.
- [ ] Step 2: run, see them fail (equal ids).
- [ ] Step 3: implement. `anchor_for` computes the ordinal with one pass over lines `1..line` using `enclosing_symbol` and `line_text` (both exist). Keep `finding` and `finding_at` unchanged.
- [ ] Step 4: `cargo test --workspace` green (expect fixture id assertions elsewhere to need recounting: any test that pinned a literal id value must be updated to the new value with the reason in the commit); fmt; commit `engine: finding ids carry the line and its ordinal, and the advisory id carries the version`.

### Task 2: A NUL byte in source does not exclude the file

**Files:**
- Modify: `crates/core/src/parse.rs`
- Test: `crates/core/src/parse.rs` (unit), `crates/rules/tests/` one e2e over a fixture file holding a NUL in a template literal

**Interfaces:**
```rust
// crates/core/src/parse.rs
/// tree-sitter's lexer treats byte 0 as end of input, so a source holding a NUL
/// (a composite-key separator in a template literal is the case that found this)
/// would parse as if it ended there. The parser is handed a copy with every NUL
/// replaced by 0x01, one byte for one byte so every span still indexes the
/// original; every reader keeps the file as written.
fn parser_bytes(source: &str) -> Cow<'_, [u8]>;
```
`parse_source` calls `parser.parse(parser_bytes(&source).as_ref(), None)`; `ParsedFile.source` stays the original. (0x01 is a control character no grammar rule matches specially and it is valid inside a string or template; if the grammar rejects it inside a template literal in the test, use 0x7F instead and record the deviation.)

- [ ] Step 1: failing tests. Unit: `a_nul_inside_a_template_literal_parses` (source `` const k = `${a}\0${b}`; `` builds a `ParsedFile` with `has_error == false`, and `line_text(file, 1)` still contains the NUL); e2e: a fixture `.ts` with the same construct plus a `console.log` after it produces the `leftover-debug` finding (proving rules ran) and no parse warning.
- [ ] Step 2: run, see them fail (`has_error == true`, no finding).
- [ ] Step 3: implement.
- [ ] Step 4: green; fmt; commit `engine: a NUL byte in source no longer ends the parse`.

### Task 3: A bare ampersand in JSX text does not exclude the file

**Files:**
- Modify: `crates/core/src/parse.rs` (`has_error` computation), `crates/core/src/tree.rs` (a walker if none fits)
- Test: `crates/core/src/parse.rs` (unit), `crates/rules/tests/` one e2e over a `.tsx` fixture

**Interfaces:**
```rust
// crates/core/src/parse.rs
/// Whether the tree has a parse error the engine cannot see past. The JSX-text
/// scanner refuses a bare `&` (tree-sitter-javascript #366), leaving an ERROR
/// node among the element's text children with the rest of the tree intact;
/// that one shape is tolerated so `BODY & NUTRITION` does not exempt a
/// 2000-line screen from every rule. Everything else is an error.
fn has_blocking_error(root: Node, src: &str) -> bool;
fn is_tolerated_jsx_text_error(node: Node, src: &str) -> bool;
// tolerated: node.is_error() (kind "ERROR"), parent kind in {"jsx_element", "jsx_fragment"},
// node text contains none of '<', '{', '}', and every named child of the node (if any) is jsx_text or absent.
// MISSING nodes are never tolerated.
```
`ParsedFile.has_error = has_blocking_error(tree.root_node(), &source)`.

- [ ] Step 1: failing tests. Unit: `a_bare_ampersand_in_jsx_text_is_tolerated` (`<Label>BODY & NUTRITION</Label>` inside a component: `has_error == false`); `a_broken_attribute_is_still_an_error` (`<Label a=>x</Label>`: `has_error == true`); `an_import_type_with_an_array_suffix_is_still_an_error` (the FastLift `as import('./t').Seg[]` shape: `has_error == true`, pinning that the fifth file stays excluded rather than silently misparsed). E2e: a `.tsx` fixture with the ampersand and a `console.log` produces the `leftover-debug` finding and no warning.
- [ ] Step 2: run, see them fail.
- [ ] Step 3: implement; walk with a cursor over every node (`tree.rs` may already have a `walk` helper; reuse it).
- [ ] Step 4: green; fmt; commit `engine: a bare ampersand in JSX text is a tolerated parse error`.

### Task 4: `init` writes its baseline from the scan it just ran

**Files:**
- Modify: `crates/cli/src/run.rs` (`baseline_create`), `crates/cli/src/init.rs`, `crates/cli/src/main.rs` (the CLI call passes `false`)
- Test: `crates/cli/tests/init.rs`

**Interfaces:**
```rust
// crates/cli/src/run.rs
/// `record` decides whether this pass may leave its mark on the repository's
/// index. `init` passes true: it has just scanned, the findings cache is warm,
/// and a second cold parse of the whole repository would double its time and
/// repeat every warning. The CLI's `baseline create` passes false for the same
/// reason `baseline accept` does: a baseline command must never move the
/// `--changed` watermark.
pub fn baseline_create(root: &Path, offline: bool, record: bool) -> anyhow::Result<usize>;
```

- [ ] Step 1: failing test in `tests/init.rs`: `init_parses_the_repository_once` (a fixture file with a deliberate parse error, `<Label a=>x</Label>`, so the warning is observable; run `init`; assert stderr contains `warning: parse errors in` exactly once). A second assertion: stderr's `indexed N file(s) in` line appears once and no second `indexing` line follows.
- [ ] Step 2: run, see it fail (two warnings).
- [ ] Step 3: implement; `main.rs`'s `BaselineCmd::Create` passes `false`.
- [ ] Step 4: green; fmt; commit `engine: init writes the baseline from its own scan`.

### Task 5: Docs, re-baseline note, benchmarks, pull request

**Files:** `README.md`, `docs/superpowers/plans/2026-09-10-init-hooks-and-mcp-measurement.md` (append `## Dogfood cleanup` section), this plan's deviations.

- [ ] Step 1: README: under the baseline section, one paragraph: ids changed in this release, recreate baselines with `locrin baseline create`; under limits, the import-type-with-array-suffix grammar limit with the workaround (`Array<import('..').T>` or a named type import) and that such a file is excluded with one warning.
- [ ] Step 2: measurement doc: the dogfood findings above, the fix per item, and the ignored benchmark set re-run once (four runs, first discarded, power state recorded via `(Get-CimInstance Win32_Battery).BatteryStatus`), with a one-line ruling per gate; Task 2 and 3 add a byte scan per parse and a tree walk per file, so the cold number is the one to watch.
- [ ] Step 3: on the FastLift checkout, read-only: run `locrin check --offline` with a fresh `LOCRIN_CACHE_DIR` against the temporarily moved-aside baseline (move `locrin-baseline.json` to the scratch dir and back, never delete it) and record: total findings, distinct ids (must equal total), and that the four fixed files now produce findings or at least no warning; the fifth still warns. Put the numbers in the doc.
- [ ] Step 4: commit `docs: the dogfood cleanup, measured on FastLift`; `gh pr create --base main` from `engine/cleanup`.

## Self-review

- Spec 7.1: ids remain a hash of rule, path and a normalised anchor; stability across line shifts is pinned by a test (Task 1). Spec 9: parse failure still warns once and excludes (Task 3 narrows "failure" to what the engine cannot see past; the third unit test pins that a real error stays an error). Spec 7.6: baselines keyed by id, so the id change is announced (Task 5).
- Type consistency: `anchor_for(file, line)` keeps its signature; `baseline_create` gains one parameter and both callers are named; `parser_bytes`, `has_blocking_error`, `is_tolerated_jsx_text_error` are introduced once each.
- No placeholders: every test names its input and its assertion; the only conditional is the substitute byte in Task 2, with the fallback and the deviation rule stated.
