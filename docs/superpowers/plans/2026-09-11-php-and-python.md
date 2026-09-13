# PHP and Python Behind the Flag Implementation Plan (phase two, plan B)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Locrin parses PHP and Python when a repository turns them on in `locrin.toml`, runs the rules whose logic is language-independent on those files (leftovers, secrets, vulnerable dependencies via Composer and PyPI lockfiles) at the same 85 percent precision bar, and keeps every TypeScript-only rule silent on them.

**Architecture:** One runtime bump (tree-sitter 0.23 to 0.24, proven safe on 2026-09-11) and two grammar crates. `Language` gains `Php` and `Python`; the walker and the explicit-path resolver consult a new `[languages]` config table so nothing changes for existing users. Symbols for both languages feed anchors and `find_existing`. The `Rule` trait gains `languages()`, defaulting to the JS family, and the runner filters files per rule, so graph and framework rules never see the new languages. Three leftover rules gain per-language vocabularies; `secret-exposed` is text-based and needs only the language list. Lockfile parsing gains `composer.lock`, `poetry.lock` and pinned `requirements.txt`, and the OSV client carries the ecosystem per package.

**Tech Stack:** Rust workspace (version becomes 0.4.0), tree-sitter 0.24, tree-sitter-typescript 0.23 (unchanged), tree-sitter-php 0.23 (`LANGUAGE_PHP`, the mixed HTML grammar), tree-sitter-python 0.23 (`LANGUAGE`), existing OSV client and lockfile module.

**Spec:** `docs/superpowers/specs/2026-09-05-agent-native-quality-gate-design.md` sections 1 (Languages v2: "PHP, Python behind a config flag, same pipeline"), 4.1 and 4.3 (the 85 percent precision gate per rule), 9, 11 phase two.

## Global Constraints

- Default behaviour is byte-identical for repositories without a `[languages]` table: `.php` and `.py` files are skipped by the walker and refused as explicit paths (with a one-line hint) unless enabled.
- Rules that run on PHP or Python in this plan, and only these: `leftover-debug`, `leftover-commented-code`, `leftover-agent-marker`, `secret-exposed`, `vulnerable-dependency`. Every other rule declares the JS family and never receives a PHP or Python file. `already-exists` stays release two.
- Precision gate (spec 4.3): each new rule-language pair is hand-checked on 20 sampled findings from the corpora in Task 8; under 17 of 20 the pair ships disabled for that language (recorded in the plan and README), never removed.
- Corpora, read-only: Python is the FastSpot checkout (a private Python service, 96 files); PHP is a clone of `https://github.com/BookStackApp/BookStack` (MIT) at its latest release tag into the scratchpad directory, never into the repository. No corpus file is ever written to.
- Performance: the five existing benchmarks (TypeScript, FastLift) must not regress; a PHP cold index number on BookStack is recorded, not gated.
- Ids: no existing finding id changes (the anchor rules are untouched), so FastLift's baseline stays valid.
- Git: branch `engine/languages` off `main`. One commit per task, plain messages, no attribution trailers, never `git add -A`, no em dashes. `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, zero warnings, `cargo test --workspace` green before every commit. Toolchain in Git Bash: `export PATH="$USERPROFILE/.cargo/bin:$PATH"`.
- CI (`.github/workflows/ci.yml`) runs on the branch's pull request; it must stay green.

## File structure

```
Cargo.toml                                   tree-sitter 0.24, tree-sitter-php 0.23, tree-sitter-python 0.23, version 0.4.0
crates/core/Cargo.toml                       the two grammar deps
crates/core/src/lang.rs                      Php, Python variants; grammar(); as_str(); JS_FAMILY, ALL consts
crates/core/src/config.rs                    [languages] table: Languages { php, python }
crates/core/src/walk.rs                      WalkOptions.languages; gate uses Language::enabled
crates/cli/src/run.rs                        explicit-path gate with hint
crates/core/src/symbols.rs                   PHP and Python top-level symbols
crates/rules/src/lib.rs                      Rule::languages(); per-rule file filter in run_rules and run_file_rules
crates/rules/src/leftover_debug.rs           PHP and Python sinks
crates/rules/src/leftover_commented.rs       per-language code starts and strong signals
crates/rules/src/leftover_marker.rs          declares ALL
crates/rules/src/secrets/mod.rs              declares ALL
crates/core/src/lockfile.rs                  composer.lock, poetry.lock, requirements.txt; Package.ecosystem; all present lockfiles
crates/core/src/osv.rs                       ecosystem per package in the batch query and the match
crates/rules/src/vulnerable_dependency.rs    declares ALL (it reads lockfiles, not sources)
crates/rules/tests/fixtures/<rule>/{php,py}/ fixtures per language
crates/cli/tests/fixtures/multilang/         a repo with .ts, .php, .py and three lockfiles
docs/superpowers/plans/2026-09-11-php-and-python-precision.md   Task 8 report
README.md                                    Languages section; rule table gains a languages column
```

Shared additions, defined once:

```rust
// core::lang
pub enum Language { TypeScript, Tsx, JavaScript, Php, Python }
pub const JS_FAMILY: &[Language] = &[Language::TypeScript, Language::Tsx, Language::JavaScript];
pub const ALL: &[Language] = &[Language::TypeScript, Language::Tsx, Language::JavaScript, Language::Php, Language::Python];
impl Language { pub fn enabled(&self, langs: &Languages) -> bool }

// core::config
#[derive(Default)] pub struct Languages { pub php: bool, pub python: bool }   // Config.languages

// rules
trait Rule { fn languages(&self) -> &'static [Language] { locrin_core::lang::JS_FAMILY } ... }
```

---

### Task 1: Runtime bump, grammar crates, language variants

**Files:**
- Modify: `Cargo.toml` (workspace deps and version 0.4.0), `crates/core/Cargo.toml`, `crates/core/src/lang.rs`, `crates/core/src/parse.rs` (tests only)

**Interfaces:**
- Produces: `Language::Php`, `Language::Python`; `from_path` maps `.php` and `.phtml` to Php, `.py` and `.pyi` to Python; `grammar()` returns `tree_sitter_php::LANGUAGE_PHP.into()` and `tree_sitter_python::LANGUAGE.into()`; `as_str()` gives `"php"` and `"python"`; `JS_FAMILY` and `ALL` constants.

- [ ] **Step 1: Write the failing tests**

Append to the tests in `crates/core/src/lang.rs`:
```rust
    #[test]
    fn detects_php_and_python() {
        assert_eq!(Language::from_path(Path::new("app/Http/Kernel.php")), Some(Language::Php));
        assert_eq!(Language::from_path(Path::new("views/x.phtml")), Some(Language::Php));
        assert_eq!(Language::from_path(Path::new("bot/main.py")), Some(Language::Python));
        assert_eq!(Language::from_path(Path::new("bot/types.pyi")), Some(Language::Python));
        assert_eq!(Language::Php.as_str(), "php");
        assert_eq!(Language::Python.as_str(), "python");
        assert_eq!(JS_FAMILY.len(), 3);
        assert_eq!(ALL.len(), 5);
    }
```

Append to the tests in `crates/core/src/parse.rs`:
```rust
    #[test]
    fn parses_php_with_html_prefix_and_python() {
        let php = "<html><?php\nfunction add(int $a, int $b): int { return $a + $b; }\n?></html>\n".to_string();
        let p = parse_source(Path::new("x/add.php"), "x/add.php", php).unwrap();
        assert_eq!(p.language, Language::Php);
        assert!(!p.has_error);
        let py = "def add(a: int, b: int) -> int:\n    return a + b\n".to_string();
        let p = parse_source(Path::new("x/add.py"), "x/add.py", py).unwrap();
        assert_eq!(p.language, Language::Python);
        assert!(!p.has_error);
        assert_eq!(p.tree.root_node().kind(), "module");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd . && export PATH="$USERPROFILE/.cargo/bin:$PATH" && cargo test -p locrin-core -q 2>&1 | tail -5
```
Expected: compile errors (`Php` and `JS_FAMILY` unknown).

- [ ] **Step 3: Implement**

`Cargo.toml` workspace dependencies: `tree-sitter = "0.24"`, add `tree-sitter-php = "0.23"`, `tree-sitter-python = "0.23"`; `[workspace.package] version = "0.4.0"`. `crates/core/Cargo.toml`: add `tree-sitter-php.workspace = true` and `tree-sitter-python.workspace = true`.

`crates/core/src/lang.rs`: add the two variants, extend `from_path` with `"php" | "phtml" => Some(Language::Php)` and `"py" | "pyi" => Some(Language::Python)`, extend `grammar()` with `Language::Php => tree_sitter_php::LANGUAGE_PHP.into()` and `Language::Python => tree_sitter_python::LANGUAGE.into()`, extend `as_str()`, and add:
```rust
pub const JS_FAMILY: &[Language] = &[Language::TypeScript, Language::Tsx, Language::JavaScript];
pub const ALL: &[Language] = &[Language::TypeScript, Language::Tsx, Language::JavaScript, Language::Php, Language::Python];
```
Run `cargo update -p tree-sitter` so the lock moves to 0.24.x. Every `match` on `Language` elsewhere in the workspace that is exhaustive will now fail to compile; extend each with the two variants doing the JS-neutral thing (for example `imports.rs` and `testcases.rs` return no imports and no test cases for Php and Python). Record every site touched.

- [ ] **Step 4: Run the full workspace**

```bash
cd . && export PATH="$USERPROFILE/.cargo/bin:$PATH" && cargo test --workspace -q 2>&1 | grep -E "test result|error" | head -12 && cargo clippy --workspace --all-targets -q -- -D warnings && cargo fmt --all --check && echo CLEAN
```
Expected: every crate ok (477 plus 2 new), `CLEAN`. If `LANGUAGE_PHP` is not the export name in the installed tree-sitter-php, run `cargo doc -p tree-sitter-php --no-deps` and use the documented constant; record it.

- [ ] **Step 5: Commit**

```bash
cd . && git add Cargo.toml Cargo.lock crates/core/Cargo.toml crates/core/src/lang.rs crates/core/src/parse.rs $(git diff --name-only) && git commit -m "engine: tree-sitter 0.24, PHP and Python grammars, language variants; version 0.4.0"
```
(The `$(git diff --name-only)` picks up the exhaustive-match sites you had to touch; list them in the report.)

---

### Task 2: The `[languages]` flag in config, walker and explicit paths

**Files:**
- Modify: `crates/core/src/config.rs`, `crates/core/src/walk.rs`, `crates/cli/src/run.rs`, `crates/cli/tests/cli.rs`
- Create: `crates/cli/tests/fixtures/multilang/` (`locrin.toml` absent; `src/a.ts`, `src/b.php`, `src/c.py`, `composer.lock`, `poetry.lock`, `requirements.txt`, `package.json`)

**Interfaces:**
- Produces: `config::Languages { pub php: bool, pub python: bool }` with `Default` all false, `Config.languages: Languages` (serde default, `deny_unknown_fields`); `Language::enabled(&self, langs: &Languages) -> bool` (JS family always true); `walk::WalkOptions.languages: Languages`; the walker skips files whose language is not enabled; `run.rs` explicit paths: a disabled-language file is skipped with one stderr line `note: src/b.php skipped; enable it with [languages] php = true in locrin.toml`.

Config shape:
```toml
[languages]
php = true
python = true
```

- [ ] **Step 1: Write the fixture and the failing tests**

Fixture files: `src/a.ts` with a `console.log("x");` inside an exported function; `src/b.php` with `<?php\nfunction f() {\n    var_dump($x);\n}\n`; `src/c.py` with `def f():\n    breakpoint()\n`; the three lockfiles are written in Task 7 (create them empty-but-valid now: `composer.lock` = `{"packages": [], "packages-dev": []}`, `poetry.lock` = `# empty\n`, `requirements.txt` = empty); `package.json` = `{ "name": "multilang" }`.

Append to `crates/cli/tests/cli.rs` (reuse the existing `copy_fixture`-style helper, parameterised on the fixture name):
```rust
#[test]
fn php_and_python_are_skipped_until_enabled() {
    let dir = copy_named_fixture("multilang");
    let out = locrin(dir.path()).args(["check", "--json", "--offline"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let files: Vec<&str> = v["findings"].as_array().unwrap().iter().map(|f| f["file"].as_str().unwrap()).collect();
    assert!(files.iter().all(|f| f.ends_with(".ts")), "{files:?}");

    let out = locrin(dir.path()).args(["check", "--offline", "src/b.php"]).output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8(out.stderr).unwrap().contains("[languages] php = true"));

    std::fs::write(dir.path().join("locrin.toml"), "[languages]\nphp = true\npython = true\n").unwrap();
    let out = locrin(dir.path()).args(["check", "--json", "--offline"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let files: Vec<&str> = v["findings"].as_array().unwrap().iter().map(|f| f["file"].as_str().unwrap()).collect();
    assert!(files.iter().any(|f| f.ends_with("b.php")), "{files:?}");
    assert!(files.iter().any(|f| f.ends_with("c.py")), "{files:?}");
}
```
(This test passes fully only after Task 5 makes `leftover-debug` fire on `var_dump` and `breakpoint`; until then assert on the first two blocks and mark the third `#[ignore]` with a note, un-ignoring it in Task 5.)

Unit tests in `config.rs`: `[languages]` absent gives both false; `php = true` alone gives python false; an unknown key under `[languages]` errors naming `locrin.toml`.

- [ ] **Step 2: Run to verify failure, implement, run to verify pass**

Implement `Languages`, `Config.languages`, `Language::enabled`, the walker gate (`if !lang.enabled(&opts.languages) { continue; }` at the same point `from_path` is consulted, for both walk paths), and the explicit-path hint in `run.rs` (where `Language::from_path(&canon).is_some()` is checked). Thread `config.languages` into `WalkOptions` at every construction site. Run the crate tests, then the full workspace with clippy and fmt.

- [ ] **Step 3: Commit**

```bash
cd . && git add crates/core/src/config.rs crates/core/src/walk.rs crates/cli/src/run.rs crates/cli/tests/cli.rs crates/cli/tests/fixtures/multilang && git commit -m "engine: [languages] flag gates PHP and Python in the walker and explicit paths"
```

---

### Task 3: Symbols for PHP and Python

**Files:**
- Modify: `crates/core/src/symbols.rs`

**Interfaces:**
- Produces: `extract` handles Php (`function_definition` name field, kind `function`; `class_declaration` name, kind `class`; `method_declaration` inside a class body, kind `method`, name `Class::method`; `interface_declaration`, `trait_declaration`, `enum_declaration` kinds `interface`, `trait`, `enum`; top-level declarations are exported) and Python (`function_definition` and `class_definition` at module level, kinds `function`, `class`; `expression_statement > assignment` with an `identifier` left side at module level, kind `const`; exported when the name does not start with `_`). Spans and `enclosing_symbol` behave as for TypeScript. PHP declarations sit under `program > php_tag ... ` or directly under `program`; walk the root's children and descend one level into `php` text nodes as the grammar exposes them (print the tree once in a test to learn the exact shape and encode it).

- [ ] **Step 1: Write the failing tests**

```rust
    #[test]
    fn extracts_php_symbols() {
        let src = "<?php\nnamespace App;\nfunction helper() {}\nclass Repo {\n    public function find(int $id) {}\n}\ninterface Shape {}\n";
        let p = parse_source(Path::new("src/a.php"), "src/a.php", src.to_string()).unwrap();
        let view: Vec<(String, String, bool)> = extract(&p).iter().map(|s| (s.kind.clone(), s.name.clone(), s.exported)).collect();
        assert_eq!(view, vec![
            ("function".into(), "helper".into(), true),
            ("class".into(), "Repo".into(), true),
            ("method".into(), "Repo::find".into(), true),
            ("interface".into(), "Shape".into(), true),
        ]);
        assert_eq!(enclosing_symbol(&p, 5).as_deref(), Some("Repo::find"));
    }

    #[test]
    fn extracts_python_symbols() {
        let src = "import os\nLIMIT = 3\n_private = 1\ndef run():\n    return LIMIT\nclass Bot:\n    def tick(self):\n        pass\n";
        let p = parse_source(Path::new("bot/a.py"), "bot/a.py", src.to_string()).unwrap();
        let view: Vec<(String, String, bool)> = extract(&p).iter().map(|s| (s.kind.clone(), s.name.clone(), s.exported)).collect();
        assert_eq!(view, vec![
            ("const".into(), "LIMIT".into(), true),
            ("const".into(), "_private".into(), false),
            ("function".into(), "run".into(), true),
            ("class".into(), "Bot".into(), true),
        ]);
        assert_eq!(enclosing_symbol(&p, 7).as_deref(), Some("Bot"));
    }
```

- [ ] **Step 2: Run to verify failure, implement per language in `collect` (dispatch on `file.language`), run to verify pass, run the workspace, commit**

```bash
cd . && git add crates/core/src/symbols.rs && git commit -m "engine: PHP and Python top-level symbols"
```

---

### Task 4: `Rule::languages()` and the per-rule file filter

**Files:**
- Modify: `crates/rules/src/lib.rs`, plus each rule module that declares `ALL` (leftover_commented, leftover_marker, secrets/mod.rs, vulnerable_dependency; leftover_debug in Task 5)

**Interfaces:**
- Produces: `trait Rule { fn languages(&self) -> &'static [Language] { JS_FAMILY } }`; `run_rules` and `run_file_rules` hand each rule only the files whose language it declares (build the per-rule slice once per run; keep `clean_files` semantics); `Rule::languages` surfaced in `RuleMeta` for SARIF (`properties.languages`) and in `locrin mcp status` rule listing if one exists.

- [ ] **Step 1: Write the failing tests**

In `crates/rules/src/lib.rs` tests: a test rule with default `languages()` given one `.ts` and one `.py` file yields findings from the `.ts` file only; a test rule declaring `ALL` yields both. In `crates/rules/tests/`: with `[languages] php = true`, a PHP file containing an unused `use` statement yields no `unused-import` finding (the rule is JS-only), and a PHP file with a `TODO` comment yields a `leftover-agent-marker` finding.

- [ ] **Step 2: Implement, run, commit**

```bash
cd . && git add crates/rules/src && git commit -m "rules: per-rule language declaration and file filter"
```

---

### Task 5: `leftover-debug` for PHP and Python

**Files:**
- Modify: `crates/rules/src/leftover_debug.rs`; create fixtures `crates/rules/tests/fixtures/leftover_debug/{php,py}/{flag,clean}/`; extend `crates/rules/tests/leftover_debug.rs`; un-ignore the Task 2 CLI test block.

**Rule definition additions:**
- PHP: a `function_call_expression` whose function name is one of `var_dump`, `print_r`, `var_export`, `dd`, `dump`, `debug_zval_dump`; plus the `xdebug_break()` call. Not flagged: `error_log`, `echo`, `printf` (legitimate output), anything inside `debug_allowed` globs.
- Python: a call to `breakpoint`, `pdb.set_trace`, `ipdb.set_trace`, `pudb.set_trace`; an `import pdb` / `import ipdb` / `from pdb import` statement. `print(` is NOT flagged (ruled: too common in scripts; precision would fail).
- Same severity and confidence (High, High); same `locrin:allow` handling.

- [ ] **Step 1: Write fixtures and failing tests** (flag: each sink once on its own line, expected line numbers asserted; clean: `error_log`, `echo`, `print(...)`, `logging.debug(...)`).

- [ ] **Step 2: Implement per language (dispatch on `file.language`; the TypeScript path is untouched), run, commit**

```bash
cd . && git add crates/rules/src/leftover_debug.rs crates/rules/tests && git commit -m "rules: leftover-debug sinks for PHP and Python"
```

---

### Task 6: `leftover-commented-code` vocabularies for PHP and Python

**Files:**
- Modify: `crates/rules/src/leftover_commented.rs`; fixtures `leftover_commented/{php,py}/{flag,clean}/`; tests.

**Rule definition additions:** the run detection is unchanged; `looks_like_code` and `is_strong_code` take the language:
- PHP strong: ends with `;`, `{`, `}`; starts with `$`, `function `, `return `, `if (`, `foreach (`, `echo `, `use `, `namespace `, `public `, `private `. Supporting: `,`, `)`. Comment forms: `//`, `#`, `/* */` (PHP `comment` nodes cover all three; a `#` line comment is stripped like `//`).
- Python strong: starts with `def `, `class `, `import `, `from `, `return `, `if `, `for `, `while `, `with `, `try:`, `except`, `self.`, `print(`; or ends with `:`. Supporting: `,`, `)`. Comments are `#` only; strip `#`.
- Prose runs (sentences ending in full stops) must not flag in either language; a docstring is not a comment node and is never scanned.

- [ ] **Step 1: Fixtures and failing tests** (flag: three commented-out statements in each language; clean: three-line prose comment, a license header, a Python docstring block).

- [ ] **Step 2: Implement, run, commit**

```bash
cd . && git add crates/rules/src/leftover_commented.rs crates/rules/tests && git commit -m "rules: commented-code vocabularies for PHP and Python"
```

---

### Task 7: Composer, Poetry and requirements lockfiles

**Files:**
- Modify: `crates/core/src/lockfile.rs`, `crates/core/src/osv.rs`, `crates/rules/src/vulnerable_dependency.rs`; fixtures under `crates/core/tests/fixtures/lockfiles/` and the multilang CLI fixture's three lockfiles.

**Interfaces:**
- Produces: `Package.ecosystem: &'static str` (`"npm"`, `"Packagist"`, `"PyPI"`); `lockfile::read(root)` returns every present lockfile (a `Vec<Lockfile>`; call sites updated), each with its own `rel` and key; parsers: `composer.lock` (`packages` and `packages-dev` arrays: `name`, `version` with a leading `v` stripped), `poetry.lock` (`[[package]]` tables: `name`, `version`), `requirements.txt` (lines `name==version` only; other specifiers, `-r` includes, comments and blank lines skipped; extras `name[extra]==v` take the bare name). The OSV batch query sends each package's ecosystem; the advisory match compares ecosystem per package; the snapshot key includes the ecosystem so an npm snapshot never answers for PyPI. `vulnerable-dependency` declares `ALL` and reports findings on whichever lockfile the package came from.

- [ ] **Step 1: Failing tests** for each parser on small fixtures (including `v1.2.3` stripping, `requirements.txt` with `>=` lines skipped, extras), for `read` returning two lockfiles when both `package-lock.json` and `composer.lock` exist, and for the OSV match refusing a same-named package in another ecosystem.

- [ ] **Step 2: Implement, run, commit**

```bash
cd . && git add crates/core/src/lockfile.rs crates/core/src/osv.rs crates/rules/src/vulnerable_dependency.rs crates/core/tests/fixtures/lockfiles crates/cli/tests/fixtures/multilang && git commit -m "engine: Composer, Poetry and requirements lockfiles with per-package ecosystems"
```

---

### Task 8: Precision gate on the corpora

**Files:**
- Create: `docs/superpowers/plans/2026-09-11-php-and-python-precision.md`
- Modify: rule `enabled_by_default` per language if a pair fails (add `fn enabled_for(&self, lang: Language) -> bool` with a default of `true`, consulted by the runner, and set it false for a failed pair); `README.md` rule table.

- [ ] **Step 1: Corpora**

Python: the FastSpot checkout (read-only; use `LOCRIN_CACHE_DIR` in the scratchpad; write a temporary `locrin.toml` enabling python into a COPY of the repo in the scratchpad, never into the original). PHP: `git clone --depth 1 --branch <latest release tag> https://github.com/BookStackApp/BookStack` into the scratchpad; write `locrin.toml` with `php = true` there. Run `locrin check --json --offline` (cap lifted: use `--sarif-file` to get every finding) on each.

- [ ] **Step 2: Sample and label**

For each pair (`leftover-debug`, `leftover-commented-code`, `leftover-agent-marker`, `secret-exposed` on php and py; `vulnerable-dependency` on Packagist and PyPI, using a network-enabled run for the advisory fetch), take 20 findings at random (seeded, seed 11) or all if fewer, read the source, and label each true or false with a one-line reason using the same standard as the TypeScript gate reports (`docs/superpowers/plans/2026-09-09-error-test-and-security-precision.md`). Record per pair: sample size, true count, verdict (ship on, or ship off for that language), and the three most instructive false positives with the fix idea.

- [ ] **Step 3: Act on the verdicts**

A pair under 17 of 20 ships off for that language via `enabled_for`, with the reason in the precision report and the README table. A pair with fewer than 5 findings in the corpus is recorded as "unmeasured, ships on" (the TypeScript plans used the same rule).

- [ ] **Step 4: Commit**

```bash
cd . && git add docs/superpowers/plans/2026-09-11-php-and-python-precision.md crates/rules/src README.md && git commit -m "rules: precision gate results for PHP and Python; per-language defaults"
```

---

### Task 9: Benchmarks, README, SARIF metadata

**Files:**
- Modify: `README.md` (new "Languages" section: the flag, which rules run, what stays TypeScript-only, the corpora and numbers), `crates/reporters/src/sarif.rs` (`properties.languages` on each rule), `crates/cli/tests/bench.rs` (a non-gated `php_cold_index_recorded` ignored test that prints the BookStack cold index time when `LOCRIN_PHP_BENCH_REPO` is set).

- [ ] **Step 1: Run the five existing benchmarks in release mode and record them; run the PHP one with `LOCRIN_PHP_BENCH_REPO` pointed at the scratchpad clone.**

- [ ] **Step 2: Docs and SARIF, commit**

```bash
cd . && git add README.md crates/reporters/src/sarif.rs crates/cli/tests/bench.rs && git commit -m "docs: languages section; SARIF rule languages; PHP cold-index benchmark"
```

Post-merge, founder's actions: tag `v0.4.0` (the release pipeline from plan A publishes it); the examples' pins move to `v0.4.0` in a follow-up docs commit.

---

## Self-review

**Spec coverage.** Section 1 "PHP, Python behind a config flag, same pipeline": Tasks 1 to 3 (grammars, flag, symbols in the same index). Section 4.3 precision gate: Task 8 with the 17-of-20 rule and per-language defaults. Section 9: parse errors in the new languages degrade the file the same way (`has_error` from the same `parse_source`). Section 3.4: existing benchmarks re-run in Task 9; the PHP number is recorded, not gated, because the spec's targets name a TypeScript repository. Everything TypeScript-only stays silent on the new languages by the `languages()` filter (Task 4), which is the guarantee that makes "behind the flag" honest.

**Placeholder scan.** Tasks 1 to 3 carry code and tests; Tasks 4 to 7 carry exact rule vocabularies, interface names and test intents rather than full code, because each is a per-language dispatch inside an existing, reviewed module and the executor reads that module first; Task 8 carries the exact corpora, seed, sample size, bar and consequence. No TBD or TODO.

**Type consistency.** `Languages` defined in Task 2 and consumed by `Language::enabled`, `WalkOptions`, `run.rs`; `JS_FAMILY` and `ALL` from Task 1 used by Task 4's trait default and the declaring rules; `Package.ecosystem` from Task 7 used by `osv.rs` and the rule; `enabled_for` from Task 8 consulted by the runner from Task 4.

**Known judgment calls.** Python `print(` is deliberately not a debug sink. PHP uses the mixed HTML grammar so templates parse; pure-PHP repositories are unaffected. Graph rules for PHP and Python (imports, dead exports) are a later plan, not this one.
