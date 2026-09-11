# CI Action and Distribution Implementation Plan (phase two, plan A)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Locrin runs as a GitHub Action on pull requests and as a deployment gate, with a release pipeline that publishes signed-by-checksum binaries for Linux, macOS and Windows, a Markdown summary comment that updates in place, SARIF upload into code scanning, and a check status branch protection can require.

**Architecture:** Two engine additions (a Markdown reporter and two CLI flags: `--markdown`, `--sarif-file`) so one `locrin check` run feeds the comment, the SARIF upload and the exit code. Everything else is repository plumbing: a CI workflow for the engine itself, a tag-triggered release workflow with a four-target build matrix, a composite action under `action/` that downloads a release asset by checksum (or uses a locally built binary for the action's own smoke test), and example workflows for the pull-request view and the deployment gate.

**Tech Stack:** Rust (existing workspace, tree-sitter 0.23, clap 4), GitHub Actions composite action (bash steps, `gh` CLI, `github/codeql-action/upload-sarif@v3`, `actions/checkout@v4`, `dtolnay/rust-toolchain@stable`), release assets as `.tar.gz` (Linux, macOS) and `.zip` (Windows) plus `SHA256SUMS`.

**Spec:** `docs/superpowers/specs/2026-09-05-agent-native-quality-gate-design.md` sections 8.1 (GitHub Action), 8.2 (deployment gate), 3 (distribution: npm package and Homebrew are NOT in this plan), 7.2 to 7.4 (reporters and exit codes), 11 phase two.

## Global Constraints

- Product Locrin, binary `locrin`, workspace version becomes `0.3.0` in this plan (Task 4); every crate uses `version.workspace = true`.
- The engine never sends source anywhere. The action sends only what `locrin` prints (Markdown, SARIF) to GitHub through the runner's own token.
- Exit codes are the contract: 0 pass or advisory, 1 block, 2 engine error (spec 7.4). The action fails its step on 1 (unless `fail-on-block: false`) and always on 2.
- The Markdown comment carries the hidden marker `<!-- locrin-report -->` on its first line so the action can find and edit its own comment; never more than one Locrin comment per pull request.
- The Markdown comment is capped at ten findings ordered as the verdict orders them (blocking first), with a "and N more" line; the SARIF file carries every finding.
- `--sarif-file PATH` writes SARIF 2.1.0 to PATH and leaves stdout to whichever format was chosen; `--markdown` conflicts with `--json` and `--sarif` on stdout.
- Release assets are named `locrin-<tag>-<target>.<ext>` where tag is `vX.Y.Z`, target one of `x86_64-unknown-linux-gnu`, `aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-pc-windows-msvc`, ext `tar.gz` or `zip`; a single `SHA256SUMS` file lists all four. The action refuses an asset whose checksum does not match.
- The action never runs a network-dependent rule in `offline: true` mode; the default is `offline: false` because a CI runner has network and the advisory snapshot is what makes `vulnerable-dependency` useful.
- All GitHub Actions used are pinned to a major tag (`@v4`, `@v3`, `@stable`); no unpinned `@main` references.
- Git: branch `engine/ci` off `main`. One commit per task, plain messages, no attribution trailers, never `git add -A`, no em dashes anywhere. YAML workflows are verified by pushing the branch and watching the run with `gh run watch`; there is no local Actions runner.
- The founder's repos are never written to by this plan. The FastLift workflow file is produced under `action/examples/` for the founder to copy.
- Toolchain: prefix cargo commands in Git Bash with `export PATH="$USERPROFILE/.cargo/bin:$PATH"`. `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings` and zero warnings before every commit (the CI workflow from Task 3 enforces exactly this).

## File structure

```
crates/reporters/src/markdown.rs        Markdown summary renderer (new)
crates/reporters/src/lib.rs             pub mod markdown
crates/cli/src/main.rs                  --markdown, --sarif-file flags on Check
crates/cli/tests/cli.rs                 flag tests
.github/workflows/ci.yml                fmt, clippy, tests on ubuntu and windows; action smoke job
.github/workflows/release.yml           tag-triggered four-target release with SHA256SUMS; dry-run dispatch
action/action.yml                       composite action
action/README.md                        inputs, outputs, permissions, examples
action/examples/pull-request.yml        PR view with comment and SARIF
action/examples/deploy-gate.yml         check --since last deploy tag
action/examples/fastlift.yml            the founder's FastLift workflow, to copy into fasting-app
README.md                               Install section gains release downloads; new "GitHub Action" and "Deployment gate" sections
Cargo.toml                              version 0.3.0
```

---

### Task 1: Markdown summary reporter

**Files:**
- Create: `crates/reporters/src/markdown.rs`
- Modify: `crates/reporters/src/lib.rs`

**Interfaces:**
- Consumes: `locrin_core::finding::{Verdict, Finding, Status, Severity, Confidence}`.
- Produces: `markdown::MARKER: &str = "<!-- locrin-report -->"`, `markdown::CAP: usize = 10`, `markdown::render(v: &Verdict, version: &str, run_url: Option<&str>) -> String`.

Rendered shape (exact):

```
<!-- locrin-report -->
### Locrin: BLOCK

2 finding(s): 1 high, 0 medium, 1 low. 1 blocking. 15 ms. locrin 0.3.0

| | File | Rule | Evidence | Fix |
|---|---|---|---|---|
| block | `src/dirty.ts:2` | `leftover-debug` | `console.log("debug");` | Remove the debug statement or route it through the project logger |
| advise | `src/dirty.ts:3` | `leftover-agent-marker` | `// TODO clean this up` | Link the marker to an issue or resolve it now |

Details: <run_url>
```

Rules: status word upper case (`PASS`, `ADVISORY`, `BLOCK`); the first column is `block` when confidence is High, else `advise`; evidence and file cells are wrapped in backticks with any backtick inside the evidence replaced by a straight quote; pipe characters inside cells replaced by `\|`; when there are no findings, the table is omitted and the line reads `No findings.`; when `truncated > 0` after capping, a line `... and {n} more in the SARIF upload.` follows the table; the `Details:` line is present only when `run_url` is Some.

- [ ] **Step 1: Write the failing tests**

Append to `crates/reporters/src/markdown.rs` (tests at the bottom):
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use locrin_core::finding::*;

    fn f(rule: &str, sev: Severity, conf: Confidence, file: &str, line: u32, evidence: &str) -> Finding {
        Finding {
            id: format!("{:016x}", line),
            rule: rule.into(),
            category: Category::Erosion,
            severity: sev,
            confidence: conf,
            file: file.into(),
            span: Span { start_line: line, start_col: 0, end_line: line, end_col: 1 },
            evidence: evidence.into(),
            fix: "Fix it".into(),
            related: vec![],
            owasp: None,
            cwe: None,
        }
    }

    #[test]
    fn starts_with_the_marker_and_status() {
        let v = Verdict::from_findings(vec![], 3);
        let s = render(&v, "0.3.0", None);
        assert!(s.starts_with("<!-- locrin-report -->\n### Locrin: PASS\n"));
        assert!(s.contains("No findings."));
        assert!(!s.contains("Details:"));
    }

    #[test]
    fn table_marks_blocking_rows_and_escapes_cells() {
        let v = Verdict::from_findings(
            vec![
                f("leftover-debug", Severity::High, Confidence::High, "src/a.ts", 2, "console.log(`x|y`)"),
                f("leftover-agent-marker", Severity::Low, Confidence::Medium, "src/a.ts", 3, "// TODO x"),
            ],
            15,
        );
        let s = render(&v, "0.3.0", Some("https://example/run/1"));
        assert!(s.contains("### Locrin: BLOCK"));
        assert!(s.contains("2 finding(s): 1 high, 0 medium, 1 low. 1 blocking. 15 ms. locrin 0.3.0"));
        assert!(s.contains("| block | `src/a.ts:2` | `leftover-debug` | `console.log('x\\|y')` | Fix it |"));
        assert!(s.contains("| advise | `src/a.ts:3` | `leftover-agent-marker` |"));
        assert!(s.ends_with("Details: https://example/run/1\n"));
    }

    #[test]
    fn caps_at_ten_and_points_to_sarif() {
        let many: Vec<Finding> = (0..13).map(|i| f("r", Severity::Low, Confidence::Medium, "src/a.ts", i, "e")).collect();
        let v = Verdict::from_findings(many, 1);
        let s = render(&v, "0.3.0", None);
        assert_eq!(s.matches("| advise |").count(), 10);
        assert!(s.contains("... and 3 more in the SARIF upload."));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd <repo> && export PATH="$USERPROFILE/.cargo/bin:$PATH" && cargo test -p locrin-reporters -q 2>&1 | tail -5
```
Expected: compile error, module `markdown` not found.

- [ ] **Step 3: Implement**

Prepend to `crates/reporters/src/markdown.rs`:
```rust
//! Markdown summary for pull-request comments: one marker line the action
//! finds again, the verdict, and at most ten findings. The SARIF file is the
//! complete list; this is the glanceable one.

use locrin_core::finding::{Confidence, Status, Verdict};

pub const MARKER: &str = "<!-- locrin-report -->";
pub const CAP: usize = 10;

fn status_word(s: Status) -> &'static str {
    match s {
        Status::Pass => "PASS",
        Status::Advisory => "ADVISORY",
        Status::Block => "BLOCK",
    }
}

fn cell(text: &str) -> String {
    text.replace('`', "'").replace('|', "\\|")
}

pub fn render(v: &Verdict, version: &str, run_url: Option<&str>) -> String {
    let capped = v.clone().capped(CAP);
    let total = v.high + v.medium + v.low;
    let mut out = format!("{MARKER}\n### Locrin: {}\n\n", status_word(v.status));
    out.push_str(&format!(
        "{total} finding(s): {} high, {} medium, {} low. {} blocking. {} ms. locrin {version}\n\n",
        v.high, v.medium, v.low, v.blocking, v.duration_ms
    ));
    if capped.findings.is_empty() {
        out.push_str("No findings.\n");
    } else {
        out.push_str("| | File | Rule | Evidence | Fix |\n|---|---|---|---|---|\n");
        for f in &capped.findings {
            let kind = if f.confidence == Confidence::High { "block" } else { "advise" };
            out.push_str(&format!(
                "| {kind} | `{}:{}` | `{}` | `{}` | {} |\n",
                cell(&f.file),
                f.span.start_line,
                cell(&f.rule),
                cell(&f.evidence),
                cell(&f.fix)
            ));
        }
        if capped.truncated > 0 {
            out.push_str(&format!("\n... and {} more in the SARIF upload.\n", capped.truncated));
        }
    }
    if let Some(url) = run_url {
        out.push_str(&format!("\nDetails: {url}\n"));
    }
    out
}
```

Add `pub mod markdown;` to `crates/reporters/src/lib.rs`.

- [ ] **Step 4: Run tests to verify they pass**

```bash
cd <repo> && export PATH="$USERPROFILE/.cargo/bin:$PATH" && cargo test -p locrin-reporters -q 2>&1 | tail -3
```
Expected: all reporter tests pass (three new). If the backtick-in-evidence assertion fails on the exact string, print the rendered row and align the test to the `cell` rules above, never the other way round.

- [ ] **Step 5: Commit**

```bash
cd <repo> && git add crates/reporters/src/markdown.rs crates/reporters/src/lib.rs && git commit -m "reporters: markdown summary for pull-request comments"
```

---

### Task 2: `--markdown` and `--sarif-file` on `locrin check`

**Files:**
- Modify: `crates/cli/src/main.rs`
- Modify: `crates/cli/tests/cli.rs`

**Interfaces:**
- Consumes: `locrin_reporters::{markdown, sarif, agent, terminal}`, existing `run::check`, `run::Options`.
- Produces: `locrin check --markdown` (stdout Markdown; conflicts with `--json` and `--sarif`), `locrin check --sarif-file PATH` (writes SARIF to PATH, any stdout format; PATH's parent must exist; failure to write is an engine error, exit 2), env `LOCRIN_RUN_URL` read for the Markdown `Details:` line (the action sets it to the workflow run URL).

- [ ] **Step 1: Write the failing tests**

Append to `crates/cli/tests/cli.rs`:
```rust
#[test]
fn markdown_output_carries_the_marker_and_blocks() {
    let dir = copy_fixture();
    let out = locrin(dir.path()).args(["check", "--markdown", "--offline"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.starts_with("<!-- locrin-report -->\n### Locrin: BLOCK"), "{text}");
    assert!(text.contains("`leftover-debug`"));
}

#[test]
fn markdown_conflicts_with_json_and_sarif() {
    let dir = copy_fixture();
    for other in ["--json", "--sarif"] {
        let out = locrin(dir.path()).args(["check", "--markdown", other]).output().unwrap();
        assert_eq!(out.status.code(), Some(2), "{other}");
    }
}

#[test]
fn sarif_file_is_written_alongside_any_stdout_format() {
    let dir = copy_fixture();
    let sarif = dir.path().join("out").join("locrin.sarif");
    std::fs::create_dir_all(sarif.parent().unwrap()).unwrap();
    let out = locrin(dir.path())
        .args(["check", "--markdown", "--offline", "--sarif-file"])
        .arg(&sarif)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.starts_with("<!-- locrin-report -->"));
    let doc: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&sarif).unwrap()).unwrap();
    assert_eq!(doc["version"], "2.1.0");
    assert!(doc["runs"][0]["results"].as_array().unwrap().len() >= 2);
}

#[test]
fn sarif_file_in_a_missing_directory_is_an_engine_error() {
    let dir = copy_fixture();
    let out = locrin(dir.path())
        .args(["check", "--offline", "--sarif-file"])
        .arg(dir.path().join("nope").join("x.sarif"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8(out.stderr).unwrap().contains("x.sarif"));
}

#[test]
fn markdown_details_line_comes_from_locrin_run_url() {
    let dir = copy_fixture();
    let out = locrin(dir.path())
        .args(["check", "--markdown", "--offline"])
        .env("LOCRIN_RUN_URL", "https://example/run/9")
        .output()
        .unwrap();
    assert!(String::from_utf8(out.stdout).unwrap().ends_with("Details: https://example/run/9\n"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd <repo> && export PATH="$USERPROFILE/.cargo/bin:$PATH" && cargo test -p locrin-cli --test cli markdown -q 2>&1 | tail -5
```
Expected: the `--markdown` runs exit 2 with an unexpected-argument error, so the first test fails on the exit code.

- [ ] **Step 3: Implement**

In `crates/cli/src/main.rs`, extend the `Check` variant (keep existing fields and doc comments):
```rust
        /// Markdown summary for a pull-request comment (marker line, verdict, top ten findings)
        #[arg(long, conflicts_with_all = ["json", "sarif"])]
        markdown: bool,
        /// Also write SARIF 2.1.0 with every finding to PATH, whatever stdout shows
        #[arg(long, value_name = "PATH")]
        sarif_file: Option<PathBuf>,
```

Replace the `Cmd::Check` arm body with:
```rust
        Cmd::Check { paths, changed, json, sarif, markdown, sarif_file, base, since, offline } => {
            let diff = base.map(git::DiffScope::Base).or(since.map(git::DiffScope::Since));
            let opts = run::Options { root, paths, changed_only: changed, json, offline, diff };
            let verdict = run::check(&opts)?;
            let rules = || -> Vec<locrin_reporters::sarif::RuleMeta> {
                locrin_rules::all_rules()
                    .iter()
                    .map(|r| locrin_reporters::sarif::RuleMeta {
                        id: r.id().to_string(),
                        description: r.description().to_string(),
                        severity: r.default_severity(),
                        category: r.category(),
                        enabled_by_default: r.enabled_by_default(),
                    })
                    .collect()
            };
            if let Some(path) = &sarif_file {
                let doc = locrin_reporters::sarif::render(&verdict, &rules(), env!("CARGO_PKG_VERSION"));
                std::fs::write(path, doc).with_context(|| format!("writing SARIF to {}", path.display()))?;
            }
            if sarif {
                println!("{}", locrin_reporters::sarif::render(&verdict, &rules(), env!("CARGO_PKG_VERSION")));
            } else if markdown {
                let run_url = std::env::var("LOCRIN_RUN_URL").ok().filter(|s| !s.trim().is_empty());
                print!("{}", locrin_reporters::markdown::render(&verdict, env!("CARGO_PKG_VERSION"), run_url.as_deref()));
            } else if opts.json {
                println!("{}", locrin_reporters::agent::render(&verdict));
            } else {
                print!("{}", locrin_reporters::terminal::render(&verdict));
            }
            Ok(verdict.exit_code())
        }
```
Add `use anyhow::Context;` at the top of `main.rs` if it is not already imported. Keep every other arm unchanged.

- [ ] **Step 4: Run tests to verify they pass**

```bash
cd <repo> && export PATH="$USERPROFILE/.cargo/bin:$PATH" && cargo test -p locrin-cli --test cli -q 2>&1 | tail -3 && cargo clippy --workspace --all-targets -q -- -D warnings && cargo fmt --all --check && echo CLEAN
```
Expected: all cli tests pass (five new) and `CLEAN`.

- [ ] **Step 5: Commit**

```bash
cd <repo> && git add crates/cli/src/main.rs crates/cli/tests/cli.rs && git commit -m "cli: --markdown summary and --sarif-file alongside any stdout format"
```

---

### Task 3: CI workflow for the engine

**Files:**
- Create: `.github/workflows/ci.yml`

**Interfaces:**
- Produces: a required check named `ci` on every pull request and push to `main`: fmt, clippy with `-D warnings`, tests on `ubuntu-latest` and `windows-latest`, and (after Task 5) the action smoke job. The benchmarks stay `#[ignore]` and are not run in CI.

- [ ] **Step 1: Write the workflow**

`.github/workflows/ci.yml`:
```yaml
name: ci

on:
  pull_request:
  push:
    branches: [main]

permissions:
  contents: read

jobs:
  lint:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - uses: Swatinem/rust-cache@v2
      - run: cargo fmt --all --check
      - run: cargo clippy --workspace --all-targets -- -D warnings

  test:
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, windows-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo test --workspace
        env:
          LOCRIN_HOOK_BUDGET_MS: "10000"
```

The hook budget override exists so the post-edit watchdog test does not flake on a loaded runner (Locrin PR #14).

- [ ] **Step 2: Push the branch and watch the run**

```bash
cd <repo> && git add .github/workflows/ci.yml && git commit -m "ci: fmt, clippy and tests on ubuntu and windows" && git push -u origin engine/ci && sleep 20 && gh run list --branch engine/ci --limit 3
```
Then `gh run watch <id> --exit-status` for the newest run. Expected: `lint` and both `test` jobs green. If clippy fails on something the local run did not show, fix the code in a follow-up commit on this branch (do not relax `-D warnings`).

---

### Task 4: Release workflow and version 0.3.0

**Files:**
- Create: `.github/workflows/release.yml`
- Modify: `Cargo.toml` (`version = "0.3.0"`), `Cargo.lock` (cargo updates it on the next build)

**Interfaces:**
- Produces: on a pushed tag `v*`, four release assets plus `SHA256SUMS` attached to a GitHub Release named after the tag; a `workflow_dispatch` with `dry-run: true` that builds and uploads workflow artifacts without creating a release; a guard step that fails when the tag does not equal `v` + the workspace version.

- [ ] **Step 1: Bump the version**

In `Cargo.toml` `[workspace.package]` set `version = "0.3.0"`. Run `cargo build -p locrin-cli -q` once so `Cargo.lock` follows.

- [ ] **Step 2: Write the workflow**

`.github/workflows/release.yml`:
```yaml
name: release

on:
  push:
    tags: ["v*"]
  workflow_dispatch:
    inputs:
      dry-run:
        description: "Build and upload workflow artifacts without creating a release"
        type: boolean
        default: true

permissions:
  contents: write

jobs:
  build:
    strategy:
      fail-fast: false
      matrix:
        include:
          - target: x86_64-unknown-linux-gnu
            os: ubuntu-22.04
            ext: tar.gz
          - target: aarch64-apple-darwin
            os: macos-14
            ext: tar.gz
          - target: x86_64-apple-darwin
            os: macos-13
            ext: tar.gz
          - target: x86_64-pc-windows-msvc
            os: windows-2022
            ext: zip
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}
      - uses: Swatinem/rust-cache@v2
      - name: Resolve tag
        id: tag
        shell: bash
        run: |
          version=$(grep -m1 '^version' Cargo.toml | sed 's/.*"\(.*\)"/\1/')
          if [[ "${GITHUB_REF}" == refs/tags/* ]]; then
            tag="${GITHUB_REF#refs/tags/}"
            if [[ "$tag" != "v$version" ]]; then
              echo "tag $tag does not match Cargo.toml version $version" >&2
              exit 1
            fi
          else
            tag="v$version-dryrun"
          fi
          echo "tag=$tag" >> "$GITHUB_OUTPUT"
      - run: cargo build --release -p locrin-cli --target ${{ matrix.target }}
      - name: Package
        id: pkg
        shell: bash
        run: |
          tag='${{ steps.tag.outputs.tag }}'
          name="locrin-${tag}-${{ matrix.target }}"
          mkdir -p dist/"$name"
          if [[ "${{ matrix.ext }}" == "zip" ]]; then
            cp target/${{ matrix.target }}/release/locrin.exe dist/"$name"/
            (cd dist && 7z a -tzip "$name.zip" "$name" >/dev/null)
            asset="dist/$name.zip"
          else
            cp target/${{ matrix.target }}/release/locrin dist/"$name"/
            tar -C dist -czf "dist/$name.tar.gz" "$name"
            asset="dist/$name.tar.gz"
          fi
          echo "asset=$asset" >> "$GITHUB_OUTPUT"
      - uses: actions/upload-artifact@v4
        with:
          name: ${{ matrix.target }}
          path: ${{ steps.pkg.outputs.asset }}
          if-no-files-found: error

  publish:
    needs: build
    runs-on: ubuntu-latest
    if: ${{ github.event_name == 'push' || inputs.dry-run == false }}
    steps:
      - uses: actions/download-artifact@v4
        with:
          path: assets
          merge-multiple: true
      - name: Checksums
        run: |
          cd assets && sha256sum locrin-* > SHA256SUMS && cat SHA256SUMS
      - name: Create release
        env:
          GH_TOKEN: ${{ github.token }}
        run: |
          tag="${GITHUB_REF#refs/tags/}"
          gh release create "$tag" assets/* --repo "$GITHUB_REPOSITORY" --title "locrin $tag" --generate-notes
```

- [ ] **Step 3: Dry-run the build matrix**

```bash
cd <repo> && git add Cargo.toml Cargo.lock .github/workflows/release.yml && git commit -m "release: tag-triggered four-target build with checksums; version 0.3.0" && git push && gh workflow run release.yml --ref engine/ci -f dry-run=true && sleep 30 && gh run list --workflow release.yml --limit 1
```
Then `gh run watch <id> --exit-status`. Expected: four `build` jobs green, `publish` skipped. Download one artifact with `gh run download <id> -n x86_64-pc-windows-msvc -D /tmp/locrin-dry` and confirm the zip contains `locrin.exe` that prints `locrin 0.3.0` with `--version`. Record the four build durations in the task report.

---

### Task 5: The composite action and its smoke test

**Files:**
- Create: `action/action.yml`
- Create: `action/README.md`
- Modify: `.github/workflows/ci.yml` (add `action-smoke` job)

**Interfaces:**
- Produces: `uses: BilalEjaz/locrin/action@<ref>` with inputs `version` (`latest`, a tag like `v0.3.0`, or `local` meaning a `locrin` already on PATH), `path` (repo-relative working directory, default `.`), `base` (ref; default `${{ github.event.pull_request.base.sha }}` when present), `since` (ref; default empty), `comment` (`true`), `sarif` (`true`), `offline` (`false`), `fail-on-block` (`true`), `token` (default `${{ github.token }}`); outputs `exit-code`, `status` (`pass`, `advisory`, `block`, `error`), `sarif-file`, `comment-file`.

- [ ] **Step 1: Write the action**

`action/action.yml`:
```yaml
name: "Locrin"
description: "Deterministic quality gate for code written by people and agents: verdict comment, SARIF upload, check status"
branding:
  icon: shield
  color: gray-dark

inputs:
  version:
    description: "Release tag (v0.3.0), latest, or local (a locrin already on PATH)"
    default: latest
  path:
    description: "Repository-relative directory to check"
    default: "."
  base:
    description: "Base ref for the pull-request view (files that differ from the merge base)"
    default: ${{ github.event.pull_request.base.sha }}
  since:
    description: "Ref for the deployment gate (files changed by REF..HEAD); overrides base"
    default: ""
  comment:
    description: "Post or update one summary comment on the pull request"
    default: "true"
  sarif:
    description: "Upload SARIF to code scanning (needs security-events: write)"
    default: "true"
  offline:
    description: "Never touch the network (vulnerable-dependency uses its snapshot or skips)"
    default: "false"
  fail-on-block:
    description: "Fail the step when the verdict is BLOCK"
    default: "true"
  token:
    description: "Token for the comment and the release download"
    default: ${{ github.token }}

outputs:
  exit-code:
    value: ${{ steps.check.outputs.exit-code }}
  status:
    value: ${{ steps.check.outputs.status }}
  sarif-file:
    value: ${{ steps.check.outputs.sarif-file }}
  comment-file:
    value: ${{ steps.check.outputs.comment-file }}

runs:
  using: composite
  steps:
    - name: Install locrin
      if: ${{ inputs.version != 'local' }}
      shell: bash
      env:
        GH_TOKEN: ${{ inputs.token }}
        REPO: BilalEjaz/locrin
        VERSION: ${{ inputs.version }}
      run: |
        set -euo pipefail
        case "${RUNNER_OS}-${RUNNER_ARCH}" in
          Linux-X64)   target=x86_64-unknown-linux-gnu; ext=tar.gz ;;
          macOS-ARM64) target=aarch64-apple-darwin;    ext=tar.gz ;;
          macOS-X64)   target=x86_64-apple-darwin;     ext=tar.gz ;;
          Windows-X64) target=x86_64-pc-windows-msvc;  ext=zip ;;
          *) echo "unsupported runner ${RUNNER_OS}-${RUNNER_ARCH}" >&2; exit 2 ;;
        esac
        if [[ "$VERSION" == "latest" ]]; then
          VERSION=$(gh release view --repo "$REPO" --json tagName -q .tagName)
        fi
        dir="${RUNNER_TEMP}/locrin"
        mkdir -p "$dir" && cd "$dir"
        asset="locrin-${VERSION}-${target}.${ext}"
        gh release download "$VERSION" --repo "$REPO" -p "$asset" -p SHA256SUMS --clobber
        grep " $asset\$" SHA256SUMS | sha256sum -c -
        if [[ "$ext" == "zip" ]]; then 7z x -y "$asset" >/dev/null; else tar -xzf "$asset"; fi
        echo "$dir/locrin-${VERSION}-${target}" >> "$GITHUB_PATH"

    - name: Check
      id: check
      shell: bash
      working-directory: ${{ inputs.path }}
      env:
        LOCRIN_RUN_URL: ${{ github.server_url }}/${{ github.repository }}/actions/runs/${{ github.run_id }}
        BASE: ${{ inputs.base }}
        SINCE: ${{ inputs.since }}
        OFFLINE: ${{ inputs.offline }}
      run: |
        set -uo pipefail
        sarif="${RUNNER_TEMP}/locrin.sarif"
        comment="${RUNNER_TEMP}/locrin-comment.md"
        args=(check --markdown --sarif-file "$sarif")
        if [[ -n "$SINCE" ]]; then args+=(--since "$SINCE"); elif [[ -n "$BASE" ]]; then args+=(--base "$BASE"); fi
        if [[ "$OFFLINE" == "true" ]]; then args+=(--offline); fi
        locrin --version
        locrin "${args[@]}" > "$comment"; code=$?
        cat "$comment"
        case "$code" in
          0) status=$(grep -q '### Locrin: ADVISORY' "$comment" && echo advisory || echo pass) ;;
          1) status=block ;;
          *) status=error ;;
        esac
        {
          echo "exit-code=$code"; echo "status=$status"
          echo "sarif-file=$sarif"; echo "comment-file=$comment"
        } >> "$GITHUB_OUTPUT"
        if [[ "$code" -ge 2 ]]; then exit "$code"; fi

    - name: Upload SARIF
      if: ${{ inputs.sarif == 'true' && steps.check.outputs.status != 'error' }}
      uses: github/codeql-action/upload-sarif@v3
      with:
        sarif_file: ${{ steps.check.outputs.sarif-file }}
        category: locrin
      continue-on-error: true

    - name: Comment on the pull request
      if: ${{ inputs.comment == 'true' && github.event_name == 'pull_request' }}
      shell: bash
      env:
        GH_TOKEN: ${{ inputs.token }}
        PR: ${{ github.event.pull_request.number }}
        FILE: ${{ steps.check.outputs.comment-file }}
      run: |
        set -euo pipefail
        existing=$(gh api "repos/${GITHUB_REPOSITORY}/issues/${PR}/comments" --paginate \
          --jq '[.[] | select(.body | startswith("<!-- locrin-report -->"))][0].id // empty')
        if [[ -n "$existing" ]]; then
          gh api -X PATCH "repos/${GITHUB_REPOSITORY}/issues/comments/${existing}" -F body=@"$FILE" >/dev/null
        else
          gh api -X POST "repos/${GITHUB_REPOSITORY}/issues/${PR}/comments" -F body=@"$FILE" >/dev/null
        fi

    - name: Verdict
      if: ${{ inputs.fail-on-block == 'true' && steps.check.outputs.status == 'block' }}
      shell: bash
      run: |
        echo "::error::Locrin: BLOCK. See the pull-request comment or the run log." && exit 1
```

Notes for the implementer: `github/codeql-action/upload-sarif` needs `security-events: write` and, on private repositories, GitHub Advanced Security; it is `continue-on-error` so a repo without code scanning still gets the comment and the status. The comment lookup uses the marker on the first line of the body. On Windows runners `7z` is preinstalled; on Linux and macOS the tarball path is used.

- [ ] **Step 2: Write the action README**

`action/README.md`: inputs and outputs tables copied from `action.yml`, required permissions (`contents: read`, `pull-requests: write` for the comment, `security-events: write` for SARIF), the three examples by reference to `action/examples/`, and the rule that the comment is edited in place. Under 80 lines.

- [ ] **Step 3: Add the smoke job**

Append to `.github/workflows/ci.yml` under `jobs:`:
```yaml
  action-smoke:
    needs: lint
    runs-on: ubuntu-latest
    permissions:
      contents: read
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo build --release -p locrin-cli && echo "$PWD/target/release" >> "$GITHUB_PATH"
      - id: gate
        uses: ./action
        with:
          version: local
          path: crates/cli/tests/fixtures/repo
          comment: "false"
          sarif: "false"
          offline: "true"
          fail-on-block: "false"
      - name: The fixture must block
        run: |
          test "${{ steps.gate.outputs.status }}" = "block"
          test "${{ steps.gate.outputs.exit-code }}" = "1"
          grep -q '<!-- locrin-report -->' "${{ steps.gate.outputs.comment-file }}"
          python3 -c "import json,sys; d=json.load(open('${{ steps.gate.outputs.sarif-file }}')); assert d['version']=='2.1.0' and len(d['runs'][0]['results'])>=2"
```

- [ ] **Step 4: Push and watch**

```bash
cd <repo> && git add action/action.yml action/README.md .github/workflows/ci.yml && git commit -m "action: composite GitHub Action with comment, SARIF and status; smoke job" && git push && sleep 20 && gh run list --branch engine/ci --limit 1
```
Then `gh run watch <id> --exit-status`. Expected: `lint`, both `test`, and `action-smoke` green. The pull-request comment path cannot be exercised in the smoke job (it runs on `pull_request` events only against the PR of this branch); Task 7 covers it.

---

### Task 6: Examples, the FastLift workflow, and README

**Files:**
- Create: `action/examples/pull-request.yml`, `action/examples/deploy-gate.yml`, `action/examples/fastlift.yml`
- Modify: `README.md` (Install section; new sections "GitHub Action" and "Deployment gate" before "Exit codes")

- [ ] **Step 1: Write the examples**

`action/examples/pull-request.yml`:
```yaml
name: locrin
on:
  pull_request:
permissions:
  contents: read
  pull-requests: write
  security-events: write
jobs:
  gate:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0
      - uses: BilalEjaz/locrin/action@v0.3.0
```

`action/examples/deploy-gate.yml`:
```yaml
name: deploy gate
on:
  push:
    branches: [main]
permissions:
  contents: read
jobs:
  gate:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0
      - id: last
        run: echo "tag=$(git describe --tags --match 'deploy-*' --abbrev=0 2>/dev/null || git rev-list --max-parents=0 HEAD)" >> "$GITHUB_OUTPUT"
      - uses: BilalEjaz/locrin/action@v0.3.0
        with:
          since: ${{ steps.last.outputs.tag }}
          comment: "false"
      # deploy steps follow; they never run when the gate blocks
```

`action/examples/fastlift.yml` is the pull-request example with the job named `locrin` and a comment at the top: "Copy to fasting-app/.github/workflows/locrin.yml. Requires the locrin wiring (locrin.toml, locrin-baseline.json) committed. Needs fetch-depth 0 for --base."

- [ ] **Step 2: README**

Install section: keep `cargo install --path crates/cli`, add "Release binaries for Linux, macOS (Intel and Apple silicon) and Windows are attached to every GitHub release with a `SHA256SUMS` file." Add a "GitHub Action" section (what it does, the three permissions, the minimal example, the comment marker rule, `LOCRIN_RUN_URL`) and a "Deployment gate" section (`check --since <ref>`, the deploy-tag pattern, exit codes drive the pipeline). Add `--markdown` and `--sarif-file` to the Commands section.

- [ ] **Step 3: Commit**

```bash
cd <repo> && git add action/examples README.md && git commit -m "docs: action examples, FastLift workflow, README sections for the action and the deployment gate"
```

---

### Task 7: End-to-end on a real pull request

**Files:** none new; this task proves the comment and status paths on the branch's own pull request.

- [ ] **Step 1: Open the pull request for `engine/ci` against `main`** with `gh pr create` (title "Phase two A: GitHub Action, release pipeline, Markdown and SARIF file output"; body lists the six tasks and the dry-run build times).

- [ ] **Step 2: Add a temporary workflow to exercise the comment path**

Create `.github/workflows/locrin-self.yml` on the branch: the pull-request example with `version: local` after a `cargo build --release`, `path: crates/cli/tests/fixtures/repo`, `fail-on-block: "false"`, `sarif: "false"`, permissions `pull-requests: write`. Commit as "ci: temporary self-check workflow to prove the comment path" and push. Expected on the PR: one comment starting with the marker and `### Locrin: BLOCK`. Push an empty commit (`git commit --allow-empty -m "ci: re-run"`) and confirm the SAME comment was edited (one comment, updated timestamp), not a second one. Record the comment URL in the report.

- [ ] **Step 3: Remove the temporary workflow**

Delete `.github/workflows/locrin-self.yml`, commit as "ci: remove the temporary self-check workflow", push. The `ci` workflow with the smoke job remains.

- [ ] **Step 4: Report** the run URLs, the comment URL, and the four dry-run build durations. The release itself (`git tag v0.3.0 && git push origin v0.3.0`) is the founder's action after merge; it publishes the assets the action's `version: latest` path downloads.

---

## Self-review

**Spec coverage.** 8.1: one comment edited in place (Task 5 comment step, proven in Task 7), SARIF upload (Task 5), check status via step failure on block (Task 5 Verdict step), `check --base` (existing, wired in Task 5). 8.2: `check --since` deployment gate with the deploy-tag example (Task 6). 7.2 to 7.4: Markdown reporter capped at ten and ordered by the verdict (Task 1), SARIF file with every finding (Task 2), exit codes unchanged (Task 2 and Task 5). Distribution: release binaries with checksums (Task 4); the npm shim and Homebrew formula from spec 3 are explicitly not in this plan. The engine remains offline-capable; the action's only network use is GitHub's own API through the runner token.

**Placeholder scan.** No TBD or TODO. Every code and YAML step carries its content. The action README's tables are specified by reference to `action.yml` inputs, which is deliberate: one source of truth.

**Type consistency.** `markdown::render(v, version, run_url)` defined in Task 1 and called in Task 2 with the same three arguments. `sarif::render(&verdict, &rules, version)` and `RuleMeta` fields match the existing code read on 2026-09-11. Action outputs `exit-code`, `status`, `sarif-file`, `comment-file` are produced in the `check` step and consumed by the SARIF, comment, verdict steps and the smoke job with the same names. The asset naming rule in Global Constraints is what Task 4 packages and Task 5 downloads.

**Known judgment calls for the executor.** `upload-sarif` is `continue-on-error` so repositories without code scanning still get the comment and status. The `status` output distinguishes advisory from pass by grepping the Markdown header, which is stable by Task 1's contract. The comment lookup pages through all comments; on very long PRs that is a few API calls, acceptable.
