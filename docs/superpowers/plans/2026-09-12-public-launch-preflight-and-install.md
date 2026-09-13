# Public launch, plan A: pre-flight and install channels

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the locrin repository safe to open and give it five install channels (npm, PyPI, Homebrew, curl and PowerShell installers, cargo) that all publish from one release run.

**Architecture:** The existing release workflow keeps building four platform binaries and SHA256SUMS. New packaging code lives beside the Rust workspace, one directory per channel (`npm/`, `pypi/`, `scripts/`, `Formula/` generated), each with its own tests, and a new publish stage in the release workflow stages the built binaries into every channel and publishes them in order. Pre-flight scripts (history secret scan, private reference check) live in `scripts/` and the reference check runs in CI so the tree stays clean after the flip.

**Tech Stack:** Rust workspace (unchanged), bash scripts tested with a tiny bash harness, Node 18+ with `node:test` for the npm launcher, Python 3.9+ standard library for the wheel builder, PowerShell 5.1 for the Windows installer, GitHub Actions.

**Spec:** docs/superpowers/specs/2026-09-12-public-launch-design.md (sections 3, 4, 6 and 7; the benchmark in section 5 is plan B, written after this plan lands).

## Global Constraints

- The repository stays private until the founder says "make it public". No task in this plan flips visibility, creates a GitHub release, or publishes to any registry. The publish job is written and dry-run only.
- Workspace version stays `0.4.0` until Task 14, which bumps it to `0.5.0`. Nothing is tagged in this plan.
- Repo: the locrin checkout this plan lives in (open Git Bash there). Cargo commands need `export PATH="$USERPROFILE/.cargo/bin:$PATH"` first. Local tools: node 24, npm 11, python 3.12, PowerShell 5.1. A `locrin` 0.4.0 binary is on PATH.
- Every commit: `cargo fmt --all --check` clean, `cargo clippy --workspace --all-targets -- -D warnings` clean, `cargo test --workspace` green (547 tests today plus whatever the task adds). Rust style: rustfmt.toml sets max_width 120.
- Line endings LF (`.gitattributes` has `* text=auto eol=lf`). Shell scripts start with `#!/usr/bin/env bash` and `set -euo pipefail`. PowerShell targets 5.1: no `&&`, no `||` chains, no ternary, no `?.`.
- No em dashes in any text. No `Co-Authored-By` or any AI attribution trailer in commits. Never `git add -A`; stage by explicit path. Never write into the founder's other repositories (the FastLift checkout beside this one included).
- Names are fixed by the spec: npm entry package `locrin`; platform packages `@raxbi/locrin-linux-x64`, `@raxbi/locrin-darwin-x64`, `@raxbi/locrin-darwin-arm64`, `@raxbi/locrin-win32-x64`; PyPI package `locrin`; Homebrew tap `BilalEjaz/homebrew-locrin`, formula `locrin`; release asset names `locrin-<tag>-<target>.tar.gz` (Linux and macOS) and `locrin-<tag>-<target>.zip` (Windows); targets `x86_64-unknown-linux-gnu`, `aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-pc-windows-msvc`.
- Exit codes are part of the contract: 0 pass or advisory, 1 block, 2 engine error. Every launcher and installer forwards or uses them exactly.
- TDD: write the failing test, run it red, implement, run it green, commit. One commit per task unless a task says otherwise.
- Branch: `launch/preflight-and-install` off main (c349dc7 or later). One pull request at the end.

## File structure

Created:
- `scripts/package.sh`: packages a built binary into the release asset for one target (extracted from release.yml so the release job and the CI smoke job share it).
- `scripts/scan-history.sh`: runs locrin's secret rule and gitleaks over every commit; the pre-flight history scan.
- `scripts/check-private-refs.sh`: fails on local paths, personal addresses and token prefixes anywhere in the tree; runs in CI.
- `scripts/check-versions.sh`: fails when any path dependency version differs from the workspace version; runs in CI.
- `scripts/homebrew-formula.sh`: writes `Formula/locrin.rb` from a version and a SHA256SUMS file.
- `scripts/tests/run.sh`, `scripts/tests/*.test.sh`, `scripts/tests/install.test.ps1`: the shell test harness and tests.
- `scripts/ci/install-smoke.sh`: the CI job body that builds every channel from a local binary and installs each one.
- `install.sh`, `install.ps1`: the curl and PowerShell installers.
- `npm/locrin/{package.json,bin/locrin.js,lib/resolve.js,README.md,test/*.test.js}`: the npm entry package.
- `npm/platforms/<name>/package.json` for the four platform packages, plus `npm/scripts/stage.js` that stamps versions and copies binaries in.
- `pypi/build_wheel.py`, `pypi/locrin/__init__.py`, `pypi/locrin/__main__.py`, `pypi/test_build_wheel.py`, `pypi/README.md`.
- `LICENSE`, `SECURITY.md`, `CONTRIBUTING.md`, `.github/ISSUE_TEMPLATE/{false-positive.yml,bug.yml,rule-request.yml,config.yml}`.

Modified:
- `Cargo.toml` and the five crate manifests: publish metadata, `locrin-cli` renamed to `locrin`, path dependency versions.
- `.github/workflows/ci.yml`: lint gains the reference, version and public-file checks; new `install-smoke` job.
- `.github/workflows/release.yml`: packaging via the script; publish job gains the channel steps; new dry-run job.
- `action/action.yml`, `action/README.md`, `action/examples/*.yml`: token wording, pins.
- `README.md`: rewritten top, install section, free versus paid, benchmark link.
- `docs/RELEASE-NOTES.md`: 0.5.0 section.
- `crates/cli/tests/bench.rs`: comment naming the crate.

Deleted:
- `placeholders/` (all three): superseded by the real packages. The crates.io placeholder crate `locrin` 0.0.1 is superseded by the renamed CLI crate.

---

### Task 1: Shared packaging script

**Files:**
- Create: `scripts/package.sh`
- Create: `scripts/tests/run.sh`
- Create: `scripts/tests/package.test.sh`
- Modify: `.github/workflows/release.yml` (the Package step)

**Interfaces:**
- Produces: `scripts/package.sh <target> <tag> <bin-dir> <out-dir>` prints the asset path on stdout and writes `<out-dir>/locrin-<tag>-<target>.<ext>` containing the directory `locrin-<tag>-<target>/locrin` (or `locrin.exe`). `<ext>` is `zip` for `x86_64-pc-windows-msvc`, else `tar.gz`. Tasks 10 and 11 call it.

- [ ] **Step 1: Write the harness and the failing test**

`scripts/tests/run.sh`:
```bash
#!/usr/bin/env bash
# Runs every scripts/tests/*.test.sh and fails if any fails.
set -euo pipefail
cd "$(dirname "$0")/../.."
status=0
for t in scripts/tests/*.test.sh; do
  if bash "$t"; then echo "ok   $t"; else echo "FAIL $t"; status=1; fi
done
exit "$status"
```

`scripts/tests/package.test.sh`:
```bash
#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT

mkdir -p "$tmp/bin" "$tmp/out"
printf '#!/bin/sh\necho locrin 9.9.9\n' > "$tmp/bin/locrin"; chmod +x "$tmp/bin/locrin"
asset=$(bash scripts/package.sh x86_64-unknown-linux-gnu v9.9.9 "$tmp/bin" "$tmp/out")
[[ "$asset" == "$tmp/out/locrin-v9.9.9-x86_64-unknown-linux-gnu.tar.gz" ]] || { echo "unexpected asset path: $asset"; exit 1; }
tar -tzf "$asset" | grep -qx 'locrin-v9.9.9-x86_64-unknown-linux-gnu/locrin' || { echo "tarball layout wrong"; tar -tzf "$asset"; exit 1; }

cp "$tmp/bin/locrin" "$tmp/bin/locrin.exe"
asset=$(bash scripts/package.sh x86_64-pc-windows-msvc v9.9.9 "$tmp/bin" "$tmp/out")
[[ "$asset" == "$tmp/out/locrin-v9.9.9-x86_64-pc-windows-msvc.zip" ]] || { echo "unexpected zip path: $asset"; exit 1; }
if command -v unzip >/dev/null; then
  unzip -l "$asset" | grep -q 'locrin-v9.9.9-x86_64-pc-windows-msvc/locrin.exe' || { echo "zip layout wrong"; exit 1; }
else
  7z l "$asset" | grep -q 'locrin.exe' || { echo "zip layout wrong"; exit 1; }
fi

if bash scripts/package.sh riscv64gc-unknown-linux-gnu v9.9.9 "$tmp/bin" "$tmp/out" 2>/dev/null; then
  echo "unknown target must fail"; exit 1
fi
```

- [ ] **Step 2: Run it red**

Run: `bash scripts/tests/run.sh`
Expected: `FAIL scripts/tests/package.test.sh` (script missing).

- [ ] **Step 3: Write the script**

`scripts/package.sh`:
```bash
#!/usr/bin/env bash
# Package a built locrin binary into the release asset for one target.
# Usage: scripts/package.sh <target> <tag> <bin-dir> <out-dir>
# Prints the asset path. The archive holds one directory, locrin-<tag>-<target>,
# with the binary inside, which is what action/action.yml and install.sh expect.
set -euo pipefail
target="$1"; tag="$2"; bin_dir="$3"; out_dir="$4"
case "$target" in
  x86_64-unknown-linux-gnu|aarch64-apple-darwin|x86_64-apple-darwin) ext=tar.gz; bin=locrin ;;
  x86_64-pc-windows-msvc) ext=zip; bin=locrin.exe ;;
  *) echo "package.sh: unknown target $target" >&2; exit 2 ;;
esac
name="locrin-${tag}-${target}"
mkdir -p "$out_dir/$name"
cp "$bin_dir/$bin" "$out_dir/$name/$bin"
if [[ "$ext" == "zip" ]]; then
  (cd "$out_dir" && rm -f "$name.zip" && 7z a -tzip "$name.zip" "$name" >/dev/null)
else
  chmod +x "$out_dir/$name/$bin"
  tar -C "$out_dir" -czf "$out_dir/$name.tar.gz" "$name"
fi
rm -rf "${out_dir:?}/$name"
echo "$out_dir/$name.$ext"
```

- [ ] **Step 4: Run it green**

Run: `bash scripts/tests/run.sh`
Expected: `ok   scripts/tests/package.test.sh`. (7z is on the Windows box and on GitHub runners; on a machine without it the zip case fails, which is correct.)

- [ ] **Step 5: Use it in release.yml**

Replace the whole `Package` step in `.github/workflows/release.yml` with:
```yaml
      - name: Package
        id: pkg
        shell: bash
        run: |
          asset=$(bash scripts/package.sh '${{ matrix.target }}' '${{ steps.tag.outputs.tag }}' 'target/${{ matrix.target }}/release' dist)
          echo "asset=$asset" >> "$GITHUB_OUTPUT"
```
Remove the `ext:` lines from the matrix (the script decides the extension).

- [ ] **Step 6: Commit**

```bash
git add scripts/package.sh scripts/tests/run.sh scripts/tests/package.test.sh .github/workflows/release.yml
git commit -m "release: packaging in a shared script with a test"
```

---

### Task 2: Crate metadata and the `locrin` crate name

**Files:**
- Modify: `Cargo.toml` (workspace.package)
- Modify: `crates/core/Cargo.toml`, `crates/rules/Cargo.toml`, `crates/reporters/Cargo.toml`, `crates/mcp/Cargo.toml`, `crates/cli/Cargo.toml`
- Modify: `.github/workflows/ci.yml:46`, `.github/workflows/release.yml` (the cargo build line), `crates/cli/tests/bench.rs:2`, `README.md:541`
- Create: `scripts/check-versions.sh`, `scripts/tests/check-versions.test.sh`

**Interfaces:**
- Produces: the binary crate is named `locrin` (package name), so `cargo install locrin` and `cargo build -p locrin` work. Path dependencies carry `version = "<workspace version>"`, which crates.io requires. `scripts/check-versions.sh` exits 1 when any path dependency version differs from the workspace version.

- [ ] **Step 1: Write the failing version-check test**

`scripts/tests/check-versions.test.sh`:
```bash
#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT

# The real tree must pass.
bash scripts/check-versions.sh

# A tree with a drifted path dependency must fail.
mkdir -p "$tmp/crates/a" "$tmp/crates/b"
cat > "$tmp/Cargo.toml" <<'EOF'
[workspace]
members = ["crates/a", "crates/b"]
[workspace.package]
version = "1.2.3"
EOF
cat > "$tmp/crates/a/Cargo.toml" <<'EOF'
[package]
name = "a"
version.workspace = true
EOF
cat > "$tmp/crates/b/Cargo.toml" <<'EOF'
[package]
name = "b"
version.workspace = true
[dependencies]
a = { path = "../a", version = "1.2.2" }
EOF
if bash scripts/check-versions.sh "$tmp" 2>/dev/null; then echo "drift must fail"; exit 1; fi
```

- [ ] **Step 2: Run it red**

Run: `bash scripts/tests/run.sh`
Expected: `FAIL scripts/tests/check-versions.test.sh`.

- [ ] **Step 3: Write the check**

`scripts/check-versions.sh`:
```bash
#!/usr/bin/env bash
# Every `{ path = "...", version = "X" }` dependency must carry the workspace version,
# because crates.io ignores `path` and resolves by `version`.
# Usage: scripts/check-versions.sh [repo-root]
set -euo pipefail
root="${1:-$(cd "$(dirname "$0")/.." && pwd)}"
want=$(grep -m1 '^version' "$root/Cargo.toml" | sed 's/.*"\(.*\)"/\1/')
status=0
for manifest in "$root"/crates/*/Cargo.toml; do
  while IFS= read -r line; do
    got=$(sed -n 's/.*version *= *"\([^"]*\)".*/\1/p' <<<"$line")
    if [[ -z "$got" ]]; then
      echo "$manifest: path dependency without a version: $line" >&2; status=1
    elif [[ "$got" != "$want" ]]; then
      echo "$manifest: path dependency at $got, workspace is $want: $line" >&2; status=1
    fi
  done < <(grep -E '^\s*[A-Za-z0-9_-]+ *= *\{[^}]*path *=' "$manifest" || true)
done
exit "$status"
```

- [ ] **Step 4: Update the manifests**

Root `Cargo.toml`, replace the `[workspace.package]` table with:
```toml
[workspace.package]
edition = "2021"
version = "0.4.0"
license = "MIT"
repository = "https://github.com/BilalEjaz/locrin"
homepage = "https://locrin.com"
```

In each of the five crate manifests, add after `license.workspace = true`:
```toml
repository.workspace = true
homepage.workspace = true
```
and a `description` line per crate:
- core: `description = "Locrin engine: tree-sitter parsing, SQLite index, findings and verdicts"`
- rules: `description = "Locrin rules: the deterministic rule set for TypeScript, JavaScript, PHP and Python"`
- reporters: `description = "Locrin reporters: terminal, agent JSON, Markdown and SARIF output"`
- mcp: `description = "Locrin MCP server: the quality gate as tools for coding agents"`
- cli: `description = "Locrin: the deterministic quality gate for code written by people and agents"`

Add `readme = "../../README.md"` to the cli crate only.

In `crates/cli/Cargo.toml` change `name = "locrin-cli"` to `name = "locrin"`. Change every path dependency in every crate to carry the version, for example in cli:
```toml
locrin-core = { path = "../core", version = "0.4.0" }
locrin-rules = { path = "../rules", version = "0.4.0" }
locrin-reporters = { path = "../reporters", version = "0.4.0" }
locrin-mcp = { path = "../mcp", version = "0.4.0" }
```
and the same shape for `locrin-core` in rules, reporters and mcp (check each manifest's `[dependencies]`).

Replace `locrin-cli` with `locrin` in `.github/workflows/ci.yml` line 46, the `cargo build --release -p` line in `.github/workflows/release.yml`, `crates/cli/tests/bench.rs` line 2, and `README.md` line 541.

- [ ] **Step 5: Run everything green**

Run:
```bash
export PATH="$USERPROFILE/.cargo/bin:$PATH"
bash scripts/tests/run.sh
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
cargo publish --dry-run -p locrin-core
```
Expected: harness ok; 547 tests pass; the dry run packages and verifies locrin-core (it needs the crates.io index, so network). If `cargo publish --dry-run` complains about a missing field, add that field; description, license and repository are what crates.io requires.

- [ ] **Step 6: Add the check to CI**

In `.github/workflows/ci.yml`, in the `lint` job after the clippy line:
```yaml
      - run: bash scripts/tests/run.sh
      - run: bash scripts/check-versions.sh
```

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock crates/core/Cargo.toml crates/rules/Cargo.toml crates/reporters/Cargo.toml crates/mcp/Cargo.toml crates/cli/Cargo.toml .github/workflows/ci.yml .github/workflows/release.yml crates/cli/tests/bench.rs README.md scripts/check-versions.sh scripts/tests/check-versions.test.sh
git commit -m "crates: publish metadata, the binary crate is named locrin, path dependency versions checked"
```

---

### Task 3: History secret scan

**Files:**
- Create: `scripts/scan-history.sh`
- Create: `scripts/tests/scan-history.test.sh`

**Interfaces:**
- Produces: `scripts/scan-history.sh [repo]` exits 0 when neither locrin's `secret-exposed` rule nor gitleaks finds anything in any commit reachable from any ref, exits 1 with the offending commit and file otherwise. It is run once by hand as the pre-flight record and kept for re-runs.

- [ ] **Step 1: Write the failing test**

`scripts/tests/scan-history.test.sh`:
```bash
#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
command -v locrin >/dev/null || { echo "skip: locrin not on PATH"; exit 0; }
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
export LOCRIN_CACHE_DIR="$tmp/cache" GITLEAKS_CACHE="$tmp/gitleaks"

mk() { git -C "$1" -c user.name=t -c user.email=t@t -c commit.gpgsign=false "${@:2}"; }
# Clean repo: passes.
git init -q "$tmp/clean"; echo 'export const a = 1;' > "$tmp/clean/a.ts"; mk "$tmp/clean" add a.ts; mk "$tmp/clean" commit -qm one
bash scripts/scan-history.sh "$tmp/clean"

# A secret committed then deleted: still found in history.
git init -q "$tmp/dirty"; echo 'export const a = 1;' > "$tmp/dirty/a.ts"; mk "$tmp/dirty" add a.ts; mk "$tmp/dirty" commit -qm one
printf 'const key = "AKIA%s";\n' "IOSFODNN7EXAMPLE" > "$tmp/dirty/k.ts"; mk "$tmp/dirty" add k.ts; mk "$tmp/dirty" commit -qm two
mk "$tmp/dirty" rm -q k.ts; mk "$tmp/dirty" commit -qm three
if bash scripts/scan-history.sh "$tmp/dirty" >"$tmp/out" 2>&1; then cat "$tmp/out"; echo "planted secret must fail"; exit 1; fi
grep -q 'k.ts' "$tmp/out" || { cat "$tmp/out"; echo "report must name the file"; exit 1; }
```

- [ ] **Step 2: Run it red**

Run: `bash scripts/tests/run.sh`
Expected: `FAIL scripts/tests/scan-history.test.sh`.

- [ ] **Step 3: Write the scanner**

`scripts/scan-history.sh`:
```bash
#!/usr/bin/env bash
# Pre-flight for going public: every commit reachable from any ref is checked out
# into a temporary directory and run through locrin's secret-exposed rule, then
# gitleaks (pinned, checksum-verified) scans the whole history in one pass.
# Usage: scripts/scan-history.sh [repo]   (default: this repository)
set -euo pipefail
repo="${1:-$(cd "$(dirname "$0")/.." && pwd)}"
GITLEAKS_VERSION="8.30.1"
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
status=0

# Part 1: locrin's own rule, one tree per commit.
while read -r commit; do
  tree="$tmp/tree"; rm -rf "$tree"; mkdir -p "$tree"
  git -C "$repo" archive "$commit" | tar -x -C "$tree"
  if [[ -z "$(ls -A "$tree")" ]]; then continue; fi
  out=$(cd "$tree" && LOCRIN_CACHE_DIR="${LOCRIN_CACHE_DIR:-$tmp/cache}" locrin check --json --offline . 2>/dev/null || true)
  hits=$(printf '%s' "$out" | python3 -c '
import json,sys
try: d=json.load(sys.stdin)
except Exception: sys.exit(0)
for f in d.get("findings",[]):
    if f.get("rule")=="secret-exposed": print(f.get("file"), f.get("line"))
')
  if [[ -n "$hits" ]]; then
    echo "secret-exposed in commit $commit:" >&2; echo "$hits" >&2; status=1
  fi
done < <(git -C "$repo" rev-list --all)

# Part 2: gitleaks over the full history.
cache="${GITLEAKS_CACHE:-$HOME/.cache/locrin-gitleaks}"
bin="$cache/gitleaks"
if [[ ! -x "$bin" ]]; then
  mkdir -p "$cache"
  case "$(uname -s)-$(uname -m)" in
    Linux-x86_64) a="linux_x64.tar.gz" ;;
    Darwin-arm64) a="darwin_arm64.tar.gz" ;;
    Darwin-x86_64) a="darwin_x64.tar.gz" ;;
    MINGW*|MSYS*|CYGWIN*) a="windows_x64.zip"; bin="$cache/gitleaks.exe" ;;
    *) echo "scan-history: no gitleaks build for this platform" >&2; exit 2 ;;
  esac
  base="https://github.com/gitleaks/gitleaks/releases/download/v$GITLEAKS_VERSION"
  curl -fsSL "$base/gitleaks_${GITLEAKS_VERSION}_$a" -o "$cache/pkg"
  curl -fsSL "$base/gitleaks_${GITLEAKS_VERSION}_checksums.txt" -o "$cache/sums"
  want=$(grep " gitleaks_${GITLEAKS_VERSION}_$a\$" "$cache/sums" | awk '{print $1}')
  if command -v sha256sum >/dev/null; then got=$(sha256sum "$cache/pkg" | awk '{print $1}'); else got=$(shasum -a 256 "$cache/pkg" | awk '{print $1}'); fi
  [[ "$want" == "$got" ]] || { echo "scan-history: gitleaks checksum mismatch" >&2; exit 2; }
  if [[ "$a" == *.zip ]]; then (cd "$cache" && 7z x -y pkg >/dev/null); else tar -xzf "$cache/pkg" -C "$cache"; fi
  chmod +x "$bin"
fi
if ! "$bin" git --no-banner --redact --exit-code 1 "$repo"; then
  echo "gitleaks reported findings" >&2; status=1
fi
exit "$status"
```
If `locrin check --json` prints its findings under a different key than `findings` or the file key differs, read `crates/reporters/src` for the agent JSON shape and adjust the two field names in the Python snippet; the test tells you.

- [ ] **Step 4: Run it green**

Run: `bash scripts/tests/run.sh`
Expected: `ok   scripts/tests/scan-history.test.sh` (first run downloads gitleaks once into `$GITLEAKS_CACHE`).

- [ ] **Step 5: Run the real pre-flight scan and record it**

Run: `bash scripts/scan-history.sh 2>&1 | tail -20; echo "exit ${PIPESTATUS[0]}"`
Expected: exit 0. If it is not 0, stop and report the commit and file in the task report; the lead decides on the history rewrite. Do not rewrite history yourself.

Record the command and its exit code in the pull request description under "Pre-flight history scan".

- [ ] **Step 6: Commit**

```bash
git add scripts/scan-history.sh scripts/tests/scan-history.test.sh
git commit -m "repo: history secret scan (locrin rule per commit plus gitleaks)"
```

---

### Task 4: Private reference sweep

**Files:**
- Create: `scripts/check-private-refs.sh`, `scripts/tests/check-private-refs.test.sh`
- Modify: any file the check reports (today: the earlier plan documents under `docs/superpowers/plans/` and a comment in `crates/cli/tests/bench.rs`)
- Modify: `.github/workflows/ci.yml` (lint job)

**Interfaces:**
- Produces: `scripts/check-private-refs.sh [root]` exits 1 and lists file:line for every match of a private pattern; exits 0 on a clean tree. Runs in CI lint from now on.

- [ ] **Step 1: Write the failing test**

`scripts/tests/check-private-refs.test.sh`:
```bash
#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT

mkdir -p "$tmp/clean/docs" "$tmp/dirty/docs"
echo 'The cache lives in the user cache directory.' > "$tmp/clean/docs/a.md"
bash scripts/check-private-refs.sh "$tmp/clean"

# The literals are split so this test file never matches its own patterns.
who='ch''ars'; mail='gm''ail'
printf 'see C:\\Users\\%s\\x\n' "$who" > "$tmp/dirty/docs/a.md"
printf 'mail someone@%s.com\n' "$mail" > "$tmp/dirty/docs/b.md"
printf 'token github_pat_11AAAA\n' > "$tmp/dirty/docs/c.md"
printf 'from /c/Users/%s/x\n' "$who" > "$tmp/dirty/docs/d.md"
if bash scripts/check-private-refs.sh "$tmp/dirty" > "$tmp/out" 2>&1; then echo "dirty tree must fail"; exit 1; fi
for f in a.md b.md c.md d.md; do grep -q "$f" "$tmp/out" || { cat "$tmp/out"; echo "must report $f"; exit 1; }; done

# The real tree, including this test and the plan that describes it, must be clean.
bash scripts/check-private-refs.sh "$(pwd)"
```

- [ ] **Step 2: Run it red**

Run: `bash scripts/tests/run.sh`
Expected: `FAIL scripts/tests/check-private-refs.test.sh`.

- [ ] **Step 3: Write the check**

`scripts/check-private-refs.sh`:
```bash
#!/usr/bin/env bash
# Fails when the tree contains local paths, personal addresses or token prefixes.
# Usage: scripts/check-private-refs.sh [root]
set -euo pipefail
root="${1:-$(cd "$(dirname "$0")/.." && pwd)}"
# Literals are split so this script never matches itself.
who='ch''ars'; mail='gm''ail'; site='hikmah''learn'
patterns=(
  "Users[\\\\/]+$who"
  'AppData[\\/]+Local'
  "[A-Za-z0-9._%+-]+@$mail\\.com"
  "$site"
  'ghp_[A-Za-z0-9]{20,}'
  'github_pat_[A-Za-z0-9_]{10,}'
  'npm_[A-Za-z0-9]{30,}'
  'pypi-AgEIcHlwaS5vcmc'
)
regex=$(IFS='|'; echo "${patterns[*]}")
hits=$(grep -rnE --binary-files=without-match \
  --exclude-dir=.git --exclude-dir=target --exclude-dir=node_modules --exclude-dir=.venv --exclude-dir=data \
  -- "$regex" "$root" || true)
if [[ -n "$hits" ]]; then
  echo "private references found:" >&2
  echo "$hits" >&2
  exit 1
fi
```

- [ ] **Step 4: Run it green on the fixtures, then on the real tree**

Run: `bash scripts/tests/run.sh`
Expected: the fixture parts pass; the final self-check line fails if the real tree has hits. Run `bash scripts/check-private-refs.sh` and fix every hit:
- Local absolute paths in plans and ledgers under `docs/superpowers/plans/`: replace with repository-relative paths (a Windows path into this checkout becomes the path relative to the repository root), or with the phrase "the scratchpad directory" where the path was a temp dir.
- The e-mail address, if present anywhere, is removed.
- Do not touch `target/`, `spike/fingerprint/data/` (gitignored) or anything under `.git/`.
Re-run until `bash scripts/tests/run.sh` is fully green.

- [ ] **Step 5: Confirm the FastLift files never entered this repo**

Run: `git log --all --diff-filter=A --name-only --pretty=format: | sort -u | grep -E 'locrin-baseline\.json|locrin\.toml' || echo "none"`
Expected: only fixture paths under `crates/cli/tests/fixtures/` (if any) and `none` otherwise. Record the output in the pull request description under "Private reference sweep".

- [ ] **Step 6: Add to CI lint**

In `.github/workflows/ci.yml` lint job, after the check-versions line:
```yaml
      - run: bash scripts/check-private-refs.sh
```

- [ ] **Step 7: Commit**

```bash
git add scripts/check-private-refs.sh scripts/tests/check-private-refs.test.sh .github/workflows/ci.yml
git add <every doc file you edited, by path>
git commit -m "repo: private reference check in CI; local paths scrubbed from plans and ledgers"
```

---

### Task 5: Files a public repository needs

**Files:**
- Create: `LICENSE`, `SECURITY.md`, `CONTRIBUTING.md`
- Create: `.github/ISSUE_TEMPLATE/false-positive.yml`, `.github/ISSUE_TEMPLATE/bug.yml`, `.github/ISSUE_TEMPLATE/rule-request.yml`, `.github/ISSUE_TEMPLATE/config.yml`
- Create: `scripts/tests/public-files.test.sh`
- Delete: `placeholders/` (three directories)
- Modify: `.gitignore` (drop the four `placeholders/` lines)

- [ ] **Step 1: Write the failing test**

`scripts/tests/public-files.test.sh`:
```bash
#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
for f in LICENSE SECURITY.md CONTRIBUTING.md .github/ISSUE_TEMPLATE/false-positive.yml .github/ISSUE_TEMPLATE/bug.yml .github/ISSUE_TEMPLATE/rule-request.yml .github/ISSUE_TEMPLATE/config.yml; do
  [[ -f "$f" ]] || { echo "missing $f"; exit 1; }
done
grep -q '^MIT License' LICENSE || { echo "LICENSE is not MIT"; exit 1; }
grep -q 'Raxbi Ltd' LICENSE || { echo "LICENSE holder"; exit 1; }
grep -q 'Report a vulnerability' SECURITY.md || { echo "SECURITY.md must point at private reporting"; exit 1; }
grep -q 'cargo test --workspace' CONTRIBUTING.md || { echo "CONTRIBUTING.md must give the test command"; exit 1; }
[[ ! -d placeholders ]] || { echo "placeholders must be gone"; exit 1; }
```

- [ ] **Step 2: Run it red**

Run: `bash scripts/tests/run.sh`
Expected: `FAIL scripts/tests/public-files.test.sh`.

- [ ] **Step 3: Write the files**

`LICENSE`: the standard MIT text with the first line `MIT License` and the copyright line `Copyright (c) 2026 Raxbi Ltd`.

`SECURITY.md`:
```markdown
# Security

Locrin runs on your source code, so a flaw in it matters. Please report
vulnerabilities privately.

## Reporting

Use GitHub's private reporting: open the repository's Security tab and choose
"Report a vulnerability". Do not open a public issue for a security problem.

You will get an acknowledgement within three working days and a fix or a
mitigation plan within thirty days for confirmed reports. Reporters are credited
in the release notes unless they ask otherwise.

## Supported versions

The latest minor release receives fixes. Older releases do not.

## Scope

In scope: the `locrin` binary, the GitHub Action, the npm, PyPI, Homebrew and
installer packages, and the release pipeline. Out of scope: findings the engine
misses (report those as a rule request) and vulnerabilities in the projects
Locrin scans.
```

`CONTRIBUTING.md`:
```markdown
# Contributing

## Build and test

    cargo build
    cargo test --workspace
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    bash scripts/tests/run.sh

All five must pass before a pull request is reviewed. CI runs them on Linux and
Windows.

## Rules and the precision gate

A rule ships only when it reaches 85 percent precision on the labelled corpus,
per rule and per rule-language pair. The corpus is private (it is mined from
private repositories), so a pull request that adds or changes a rule must
include, in its description, at least ten labelled snippets (five that should
fire, five that should not) so the maintainer can extend the corpus and run the
gate. A rule under the line ships off by default or does not ship.

Rule ids, finding ids and exit codes are contracts. Changing how an id is
computed needs a release note and a bump of `RULES_REVISION`.

## Style

Rust is formatted by rustfmt (rustfmt.toml sets the width). Prose in comments,
docs and release notes uses plain sentences and no em dashes.

## Commits

Write the commit message in your own words. Do not add attribution trailers for
tools or assistants; the author line is the author.

## Reporting false positives

The most useful report is a false positive: use the issue template, include the
rule id, the finding id from `locrin check --json`, the snippet, and why it is
wrong.
```

`.github/ISSUE_TEMPLATE/config.yml`:
```yaml
blank_issues_enabled: true
contact_links:
  - name: Security report
    url: https://github.com/BilalEjaz/locrin/security/advisories/new
    about: Report a vulnerability privately.
```

`.github/ISSUE_TEMPLATE/false-positive.yml`:
```yaml
name: False positive
description: A finding that should not have fired. The report we most want.
labels: [false-positive]
body:
  - type: input
    id: rule
    attributes: { label: Rule id, placeholder: leftover-commented-code }
    validations: { required: true }
  - type: input
    id: finding
    attributes: { label: Finding id, description: From `locrin check --json`. }
  - type: input
    id: version
    attributes: { label: Locrin version, placeholder: locrin 0.5.0 }
    validations: { required: true }
  - type: textarea
    id: snippet
    attributes: { label: The code it fired on, render: text }
    validations: { required: true }
  - type: textarea
    id: why
    attributes: { label: Why it is wrong }
    validations: { required: true }
```

`.github/ISSUE_TEMPLATE/bug.yml`:
```yaml
name: Bug
description: A crash, a wrong verdict, a hook or Action problem.
labels: [bug]
body:
  - type: input
    id: version
    attributes: { label: Locrin version and platform, placeholder: locrin 0.5.0, macOS arm64 }
    validations: { required: true }
  - type: textarea
    id: steps
    attributes: { label: Steps to reproduce }
    validations: { required: true }
  - type: textarea
    id: expected
    attributes: { label: Expected and actual }
    validations: { required: true }
```

`.github/ISSUE_TEMPLATE/rule-request.yml`:
```yaml
name: Rule request
description: Something the engine should catch and does not.
labels: [rule-request]
body:
  - type: textarea
    id: pattern
    attributes: { label: The pattern, description: Code that should fire, and code that should not. }
    validations: { required: true }
  - type: input
    id: languages
    attributes: { label: Languages }
```

Delete `placeholders/` with `git rm -r placeholders` and remove its four lines from `.gitignore`.

- [ ] **Step 4: Run it green**

Run: `bash scripts/tests/run.sh`
Expected: all ok.

- [ ] **Step 5: Commit**

```bash
git add LICENSE SECURITY.md CONTRIBUTING.md .github/ISSUE_TEMPLATE .gitignore scripts/tests/public-files.test.sh
git commit -m "repo: licence, security policy, contributing guide, issue templates; placeholders retired"
```

---

### Task 6: npm packages

**Files:**
- Create: `npm/locrin/package.json`, `npm/locrin/bin/locrin.js`, `npm/locrin/lib/resolve.js`, `npm/locrin/README.md`
- Create: `npm/locrin/test/resolve.test.js`, `npm/locrin/test/launch.test.js`, `npm/locrin/test/stage.test.js`
- Create: `npm/platforms/linux-x64/package.json`, `npm/platforms/darwin-x64/package.json`, `npm/platforms/darwin-arm64/package.json`, `npm/platforms/win32-x64/package.json`
- Create: `npm/scripts/stage.js`

**Interfaces:**
- Produces: `node npm/scripts/stage.js --version <v> --binary <path> --platform <linux-x64|darwin-x64|darwin-arm64|win32-x64> [--binary ... --platform ...]` copies each binary into its platform package as `bin/locrin` (or `bin/locrin.exe`), sets `version` in every package.json (entry and platforms) and every `optionalDependencies` entry to `<v>`. Tasks 10 and 11 call it. The entry package's `bin/locrin.js` exports `run(argv, opts) -> number` (the exit code).

- [ ] **Step 1: Write the failing tests**

`npm/locrin/test/resolve.test.js`:
```js
const test = require("node:test");
const assert = require("node:assert/strict");
const { packageFor, binaryPath, PLATFORMS } = require("../lib/resolve");

test("maps every supported platform to its package", () => {
  assert.equal(packageFor("linux", "x64"), "@raxbi/locrin-linux-x64");
  assert.equal(packageFor("darwin", "x64"), "@raxbi/locrin-darwin-x64");
  assert.equal(packageFor("darwin", "arm64"), "@raxbi/locrin-darwin-arm64");
  assert.equal(packageFor("win32", "x64"), "@raxbi/locrin-win32-x64");
  assert.equal(Object.keys(PLATFORMS).length, 4);
});

test("names the platform when there is no prebuilt binary", () => {
  assert.throws(() => packageFor("freebsd", "x64"), (e) => e.code === "LOCRIN_UNSUPPORTED" && /freebsd-x64/.test(e.message) && /install/.test(e.message));
});

test("resolves the binary inside the platform package", () => {
  const seen = [];
  const p = binaryPath("linux", "x64", (spec) => { seen.push(spec); return "/abs/" + spec; });
  assert.equal(p, "/abs/@raxbi/locrin-linux-x64/bin/locrin");
  const w = binaryPath("win32", "x64", (spec) => "/abs/" + spec);
  assert.equal(w, "/abs/@raxbi/locrin-win32-x64/bin/locrin.exe");
  assert.deepEqual(seen, ["@raxbi/locrin-linux-x64/bin/locrin"]);
});

test("explains a missing platform package", () => {
  assert.throws(() => binaryPath("linux", "x64", () => { throw new Error("Cannot find module"); }),
    (e) => e.code === "LOCRIN_MISSING_PACKAGE" && /@raxbi\/locrin-linux-x64/.test(e.message) && /optional/i.test(e.message));
});
```

`npm/locrin/test/launch.test.js`:
```js
const test = require("node:test");
const assert = require("node:assert/strict");
const { run } = require("../bin/locrin");

const node = () => process.execPath;
const quiet = { write() {} };

test("forwards exit codes 0, 1 and 2", () => {
  assert.equal(run(["-e", "process.exit(0)"], { binaryPath: node, stderr: quiet }), 0);
  assert.equal(run(["-e", "process.exit(1)"], { binaryPath: node, stderr: quiet }), 1);
  assert.equal(run(["-e", "process.exit(2)"], { binaryPath: node, stderr: quiet }), 2);
});

test("unsupported platform exits 2 with the message", () => {
  let msg = "";
  const err = new Error("locrin has no prebuilt binary for plan9-mips"); err.code = "LOCRIN_UNSUPPORTED";
  const code = run([], { binaryPath: () => { throw err; }, stderr: { write(s) { msg += s; } } });
  assert.equal(code, 2);
  assert.match(msg, /plan9-mips/);
});

test("a binary that cannot start exits 2", () => {
  let msg = "";
  const code = run([], { binaryPath: () => "/definitely/not/here/locrin", stderr: { write(s) { msg += s; } } });
  assert.equal(code, 2);
  assert.match(msg, /could not start/);
});
```

`npm/locrin/test/stage.test.js`:
```js
const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { execFileSync } = require("node:child_process");

test("stage stamps versions and copies binaries", () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "locrin-stage-"));
  fs.cpSync(path.join(__dirname, "..", ".."), root, { recursive: true, filter: (p) => !p.includes("node_modules") });
  const bin = path.join(root, "fake-locrin"); fs.writeFileSync(bin, "#!/bin/sh\necho locrin 9.9.9\n");
  const exe = path.join(root, "fake-locrin.exe"); fs.writeFileSync(exe, "MZ");
  execFileSync(process.execPath, [path.join(root, "scripts", "stage.js"), "--version", "9.9.9",
    "--binary", bin, "--platform", "linux-x64", "--binary", exe, "--platform", "win32-x64"], { cwd: root });
  const entry = JSON.parse(fs.readFileSync(path.join(root, "locrin", "package.json"), "utf8"));
  assert.equal(entry.version, "9.9.9");
  for (const v of Object.values(entry.optionalDependencies)) assert.equal(v, "9.9.9");
  const linux = JSON.parse(fs.readFileSync(path.join(root, "platforms", "linux-x64", "package.json"), "utf8"));
  assert.equal(linux.version, "9.9.9");
  assert.ok(fs.existsSync(path.join(root, "platforms", "linux-x64", "bin", "locrin")));
  assert.ok(fs.existsSync(path.join(root, "platforms", "win32-x64", "bin", "locrin.exe")));
  if (process.platform !== "win32") assert.ok(fs.statSync(path.join(root, "platforms", "linux-x64", "bin", "locrin")).mode & 0o111);
});

test("stage refuses an unknown platform", () => {
  assert.throws(() => execFileSync(process.execPath, [path.join(__dirname, "..", "..", "scripts", "stage.js"),
    "--version", "1.0.0", "--binary", __filename, "--platform", "beos-ppc"], { stdio: "pipe" }));
});
```

- [ ] **Step 2: Run them red**

Run: `node --test npm/locrin/test`
Expected: failures (modules missing).

- [ ] **Step 3: Write the packages**

`npm/locrin/package.json`:
```json
{
  "name": "locrin",
  "version": "0.0.0",
  "description": "Locrin: the deterministic quality gate for code written by people and agents.",
  "license": "MIT",
  "homepage": "https://locrin.com",
  "repository": { "type": "git", "url": "https://github.com/BilalEjaz/locrin" },
  "bin": { "locrin": "bin/locrin.js" },
  "files": ["bin", "lib", "README.md"],
  "engines": { "node": ">=18" },
  "optionalDependencies": {
    "@raxbi/locrin-linux-x64": "0.0.0",
    "@raxbi/locrin-darwin-x64": "0.0.0",
    "@raxbi/locrin-darwin-arm64": "0.0.0",
    "@raxbi/locrin-win32-x64": "0.0.0"
  }
}
```

`npm/locrin/lib/resolve.js`:
```js
"use strict";
const PLATFORMS = {
  "linux-x64": "@raxbi/locrin-linux-x64",
  "darwin-x64": "@raxbi/locrin-darwin-x64",
  "darwin-arm64": "@raxbi/locrin-darwin-arm64",
  "win32-x64": "@raxbi/locrin-win32-x64",
};
const INSTALL = "https://github.com/BilalEjaz/locrin#install";

function packageFor(platform, arch) {
  const key = `${platform}-${arch}`;
  const name = PLATFORMS[key];
  if (!name) {
    const err = new Error(`locrin has no prebuilt binary for ${key}. See ${INSTALL} for the installer and the release page.`);
    err.code = "LOCRIN_UNSUPPORTED";
    throw err;
  }
  return name;
}

function binaryPath(platform = process.platform, arch = process.arch, resolve = require.resolve) {
  const name = packageFor(platform, arch);
  const file = platform === "win32" ? "locrin.exe" : "locrin";
  try {
    return resolve(`${name}/bin/${file}`);
  } catch (e) {
    const err = new Error(`locrin: the platform package ${name} is not installed. Optional dependencies may be disabled, or the lockfile predates this platform. Reinstall with optional dependencies enabled, or see ${INSTALL}.`);
    err.code = "LOCRIN_MISSING_PACKAGE";
    throw err;
  }
}

module.exports = { PLATFORMS, packageFor, binaryPath };
```

`npm/locrin/bin/locrin.js`:
```js
#!/usr/bin/env node
"use strict";
const { spawnSync } = require("node:child_process");
const { binaryPath } = require("../lib/resolve");

// Runs the platform binary with the same arguments and returns its exit code.
// Exit codes are the contract: 0 pass or advisory, 1 block, 2 engine error.
function run(argv, opts = {}) {
  const stderr = opts.stderr || process.stderr;
  const resolveBinary = opts.binaryPath || binaryPath;
  let bin;
  try {
    bin = resolveBinary();
  } catch (e) {
    stderr.write(`${e.message}\n`);
    return 2;
  }
  const r = spawnSync(bin, argv, { stdio: "inherit", windowsHide: true });
  if (r.error) {
    stderr.write(`locrin: could not start ${bin}: ${r.error.message}\n`);
    return 2;
  }
  return r.status === null ? 2 : r.status;
}

module.exports = { run };
if (require.main === module) process.exit(run(process.argv.slice(2)));
```

`npm/platforms/linux-x64/package.json` (and the other three with their own name, os and cpu):
```json
{
  "name": "@raxbi/locrin-linux-x64",
  "version": "0.0.0",
  "description": "Locrin binary for Linux x64. Install the `locrin` package instead of this one.",
  "license": "MIT",
  "repository": { "type": "git", "url": "https://github.com/BilalEjaz/locrin" },
  "os": ["linux"],
  "cpu": ["x64"],
  "files": ["bin"]
}
```
darwin-x64: `"os": ["darwin"], "cpu": ["x64"]`; darwin-arm64: `"os": ["darwin"], "cpu": ["arm64"]`; win32-x64: `"os": ["win32"], "cpu": ["x64"]`.

`npm/scripts/stage.js`:
```js
#!/usr/bin/env node
"use strict";
// Stamps the version into every package.json under npm/ and copies binaries into
// the platform packages. Usage:
//   node npm/scripts/stage.js --version 0.5.0 --binary <path> --platform linux-x64 [--binary ... --platform ...]
const fs = require("node:fs");
const path = require("node:path");

const ROOT = path.join(__dirname, "..");
const PLATFORMS = ["linux-x64", "darwin-x64", "darwin-arm64", "win32-x64"];

function parse(argv) {
  const out = { version: null, pairs: [] };
  let pendingBinary = null;
  for (let i = 0; i < argv.length; i += 2) {
    const [flag, value] = [argv[i], argv[i + 1]];
    if (value === undefined) throw new Error(`missing value for ${flag}`);
    if (flag === "--version") out.version = value;
    else if (flag === "--binary") pendingBinary = value;
    else if (flag === "--platform") {
      if (!PLATFORMS.includes(value)) throw new Error(`unknown platform ${value}; expected one of ${PLATFORMS.join(", ")}`);
      if (!pendingBinary) throw new Error("--platform must follow --binary");
      out.pairs.push({ binary: pendingBinary, platform: value });
      pendingBinary = null;
    } else throw new Error(`unknown flag ${flag}`);
  }
  if (!out.version) throw new Error("--version is required");
  return out;
}

function stampJson(file, version) {
  const pkg = JSON.parse(fs.readFileSync(file, "utf8"));
  pkg.version = version;
  if (pkg.optionalDependencies) for (const k of Object.keys(pkg.optionalDependencies)) pkg.optionalDependencies[k] = version;
  fs.writeFileSync(file, JSON.stringify(pkg, null, 2) + "\n");
}

function main() {
  const { version, pairs } = parse(process.argv.slice(2));
  stampJson(path.join(ROOT, "locrin", "package.json"), version);
  for (const p of PLATFORMS) stampJson(path.join(ROOT, "platforms", p, "package.json"), version);
  for (const { binary, platform } of pairs) {
    const dir = path.join(ROOT, "platforms", platform, "bin");
    fs.mkdirSync(dir, { recursive: true });
    const dest = path.join(dir, platform.startsWith("win32") ? "locrin.exe" : "locrin");
    fs.copyFileSync(binary, dest);
    if (process.platform !== "win32") fs.chmodSync(dest, 0o755);
  }
  console.log(`staged ${version}: ${pairs.map((p) => p.platform).join(", ") || "no binaries"}`);
}

main();
```

`npm/locrin/README.md`: fifteen lines: what Locrin is (two sentences), `npm i -D locrin` and `npx locrin init`, the four supported platforms, the link to the repository README for everything else.

Add `npm/platforms/*/bin/` to `.gitignore` (staged binaries are never committed).

- [ ] **Step 4: Run them green**

Run: `node --test npm/locrin/test`
Expected: all pass. Then `npm pack --dry-run npm/locrin` lists exactly `bin/locrin.js`, `lib/resolve.js`, `README.md`, `package.json`.

- [ ] **Step 5: Commit**

```bash
git add npm .gitignore
git commit -m "npm: locrin entry package with per-platform binary packages and a staging script"
```

---

### Task 7: PyPI wheels

**Files:**
- Create: `pypi/build_wheel.py`, `pypi/locrin/__init__.py`, `pypi/locrin/__main__.py`, `pypi/README.md`, `pypi/test_build_wheel.py`

**Interfaces:**
- Produces: `python pypi/build_wheel.py --version <v> --binary <path> --platform-tag <tag> --out <dir>` writes `<dir>/locrin-<v>-py3-none-<tag>.whl`. Platform tags used by the pipeline: `manylinux_2_35_x86_64` (the Linux binary is built on Ubuntu 22.04, glibc 2.35), `macosx_10_12_x86_64`, `macosx_11_0_arm64`, `win_amd64`. Tasks 10 and 11 call it.

- [ ] **Step 1: Write the failing test**

`pypi/test_build_wheel.py`:
```python
import os, subprocess, sys, tempfile, unittest, zipfile

HERE = os.path.dirname(os.path.abspath(__file__))

class BuildWheel(unittest.TestCase):
    def build(self, tag, exe=False):
        tmp = tempfile.mkdtemp()
        binary = os.path.join(tmp, "locrin.exe" if exe else "locrin")
        with open(binary, "wb") as f:
            f.write(b"#!/bin/sh\necho locrin 9.9.9\n")
        subprocess.run([sys.executable, os.path.join(HERE, "build_wheel.py"), "--version", "9.9.9",
                        "--binary", binary, "--platform-tag", tag, "--out", tmp], check=True)
        return os.path.join(tmp, f"locrin-9.9.9-py3-none-{tag}.whl")

    def test_layout_and_metadata(self):
        whl = self.build("manylinux_2_35_x86_64")
        self.assertTrue(os.path.exists(whl))
        with zipfile.ZipFile(whl) as z:
            names = set(z.namelist())
            self.assertEqual(names, {
                "locrin/__init__.py", "locrin/__main__.py", "locrin/bin/locrin",
                "locrin-9.9.9.dist-info/METADATA", "locrin-9.9.9.dist-info/WHEEL",
                "locrin-9.9.9.dist-info/entry_points.txt", "locrin-9.9.9.dist-info/RECORD",
            })
            meta = z.read("locrin-9.9.9.dist-info/METADATA").decode()
            self.assertIn("Name: locrin\n", meta)
            self.assertIn("Version: 9.9.9\n", meta)
            self.assertIn("Requires-Python: >=3.9\n", meta)
            wheel = z.read("locrin-9.9.9.dist-info/WHEEL").decode()
            self.assertIn("Root-Is-Purelib: false\n", wheel)
            self.assertIn("Tag: py3-none-manylinux_2_35_x86_64\n", wheel)
            ep = z.read("locrin-9.9.9.dist-info/entry_points.txt").decode()
            self.assertIn("locrin = locrin.__main__:main", ep)
            record = z.read("locrin-9.9.9.dist-info/RECORD").decode().splitlines()
            self.assertEqual(len(record), 7)
            self.assertIn("locrin-9.9.9.dist-info/RECORD,,", record)
            self.assertTrue(any(l.startswith("locrin/bin/locrin,sha256=") for l in record))
            self.assertEqual((z.getinfo("locrin/bin/locrin").external_attr >> 16) & 0o111, 0o111)

    def test_windows_binary_name(self):
        whl = self.build("win_amd64", exe=True)
        with zipfile.ZipFile(whl) as z:
            self.assertIn("locrin/bin/locrin.exe", z.namelist())

    def test_main_locates_binary(self):
        sys.path.insert(0, HERE)
        import locrin.__main__ as m
        self.assertTrue(m.binary().endswith(os.path.join("bin", "locrin.exe" if sys.platform == "win32" else "locrin")))

if __name__ == "__main__":
    unittest.main()
```

- [ ] **Step 2: Run it red**

Run: `python -m unittest discover -s pypi -p "test_*.py" -v`
Expected: errors (build_wheel.py missing).

- [ ] **Step 3: Write the package and the builder**

`pypi/locrin/__init__.py`:
```python
"""Locrin: the deterministic quality gate for code written by people and agents.

This package carries the prebuilt binary; `locrin` on the command line runs it.
"""
```

`pypi/locrin/__main__.py`:
```python
import os
import subprocess
import sys


def binary():
    name = "locrin.exe" if sys.platform == "win32" else "locrin"
    return os.path.join(os.path.dirname(os.path.abspath(__file__)), "bin", name)


def main():
    path = binary()
    if not os.path.exists(path):
        sys.stderr.write("locrin: the binary is missing from this installation; reinstall with pip, or see https://github.com/BilalEjaz/locrin#install\n")
        return 2
    args = [path] + sys.argv[1:]
    if sys.platform == "win32":
        return subprocess.call(args)
    os.execv(path, args)


if __name__ == "__main__":
    sys.exit(main())
```

`pypi/build_wheel.py`:
```python
"""Build a platform wheel that carries the locrin binary.

Usage: python pypi/build_wheel.py --version 0.5.0 --binary path/to/locrin --platform-tag manylinux_2_35_x86_64 --out dist

No setuptools: a wheel is a zip with a dist-info directory, and writing it
directly keeps the build deterministic and dependency-free.
"""
import argparse
import base64
import hashlib
import os
import zipfile

HERE = os.path.dirname(os.path.abspath(__file__))
SUMMARY = "Locrin: the deterministic quality gate for code written by people and agents."


def record_line(name, data):
    digest = base64.urlsafe_b64encode(hashlib.sha256(data).digest()).rstrip(b"=").decode()
    return f"{name},sha256={digest},{len(data)}"


def metadata(version):
    with open(os.path.join(HERE, "README.md"), encoding="utf-8") as f:
        readme = f.read()
    return (
        "Metadata-Version: 2.1\n"
        "Name: locrin\n"
        f"Version: {version}\n"
        f"Summary: {SUMMARY}\n"
        "Home-page: https://locrin.com\n"
        "License: MIT\n"
        "Requires-Python: >=3.9\n"
        "Project-URL: Source, https://github.com/BilalEjaz/locrin\n"
        "Description-Content-Type: text/markdown\n"
        "\n" + readme
    )


def build(version, binary, platform_tag, out):
    tag = f"py3-none-{platform_tag}"
    dist_info = f"locrin-{version}.dist-info"
    exe = platform_tag.startswith("win")
    bin_name = "locrin.exe" if exe else "locrin"
    files = []
    for py in ("__init__.py", "__main__.py"):
        with open(os.path.join(HERE, "locrin", py), "rb") as f:
            files.append((f"locrin/{py}", f.read(), 0o644))
    with open(binary, "rb") as f:
        files.append((f"locrin/bin/{bin_name}", f.read(), 0o755))
    files.append((f"{dist_info}/METADATA", metadata(version).encode(), 0o644))
    files.append((f"{dist_info}/WHEEL", f"Wheel-Version: 1.0\nGenerator: locrin-build\nRoot-Is-Purelib: false\nTag: {tag}\n".encode(), 0o644))
    files.append((f"{dist_info}/entry_points.txt", b"[console_scripts]\nlocrin = locrin.__main__:main\n", 0o644))
    record = "\n".join(record_line(n, d) for n, d, _ in files) + f"\n{dist_info}/RECORD,,\n"
    files.append((f"{dist_info}/RECORD", record.encode(), 0o644))

    os.makedirs(out, exist_ok=True)
    path = os.path.join(out, f"locrin-{version}-{tag}.whl")
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as z:
        for name, data, mode in files:
            info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            info.external_attr = (0o100000 | mode) << 16
            info.compress_type = zipfile.ZIP_DEFLATED
            z.writestr(info, data)
    return path


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--version", required=True)
    p.add_argument("--binary", required=True)
    p.add_argument("--platform-tag", required=True)
    p.add_argument("--out", required=True)
    a = p.parse_args()
    print(build(a.version, a.binary, a.platform_tag, a.out))


if __name__ == "__main__":
    main()
```

`pypi/README.md`: the same fifteen-line shape as the npm README with `pip install locrin` and `pipx install locrin`.

- [ ] **Step 4: Run it green**

Run: `python -m unittest discover -s pypi -p "test_*.py" -v`
Expected: 3 tests pass. Then install the wheel for real once on this machine:
```bash
python pypi/build_wheel.py --version 0.4.0 --binary "$(command -v locrin)" --platform-tag win_amd64 --out /tmp/whl
python -m venv /tmp/venv && /tmp/venv/Scripts/pip install /tmp/whl/locrin-0.4.0-py3-none-win_amd64.whl && /tmp/venv/Scripts/locrin --version
```
Expected: `locrin 0.4.0`. (On this Windows box `command -v locrin` is the cargo-installed exe.)

- [ ] **Step 5: Commit**

```bash
git add pypi
git commit -m "pypi: platform wheels carrying the binary, built without setuptools"
```

---

### Task 8: Homebrew formula generator

**Files:**
- Create: `scripts/homebrew-formula.sh`, `scripts/tests/homebrew-formula.test.sh`, `scripts/tests/fixtures/SHA256SUMS`

**Interfaces:**
- Produces: `scripts/homebrew-formula.sh <version-without-v> <SHA256SUMS path>` prints a complete formula to stdout. Task 11 writes it to `Formula/locrin.rb` in the tap repo.

- [ ] **Step 1: Write the fixture and the failing test**

`scripts/tests/fixtures/SHA256SUMS` (hashes are arbitrary hex, 64 chars each):
```
1111111111111111111111111111111111111111111111111111111111111111  locrin-v9.9.9-aarch64-apple-darwin.tar.gz
2222222222222222222222222222222222222222222222222222222222222222  locrin-v9.9.9-x86_64-apple-darwin.tar.gz
3333333333333333333333333333333333333333333333333333333333333333  locrin-v9.9.9-x86_64-pc-windows-msvc.zip
4444444444444444444444444444444444444444444444444444444444444444  locrin-v9.9.9-x86_64-unknown-linux-gnu.tar.gz
```

`scripts/tests/homebrew-formula.test.sh`:
```bash
#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
out=$(bash scripts/homebrew-formula.sh 9.9.9 scripts/tests/fixtures/SHA256SUMS)
need() { grep -qF -- "$1" <<<"$out" || { echo "missing: $1"; echo "$out"; exit 1; }; }
need 'class Locrin < Formula'
need 'version "9.9.9"'
need 'license "MIT"'
need 'url "https://github.com/BilalEjaz/locrin/releases/download/v9.9.9/locrin-v9.9.9-aarch64-apple-darwin.tar.gz"'
need 'sha256 "1111111111111111111111111111111111111111111111111111111111111111"'
need 'url "https://github.com/BilalEjaz/locrin/releases/download/v9.9.9/locrin-v9.9.9-x86_64-apple-darwin.tar.gz"'
need 'sha256 "2222222222222222222222222222222222222222222222222222222222222222"'
need 'url "https://github.com/BilalEjaz/locrin/releases/download/v9.9.9/locrin-v9.9.9-x86_64-unknown-linux-gnu.tar.gz"'
need 'sha256 "4444444444444444444444444444444444444444444444444444444444444444"'
need 'bin.install'
need 'locrin --version'
if grep -q 3333 <<<"$out"; then echo "windows asset must not appear"; exit 1; fi
if command -v ruby >/dev/null; then ruby -c <(echo "$out") >/dev/null; fi
if bash scripts/homebrew-formula.sh 9.9.9 /dev/null 2>/dev/null; then echo "missing checksum must fail"; exit 1; fi
```

- [ ] **Step 2: Run it red**

Run: `bash scripts/tests/run.sh`
Expected: `FAIL scripts/tests/homebrew-formula.test.sh`.

- [ ] **Step 3: Write the generator**

`scripts/homebrew-formula.sh`:
```bash
#!/usr/bin/env bash
# Prints the Homebrew formula for one release.
# Usage: scripts/homebrew-formula.sh <version-without-v> <SHA256SUMS>
set -euo pipefail
version="$1"; sums="$2"
base="https://github.com/BilalEjaz/locrin/releases/download/v$version"
sha() {
  local s
  s=$(grep " locrin-v$version-$1.tar.gz\$" "$sums" | awk '{print $1}')
  [[ ${#s} -eq 64 ]] || { echo "homebrew-formula: no checksum for $1 in $sums" >&2; exit 2; }
  echo "$s"
}
arm=$(sha aarch64-apple-darwin); intel=$(sha x86_64-apple-darwin); linux=$(sha x86_64-unknown-linux-gnu)
cat <<EOF
class Locrin < Formula
  desc "Deterministic quality gate for code written by people and agents"
  homepage "https://locrin.com"
  version "$version"
  license "MIT"

  on_macos do
    on_arm do
      url "$base/locrin-v$version-aarch64-apple-darwin.tar.gz"
      sha256 "$arm"
    end
    on_intel do
      url "$base/locrin-v$version-x86_64-apple-darwin.tar.gz"
      sha256 "$intel"
    end
  end

  on_linux do
    on_intel do
      url "$base/locrin-v$version-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "$linux"
    end
  end

  def install
    # The tarball holds one directory with the binary inside; find it either way.
    bin.install Dir["**/locrin"].first
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/locrin --version")
  end
end
EOF
```

- [ ] **Step 4: Run it green**

Run: `bash scripts/tests/run.sh`
Expected: ok.

- [ ] **Step 5: Commit**

```bash
git add scripts/homebrew-formula.sh scripts/tests/homebrew-formula.test.sh scripts/tests/fixtures/SHA256SUMS
git commit -m "brew: formula generator from the release checksums"
```

---

### Task 9: The curl and PowerShell installers

**Files:**
- Create: `install.sh`, `install.ps1`
- Create: `scripts/tests/install.test.sh`, `scripts/tests/install.test.ps1`

**Interfaces:**
- Produces: `install.sh [version]` and `install.ps1 [-Version v] [-InstallDir d] [-BaseUrl u]`. Both read `LOCRIN_BASE_URL` (sh) or `-BaseUrl` (ps1) so tests can point them at a local directory laid out as `<base>/<tag>/<asset>` and `<base>/<tag>/SHA256SUMS`. Exit 2 on unsupported platform, missing checksum line, checksum mismatch, or unresolvable "latest".

- [ ] **Step 1: Write the failing tests**

`scripts/tests/install.test.sh`:
```bash
#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
case "$(uname -s)-$(uname -m)" in
  Linux-x86_64) target=x86_64-unknown-linux-gnu ;;
  Darwin-arm64) target=aarch64-apple-darwin ;;
  Darwin-x86_64) target=x86_64-apple-darwin ;;
  *) echo "skip: install.sh has no build for this platform"; exit 0 ;;
esac
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
rel="$tmp/releases/v9.9.9"; mkdir -p "$rel" "$tmp/bin"
printf '#!/bin/sh\necho locrin 9.9.9\n' > "$tmp/bin/locrin"; chmod +x "$tmp/bin/locrin"
bash scripts/package.sh "$target" v9.9.9 "$tmp/bin" "$rel" >/dev/null
(cd "$rel" && if command -v sha256sum >/dev/null; then sha256sum locrin-* > SHA256SUMS; else shasum -a 256 locrin-* > SHA256SUMS; fi)

LOCRIN_BASE_URL="file://$tmp/releases" LOCRIN_INSTALL_DIR="$tmp/out" bash install.sh v9.9.9
[[ "$("$tmp/out/locrin" --version)" == "locrin 9.9.9" ]] || { echo "installed binary wrong"; exit 1; }

sed -i.bak 's/^./0/' "$rel/SHA256SUMS"
if LOCRIN_BASE_URL="file://$tmp/releases" LOCRIN_INSTALL_DIR="$tmp/out2" bash install.sh v9.9.9 2>"$tmp/err"; then echo "mismatch must fail"; exit 1; fi
grep -q 'checksum mismatch' "$tmp/err" || { cat "$tmp/err"; exit 1; }
[[ ! -e "$tmp/out2/locrin" ]] || { echo "must not install on mismatch"; exit 1; }

: > "$rel/SHA256SUMS"
if LOCRIN_BASE_URL="file://$tmp/releases" LOCRIN_INSTALL_DIR="$tmp/out3" bash install.sh v9.9.9 2>"$tmp/err"; then echo "missing line must fail"; exit 1; fi
grep -q 'not listed' "$tmp/err" || { cat "$tmp/err"; exit 1; }
```

`scripts/tests/install.test.ps1`:
```powershell
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..\..")
$tmp = Join-Path $env:TEMP ("locrin-install-test-" + [guid]::NewGuid())
$rel = Join-Path $tmp "releases\v9.9.9"
New-Item -ItemType Directory -Force -Path $rel | Out-Null
$dir = Join-Path $tmp "locrin-v9.9.9-x86_64-pc-windows-msvc"
New-Item -ItemType Directory -Force -Path $dir | Out-Null
Set-Content -Path (Join-Path $dir "locrin.exe") -Value "not really an exe" -Encoding ascii
Compress-Archive -Path $dir -DestinationPath (Join-Path $rel "locrin-v9.9.9-x86_64-pc-windows-msvc.zip")
$hash = (Get-FileHash -Algorithm SHA256 (Join-Path $rel "locrin-v9.9.9-x86_64-pc-windows-msvc.zip")).Hash.ToLower()
Set-Content -Path (Join-Path $rel "SHA256SUMS") -Value "$hash  locrin-v9.9.9-x86_64-pc-windows-msvc.zip" -Encoding ascii

$out = Join-Path $tmp "out"
powershell -NoProfile -ExecutionPolicy Bypass -File .\install.ps1 -Version v9.9.9 -BaseUrl (Join-Path $tmp "releases") -InstallDir $out
if ($LASTEXITCODE -ne 0) { throw "install failed with $LASTEXITCODE" }
if (-not (Test-Path (Join-Path $out "locrin.exe"))) { throw "binary not installed" }

Set-Content -Path (Join-Path $rel "SHA256SUMS") -Value ("0" + $hash.Substring(1) + "  locrin-v9.9.9-x86_64-pc-windows-msvc.zip") -Encoding ascii
$out2 = Join-Path $tmp "out2"
powershell -NoProfile -ExecutionPolicy Bypass -File .\install.ps1 -Version v9.9.9 -BaseUrl (Join-Path $tmp "releases") -InstallDir $out2
if ($LASTEXITCODE -ne 2) { throw "mismatch must exit 2, got $LASTEXITCODE" }
if (Test-Path (Join-Path $out2 "locrin.exe")) { throw "must not install on mismatch" }
Remove-Item -Recurse -Force $tmp
Write-Output "ok   scripts/tests/install.test.ps1"
```

- [ ] **Step 2: Run them red**

Run: `bash scripts/tests/run.sh` (on this Windows box the sh test prints skip, which is correct) and `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/tests/install.test.ps1`
Expected: the ps1 test throws (install.ps1 missing). The sh test is exercised by CI on Linux and macOS in Task 10.

- [ ] **Step 3: Write the installers**

`install.sh`:
```bash
#!/usr/bin/env bash
# Installs the locrin binary for this machine from a GitHub release.
#   curl -fsSL https://raw.githubusercontent.com/BilalEjaz/locrin/main/install.sh | bash
#   ... | bash -s v0.5.0            pin a version (default: latest)
# LOCRIN_INSTALL_DIR  where the binary goes (default: ~/.local/bin)
# LOCRIN_BASE_URL     asset base (default: the GitHub release download URL)
set -euo pipefail
REPO="BilalEjaz/locrin"
BASE_URL="${LOCRIN_BASE_URL:-https://github.com/$REPO/releases/download}"
VERSION="${1:-latest}"
INSTALL_DIR="${LOCRIN_INSTALL_DIR:-$HOME/.local/bin}"

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64) target=x86_64-unknown-linux-gnu ;;
  Darwin-arm64) target=aarch64-apple-darwin ;;
  Darwin-x86_64) target=x86_64-apple-darwin ;;
  *) echo "locrin: no prebuilt binary for $(uname -s) $(uname -m); see https://github.com/$REPO/releases" >&2; exit 2 ;;
esac

if [[ "$VERSION" == "latest" ]]; then
  VERSION=$(curl -fsSI -o /dev/null -w '%{redirect_url}' "https://github.com/$REPO/releases/latest" || true)
  VERSION="${VERSION##*/}"
  [[ "$VERSION" == v* ]] || { echo "locrin: could not resolve the latest release" >&2; exit 2; }
fi

asset="locrin-$VERSION-$target.tar.gz"
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
curl -fsSL "$BASE_URL/$VERSION/$asset" -o "$tmp/$asset"
curl -fsSL "$BASE_URL/$VERSION/SHA256SUMS" -o "$tmp/SHA256SUMS"

expected=$(grep " $asset\$" "$tmp/SHA256SUMS" | awk '{print $1}')
[[ -n "$expected" ]] || { echo "locrin: $asset is not listed in SHA256SUMS" >&2; exit 2; }
if command -v sha256sum >/dev/null; then actual=$(sha256sum "$tmp/$asset" | awk '{print $1}'); else actual=$(shasum -a 256 "$tmp/$asset" | awk '{print $1}'); fi
[[ "$actual" == "$expected" ]] || { echo "locrin: checksum mismatch for $asset" >&2; exit 2; }

tar -xzf "$tmp/$asset" -C "$tmp"
mkdir -p "$INSTALL_DIR"
install -m 755 "$tmp/locrin-$VERSION-$target/locrin" "$INSTALL_DIR/locrin.tmp"
mv -f "$INSTALL_DIR/locrin.tmp" "$INSTALL_DIR/locrin"
echo "locrin $VERSION installed to $INSTALL_DIR/locrin"
case ":$PATH:" in *":$INSTALL_DIR:"*) ;; *) echo "add $INSTALL_DIR to your PATH" ;; esac
```

`install.ps1`:
```powershell
# Installs the locrin binary on Windows from a GitHub release.
#   irm https://raw.githubusercontent.com/BilalEjaz/locrin/main/install.ps1 | iex
#   & ([scriptblock]::Create((irm .../install.ps1))) -Version v0.5.0
param(
  [string]$Version = "latest",
  [string]$InstallDir = "$env:LOCALAPPDATA\locrin\bin",
  [string]$BaseUrl = "https://github.com/BilalEjaz/locrin/releases/download"
)
$ErrorActionPreference = "Stop"
$repo = "BilalEjaz/locrin"
function Fail($msg) { [Console]::Error.WriteLine("locrin: $msg"); exit 2 }

if ($env:PROCESSOR_ARCHITECTURE -ne "AMD64") { Fail "no prebuilt binary for $env:PROCESSOR_ARCHITECTURE; see https://github.com/$repo/releases" }
$target = "x86_64-pc-windows-msvc"

if ($Version -eq "latest") {
  $location = $null
  try { Invoke-WebRequest -Uri "https://github.com/$repo/releases/latest" -MaximumRedirection 0 -UseBasicParsing | Out-Null }
  catch { if ($_.Exception.Response) { $location = $_.Exception.Response.Headers["Location"] } }
  if (-not $location) { Fail "could not resolve the latest release" }
  $Version = $location.Split("/")[-1]
}

$asset = "locrin-$Version-$target.zip"
$tmp = Join-Path $env:TEMP ("locrin-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $tmp | Out-Null
function Get-Asset($name, $dest) {
  $src = "$BaseUrl/$Version/$name"
  if ($src -like "http*") { Invoke-WebRequest -Uri $src -OutFile $dest -UseBasicParsing }
  else { Copy-Item -LiteralPath $src -Destination $dest }
}
Get-Asset $asset (Join-Path $tmp $asset)
Get-Asset "SHA256SUMS" (Join-Path $tmp "SHA256SUMS")

$line = Get-Content (Join-Path $tmp "SHA256SUMS") | Where-Object { $_ -match ("\s" + [regex]::Escape($asset) + "$") }
if (-not $line) { Fail "$asset is not listed in SHA256SUMS" }
$expected = (($line | Select-Object -First 1) -split "\s+")[0].ToLower()
$actual = (Get-FileHash -Algorithm SHA256 (Join-Path $tmp $asset)).Hash.ToLower()
if ($actual -ne $expected) { Fail "checksum mismatch for $asset" }

Expand-Archive -Path (Join-Path $tmp $asset) -DestinationPath $tmp -Force
New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
Move-Item -Force -Path (Join-Path $tmp "locrin-$Version-$target\locrin.exe") -Destination (Join-Path $InstallDir "locrin.exe")
Remove-Item -Recurse -Force $tmp
Write-Output "locrin $Version installed to $InstallDir\locrin.exe"
if (($env:PATH -split ";") -notcontains $InstallDir) { Write-Output "add $InstallDir to your PATH" }
```

- [ ] **Step 4: Run them green**

Run: `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/tests/install.test.ps1`
Expected: `ok   scripts/tests/install.test.ps1`. Run `bash scripts/tests/run.sh`: the sh test skips here and runs in CI (Task 10). Also run `bash -n install.sh` for a syntax check.

- [ ] **Step 5: Commit**

```bash
git add install.sh install.ps1 scripts/tests/install.test.sh scripts/tests/install.test.ps1
git commit -m "install: curl and PowerShell installers with checksum verification and tests"
```

---

### Task 10: CI install smoke on three operating systems

**Files:**
- Create: `scripts/ci/install-smoke.sh`
- Modify: `.github/workflows/ci.yml` (new job)

**Interfaces:**
- Consumes: `scripts/package.sh`, `npm/scripts/stage.js`, `pypi/build_wheel.py`, `scripts/homebrew-formula.sh`, `install.sh`, `install.ps1`.
- Produces: a CI job `install-smoke` that fails when any channel cannot be built from a fresh binary and installed on that OS.

- [ ] **Step 1: Write the job body**

`scripts/ci/install-smoke.sh`:
```bash
#!/usr/bin/env bash
# Builds every install channel from the release binary just built and installs
# each one on this runner. Run by .github/workflows/ci.yml install-smoke.
# Usage: scripts/ci/install-smoke.sh <target> <npm-platform> <pypi-tag>
set -euo pipefail
cd "$(dirname "$0")/../.."
target="$1"; npm_platform="$2"; pypi_tag="$3"
version=$(grep -m1 '^version' Cargo.toml | sed 's/.*"\(.*\)"/\1/')
tag="v$version"
bin=target/release/locrin; [[ "$target" == *windows* ]] && bin=target/release/locrin.exe
work=$(mktemp -d)

echo "== package"
asset=$(bash scripts/package.sh "$target" "$tag" target/release "$work/rel/$tag")
(cd "$work/rel/$tag" && if command -v sha256sum >/dev/null; then sha256sum locrin-* > SHA256SUMS; else shasum -a 256 locrin-* > SHA256SUMS; fi)

echo "== npm"
node npm/scripts/stage.js --version "$version" --binary "$bin" --platform "$npm_platform"
mkdir -p "$work/npm-proj" "$work/npm-tgz"
(cd "$work/npm-tgz" && npm pack "$OLDPWD/npm/platforms/$npm_platform" "$OLDPWD/npm/locrin" >/dev/null)
(cd "$work/npm-proj" && npm init -y >/dev/null && npm install --no-audit --no-fund "$work"/npm-tgz/*.tgz >/dev/null)
got=$("$work/npm-proj/node_modules/.bin/locrin" --version)
[[ "$got" == "locrin $version" ]] || { echo "npm launcher printed: $got"; exit 1; }
git checkout -- npm  # undo the version stamps

echo "== pypi"
whl=$(python pypi/build_wheel.py --version "$version" --binary "$bin" --platform-tag "$pypi_tag" --out "$work/whl")
python -m venv "$work/venv"
if [[ -d "$work/venv/Scripts" ]]; then vbin="$work/venv/Scripts"; else vbin="$work/venv/bin"; fi
"$vbin/pip" install --quiet "$whl"
got=$("$vbin/locrin" --version)
[[ "$got" == "locrin $version" ]] || { echo "pip launcher printed: $got"; exit 1; }

echo "== installers"
if [[ "$target" == *windows* ]]; then
  powershell -NoProfile -ExecutionPolicy Bypass -File scripts/tests/install.test.ps1
  powershell -NoProfile -ExecutionPolicy Bypass -File install.ps1 -Version "$tag" -BaseUrl "$(cygpath -w "$work/rel")" -InstallDir "$(cygpath -w "$work/inst")"
  got=$("$work/inst/locrin.exe" --version)
else
  bash scripts/tests/install.test.sh
  LOCRIN_BASE_URL="file://$work/rel" LOCRIN_INSTALL_DIR="$work/inst" bash install.sh "$tag"
  got=$("$work/inst/locrin" --version)
  bash scripts/homebrew-formula.sh "$version" "$work/rel/$tag/SHA256SUMS" > "$work/locrin.rb"
  if command -v ruby >/dev/null; then ruby -c "$work/locrin.rb"; fi
fi
[[ "$got" == "locrin $version" ]] || { echo "installer binary printed: $got"; exit 1; }
echo "install smoke passed for $target"
```
Note the Homebrew check runs only where the formula applies (Linux and macOS); the Linux SHA256SUMS lacks the two macOS lines, so on Linux the formula generator will fail on the missing checksum. Make the formula step macOS-only: replace the two formula lines with
```bash
  if [[ "$target" == *apple* ]]; then
    printf '%s  locrin-%s-x86_64-unknown-linux-gnu.tar.gz\n%s  locrin-%s-aarch64-apple-darwin.tar.gz\n%s  locrin-%s-x86_64-apple-darwin.tar.gz\n' \
      "$(printf '0%.0s' {1..64})" "$tag" "$(printf '0%.0s' {1..64})" "$tag" "$(printf '0%.0s' {1..64})" "$tag" >> "$work/rel/$tag/SHA256SUMS"
    bash scripts/homebrew-formula.sh "$version" "$work/rel/$tag/SHA256SUMS" > "$work/locrin.rb"
    ruby -c "$work/locrin.rb"
  fi
```
(the padding lines only feed the generator's lookup; the real formula is generated in the release job from the real checksums).

- [ ] **Step 2: Add the job**

Append to `.github/workflows/ci.yml`:
```yaml
  install-smoke:
    needs: lint
    strategy:
      fail-fast: false
      matrix:
        include:
          - os: ubuntu-22.04
            target: x86_64-unknown-linux-gnu
            npm: linux-x64
            pypi: manylinux_2_35_x86_64
          - os: macos-14
            target: aarch64-apple-darwin
            npm: darwin-arm64
            pypi: macosx_11_0_arm64
          - os: windows-2022
            target: x86_64-pc-windows-msvc
            npm: win32-x64
            pypi: win_amd64
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - uses: actions/setup-node@v4
        with:
          node-version: 20
      - uses: actions/setup-python@v5
        with:
          python-version: "3.12"
      - run: cargo build --release -p locrin
      - run: node --test npm/locrin/test
      - run: python -m unittest discover -s pypi -p "test_*.py"
      - shell: bash
        run: bash scripts/ci/install-smoke.sh '${{ matrix.target }}' '${{ matrix.npm }}' '${{ matrix.pypi }}'
```

- [ ] **Step 3: Run what can run locally**

Run: `bash -n scripts/ci/install-smoke.sh && node --test npm/locrin/test && python -m unittest discover -s pypi -p "test_*.py"`
Expected: syntax ok, tests pass. Then run the smoke body on this box against the installed binary to catch Windows path problems before CI:
```bash
export PATH="$USERPROFILE/.cargo/bin:$PATH"; cargo build --release -p locrin
bash scripts/ci/install-smoke.sh x86_64-pc-windows-msvc win32-x64 win_amd64
```
Expected: `install smoke passed for x86_64-pc-windows-msvc`. Fix the script until it does; `git status` must show `npm/` clean afterwards.

- [ ] **Step 4: Commit and push the branch so CI runs**

```bash
git add scripts/ci/install-smoke.sh .github/workflows/ci.yml
git commit -m "ci: install smoke for npm, PyPI, the installers and the formula on three operating systems"
git push -u origin launch/preflight-and-install
```
Open a draft pull request (`gh pr create --draft --title "Public launch A: pre-flight and install channels" --body "Draft; body filled in at the end."`) so the workflow runs. Wait for `install-smoke` on all three runners with `gh pr checks --watch`. Fix and push until green; record the run URL in the task report.

---

### Task 11: The publish stage of the release workflow

**Files:**
- Modify: `.github/workflows/release.yml`
- Create: `scripts/release/publish.sh`

**Interfaces:**
- Consumes: the four build artifacts and SHA256SUMS from the `build` job; `npm/scripts/stage.js`; `pypi/build_wheel.py`; `scripts/homebrew-formula.sh`.
- Produces: on a tag push, a GitHub release created as pre-release, then npm, PyPI, crates.io and the Homebrew tap published in that order, then the pre-release flag cleared. On `workflow_dispatch` with dry-run, the same staging with every publish command in its dry-run form and no release.
- Secrets the founder creates before the first real tag (not needed for the dry run): `NPM_TOKEN` (npm granular access token, publish, packages `locrin` and scope `@raxbi`, bypass two-factor for automation), `PYPI_TOKEN` (PyPI API token scoped to the `locrin` project), `CARGO_REGISTRY_TOKEN` (crates.io token with publish-new and publish-update scopes for the five crates (four of them have never been published)), `HOMEBREW_TAP_TOKEN` (fine-grained PAT, Contents read and write on `BilalEjaz/homebrew-locrin` only).

- [ ] **Step 1: Write the publish script**

`scripts/release/publish.sh`:
```bash
#!/usr/bin/env bash
# Stages and publishes every channel from the release assets.
# Usage: scripts/release/publish.sh <assets-dir> <dry|live>
# <assets-dir> holds the four archives and SHA256SUMS. Publishing order: npm
# platform packages, npm entry, PyPI wheels, crates in dependency order, the
# Homebrew tap. A failure stops the script; the caller leaves the release marked
# pre-release, and re-running after a fix completes it (every step is idempotent
# for an already-published version except crates.io, which is skipped when the
# version exists).
set -euo pipefail
assets="$1"; mode="$2"
cd "$(dirname "$0")/../.."
version=$(grep -m1 '^version' Cargo.toml | sed 's/.*"\(.*\)"/\1/')
tag="v$version"
work=$(mktemp -d)
extract() { # <target> -> prints the binary path
  local t="$1" d="$work/$1"; mkdir -p "$d"
  if [[ "$t" == *windows* ]]; then (cd "$d" && 7z x -y "$assets/locrin-$tag-$t.zip" >/dev/null); echo "$d/locrin-$tag-$t/locrin.exe"
  else tar -xzf "$assets/locrin-$tag-$t.tar.gz" -C "$d"; echo "$d/locrin-$tag-$t/locrin"; fi
}
linux=$(extract x86_64-unknown-linux-gnu); darm=$(extract aarch64-apple-darwin); dx64=$(extract x86_64-apple-darwin); win=$(extract x86_64-pc-windows-msvc)
if [[ "$mode" == "live" ]]; then dry=""; else dry="--dry-run"; fi

echo "== npm $version"
node npm/scripts/stage.js --version "$version" --binary "$linux" --platform linux-x64 --binary "$darm" --platform darwin-arm64 --binary "$dx64" --platform darwin-x64 --binary "$win" --platform win32-x64
for p in linux-x64 darwin-arm64 darwin-x64 win32-x64; do
  if [[ "$mode" == "live" ]] && npm view "@raxbi/locrin-$p@$version" version >/dev/null 2>&1; then echo "npm @raxbi/locrin-$p@$version exists, skipping"; continue; fi
  (cd "npm/platforms/$p" && npm publish --access public $dry)
done
if [[ "$mode" == "live" ]] && npm view "locrin@$version" version >/dev/null 2>&1; then echo "npm locrin@$version exists, skipping"; else (cd npm/locrin && npm publish --access public $dry); fi

echo "== pypi $version"
python pypi/build_wheel.py --version "$version" --binary "$linux" --platform-tag manylinux_2_35_x86_64 --out "$work/whl"
python pypi/build_wheel.py --version "$version" --binary "$darm" --platform-tag macosx_11_0_arm64 --out "$work/whl"
python pypi/build_wheel.py --version "$version" --binary "$dx64" --platform-tag macosx_10_12_x86_64 --out "$work/whl"
python pypi/build_wheel.py --version "$version" --binary "$win" --platform-tag win_amd64 --out "$work/whl"
python -m twine check "$work"/whl/*.whl
if [[ "$mode" == "live" ]]; then python -m twine upload --skip-existing "$work"/whl/*.whl; fi

echo "== crates $version"
for c in locrin-core locrin-rules locrin-reporters locrin-mcp locrin; do
  if [[ "$mode" == "live" ]]; then
    if cargo info "$c@$version" >/dev/null 2>&1; then echo "$c $version exists, skipping"; continue; fi
    cargo publish -p "$c" --no-verify
  elif [[ "$c" == "locrin-core" ]]; then
    cargo publish -p "$c" --dry-run
  else
    echo "dry run: $c depends on unpublished crates; packaging only"; cargo package -p "$c" --no-verify >/dev/null
  fi
done

echo "== homebrew $version"
bash scripts/homebrew-formula.sh "$version" "$assets/SHA256SUMS" > "$work/locrin.rb"
if [[ "$mode" == "live" ]]; then
  git clone --depth 1 "https://x-access-token:${HOMEBREW_TAP_TOKEN}@github.com/BilalEjaz/homebrew-locrin.git" "$work/tap"
  mkdir -p "$work/tap/Formula" && cp "$work/locrin.rb" "$work/tap/Formula/locrin.rb"
  (cd "$work/tap" && git -c user.name=locrin-release -c user.email=release@locrin.com add Formula/locrin.rb && (git -c user.name=locrin-release -c user.email=release@locrin.com commit -qm "locrin $version" || echo "formula unchanged") && git push -q)
else
  cat "$work/locrin.rb"
fi
echo "published $version ($mode)"
```
`cargo info` exists in cargo 1.82 and later; the runner's stable toolchain is newer. `cargo publish` waits for each crate to be available in the index before the next one publishes (cargo does this itself since 1.66), which is why the loop is sequential.

- [ ] **Step 2: Rewrite the publish jobs in release.yml**

Replace the `publish` job with these two jobs:
```yaml
  dry-run:
    needs: build
    if: ${{ github.event_name == 'workflow_dispatch' && inputs.dry-run == true }}
    runs-on: ubuntu-22.04
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: actions/setup-node@v4
        with:
          node-version: 20
      - uses: actions/setup-python@v5
        with:
          python-version: "3.12"
      - run: python -m pip install --quiet twine
      - uses: actions/download-artifact@v4
        with:
          path: assets
          merge-multiple: true
      - name: Checksums
        run: cd assets && sha256sum locrin-* > SHA256SUMS && cat SHA256SUMS
      - name: Stage every channel without publishing
        run: bash scripts/release/publish.sh "$PWD/assets" dry

  publish:
    needs: build
    if: ${{ github.ref_type == 'tag' && github.event_name == 'push' }}
    runs-on: ubuntu-22.04
    permissions:
      contents: write
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: actions/setup-node@v4
        with:
          node-version: 20
          registry-url: https://registry.npmjs.org
      - uses: actions/setup-python@v5
        with:
          python-version: "3.12"
      - run: python -m pip install --quiet twine
      - uses: actions/download-artifact@v4
        with:
          path: assets
          merge-multiple: true
      - name: Checksums
        run: cd assets && sha256sum locrin-* > SHA256SUMS && cat SHA256SUMS
      - name: Create the release as a pre-release
        env:
          GH_TOKEN: ${{ github.token }}
        run: |
          [[ "$GITHUB_REF" == refs/tags/* ]] || { echo "not a tag ref" >&2; exit 1; }
          tag="${GITHUB_REF#refs/tags/}"
          if gh release view "$tag" --repo "$GITHUB_REPOSITORY" >/dev/null 2>&1; then
            echo "release $tag exists; re-run completes the channels"
          else
            gh release create "$tag" assets/* --repo "$GITHUB_REPOSITORY" --title "locrin $tag" --generate-notes --prerelease
          fi
      - name: Publish every channel
        env:
          NODE_AUTH_TOKEN: ${{ secrets.NPM_TOKEN }}
          TWINE_USERNAME: __token__
          TWINE_PASSWORD: ${{ secrets.PYPI_TOKEN }}
          CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN }}
          HOMEBREW_TAP_TOKEN: ${{ secrets.HOMEBREW_TAP_TOKEN }}
        run: bash scripts/release/publish.sh "$PWD/assets" live
      - name: Clear the pre-release flag
        env:
          GH_TOKEN: ${{ github.token }}
        run: gh release edit "${GITHUB_REF#refs/tags/}" --repo "$GITHUB_REPOSITORY" --prerelease=false --latest
```
Keep the `build` job and the `workflow_dispatch` input as they are. The old `inputs.dry-run == false` dispatch path (publishing a real release from a dispatch) is removed: releases come from tags only.

- [ ] **Step 3: Run the dry run**

Run: `bash -n scripts/release/publish.sh`, commit, push, then `gh workflow run release.yml --ref launch/preflight-and-install -f dry-run=true` and `gh run watch` the run. (Dispatch works on a branch once the workflow file exists on the default branch; the current release.yml is on main, so the dispatch picks up the branch's version of the file.)
Expected: `dry-run` job green: four `npm publish --dry-run` listings, `twine check` PASSED for four wheels, `cargo publish --dry-run` for locrin-core, four `cargo package`, the formula printed. If the dispatch is refused because the branch is not the default, note it in the report; the dry run then happens right after the merge, before the flip, and the lead runs it.

- [ ] **Step 4: Commit**

```bash
git add scripts/release/publish.sh .github/workflows/release.yml
git commit -m "release: publish npm, PyPI, crates and the Homebrew tap from one run; dry-run job"
git push
```

---

### Task 12: Action and examples for a public repository

**Files:**
- Modify: `action/action.yml:32-37`, `action/README.md`, `action/examples/fastlift.yml`, `action/examples/pull-request.yml`, `action/examples/deploy-gate.yml`

- [ ] **Step 1: Update the inputs**

In `action/action.yml` change the two descriptions:
```yaml
  token:
    description: "Token for the comment and the release download"
    default: ${{ github.token }}
  download-token:
    description: "Optional token for the release download; only needed if the repository ever needs one. Defaults to token"
    default: ""
```
Keep the input so existing workflows that pass it keep working.

In `action/examples/fastlift.yml` delete the four header comment lines about the private repository and LOCRIN_TOKEN, and delete the `download-token:` line and its comment. The example becomes:
```yaml
# Copy to .github/workflows/locrin.yml. Requires locrin.toml and
# locrin-baseline.json committed, and fetch-depth 0 for --base.
name: locrin
on:
  pull_request:
permissions:
  contents: read
  pull-requests: write
  security-events: write
jobs:
  locrin:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0
      - uses: BilalEjaz/locrin/action@v0.4.0
        with:
          version: v0.4.0
```
In `action/README.md` remove any sentence about the repository being private, the Actions access setting, or `LOCRIN_TOKEN`; keep `download-token` in the inputs table with the new wording.

- [ ] **Step 2: Check and commit**

Run: `grep -rn -i "private\|LOCRIN_TOKEN\|access setting" action/` and expect no hits. Run `bash scripts/check-private-refs.sh`.

```bash
git add action/action.yml action/README.md action/examples/fastlift.yml
git commit -m "action: token wording and examples for a public repository"
```

---

### Task 13: README for a stranger

**Files:**
- Modify: `README.md` (lines 1-11 and the Install section at 52-62; add two sections; keep the rest)

- [ ] **Step 1: Rewrite the top**

Replace lines 1 to 11 with:
```markdown
# Locrin

Locrin is a deterministic quality gate for code written by people and by
agents. It answers one question about a change, pass, advisory or block, the
same way every time, in under a second, with no model in the loop.

It reads TypeScript, JavaScript, PHP and Python with tree-sitter, keeps a
SQLite index of the repository outside the working tree, and runs 21 rules
whose precision is measured before they ship. The same binary serves a person
on the command line, a git pre-commit hook, Claude Code's hooks, an MCP client
and a GitHub Action.

## Install

    npm i -D locrin            # or: npx locrin --version
    pip install locrin
    brew install BilalEjaz/locrin/locrin
    cargo install locrin
    curl -fsSL https://raw.githubusercontent.com/BilalEjaz/locrin/main/install.sh | bash
    irm https://raw.githubusercontent.com/BilalEjaz/locrin/main/install.ps1 | iex

Every channel ships the same binary from the same release, verified against the
release's `SHA256SUMS`. Linux x86_64, macOS (Intel and Apple silicon) and
Windows x86_64 are prebuilt; anything else builds from source with cargo.

## Thirty seconds

    locrin init          # writes locrin.toml, the baseline, and the hooks
    locrin check         # verdict on what changed; exit 1 blocks, 2 is an engine error

With Claude Code, `init` also wires the post-edit hook, so an agent hears about
a blocking finding before it moves on, and the stop hook, so a session cannot
end with a block outstanding. Without an agent, the pre-commit hook and the
GitHub Action give the same verdict.

## Free and paid

Everything in this repository is free and MIT licensed: the engine, all 21
rules including the three security rules, the hooks, the MCP server and the
Action. Nothing here needs an account or touches the network except the
dependency advisory lookup, which `--offline` turns off.

Paid, later and separate: a security pack with framework-specific checks and
compliance reports, unlocked offline by licence key, and a hosted layer for
history across runs and people. Neither will take back anything that shipped
free.

## Benchmark

Precision and recall per rule, measured on a public corpus anyone can rerun:
https://github.com/BilalEjaz/locrin-benchmark
```
Then delete the old `## Install` section (the `cargo install --path crates/cli` block, the release-binaries paragraph and the "phase two" sentence). Keep `## Limits worth knowing first` and everything after `## Quick start` as they are, except: in `## GitHub Action` remove any sentence about a private repository or `LOCRIN_TOKEN`; in `### Corpora and numbers` add one sentence pointing at the benchmark repository for the public numbers.

- [ ] **Step 2: Check and commit**

Run: `grep -n -i "phase two\|private\|LOCRIN_TOKEN" README.md` and expect no hits except a "private corpus" mention in the corpora section if it exists (that one is correct). Run `bash scripts/check-private-refs.sh`.

```bash
git add README.md
git commit -m "docs: README for a first-time reader; install channels, free and paid, benchmark"
```

---

### Task 14: Version 0.5.0 and release notes

**Files:**
- Modify: `Cargo.toml` (version), the four path dependency versions in `crates/*/Cargo.toml`, `Cargo.lock`
- Modify: `docs/RELEASE-NOTES.md`
- Modify: `action/examples/pull-request.yml`, `action/examples/deploy-gate.yml`, `action/examples/fastlift.yml`, `action/README.md` (pins `v0.4.0` to `v0.5.0`)

- [ ] **Step 1: Bump**

Set `version = "0.5.0"` in the root `Cargo.toml` and in every `version = "0.4.0"` path dependency. Run `cargo build` so `Cargo.lock` updates. Run `bash scripts/check-versions.sh`; it must pass. Replace `v0.4.0` with `v0.5.0` in the action examples and the action README.

- [ ] **Step 2: Release notes**

In `docs/RELEASE-NOTES.md`, rename `## Unreleased` to `## 0.5.0` and add above the existing 0.5.0 paragraphs:
```markdown
0.5.0 is the first public release. The repository is open under MIT, and the
binary installs from npm (`locrin`), PyPI (`locrin`), Homebrew
(`BilalEjaz/locrin/locrin`), crates.io (`locrin`), or the installer scripts
at the repository root. Every channel carries the same binary from the same
release, verified against `SHA256SUMS`. The `locrin-cli` crate is renamed
`locrin` so `cargo install locrin` works.

### What changes without opting in

Nothing in the verdicts. Rule ids, finding ids and exit codes are unchanged
from 0.4.0. The GitHub Action no longer needs a download token.
```
Add a fresh empty `## Unreleased` heading above it.

- [ ] **Step 3: Full check and commit**

Run:
```bash
export PATH="$USERPROFILE/.cargo/bin:$PATH"
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
bash scripts/tests/run.sh && bash scripts/check-versions.sh && bash scripts/check-private-refs.sh
node --test npm/locrin/test && python -m unittest discover -s pypi -p "test_*.py"
```
Expected: all green; `locrin --version` from `cargo run -q -p locrin -- --version` prints `locrin 0.5.0`.

```bash
git add Cargo.toml Cargo.lock crates/core/Cargo.toml crates/rules/Cargo.toml crates/reporters/Cargo.toml crates/mcp/Cargo.toml crates/cli/Cargo.toml docs/RELEASE-NOTES.md action/README.md action/examples/pull-request.yml action/examples/deploy-gate.yml action/examples/fastlift.yml
git commit -m "release: 0.5.0, the first public release"
git push
```

- [ ] **Step 4: Finish the pull request**

Mark the draft ready and fill the body: what changed per task, the pre-flight history scan command and exit code (Task 3), the private reference sweep result (Task 4), the CI install-smoke run URL (Task 10), the dry-run run URL or the note that it must run after merge (Task 11). Wait for all checks green.

---

## After this plan (not tasks; the lead runs these on the founder's word)

1. Merge the pull request. Run the release dry run from main if Task 11 could not: `gh workflow run release.yml -f dry-run=true`.
2. Founder creates the four secrets on BilalEjaz/locrin: `NPM_TOKEN` (granular token from the `raxbi` account, publish, packages `locrin` and scope `@raxbi`, bypass two-factor for automation), `PYPI_TOKEN` (project-scoped to `locrin`), `CARGO_REGISTRY_TOKEN` (publish-new and publish-update), `HOMEBREW_TAP_TOKEN` (fine-grained PAT, Contents read and write on `BilalEjaz/homebrew-locrin` only). The lead gives exact click paths at that point. The tap repository (step 3) must exist before the first tag.
3. Lead creates the empty public tap repository: `gh repo create BilalEjaz/homebrew-locrin --public --description "Homebrew tap for locrin"` with a one-line README.
4. Plan B (benchmark repository) lands and runs against 0.4.0.
5. Founder says "make it public". Lead runs `gh repo edit BilalEjaz/locrin --visibility public --accept-visibility-change-consequences`, enables private vulnerability reporting (`gh api -X PUT repos/BilalEjaz/locrin/private-vulnerability-reporting`), and resets the Actions access setting (`gh api -X PUT repos/BilalEjaz/locrin/actions/permissions/access -f access_level=none`).
6. Tag: `git tag v0.5.0 && git push origin v0.5.0`. Watch the release run; every channel must publish; then verify from a clean machine or container: `npx locrin@0.5.0 --version`, `pipx run locrin --version`, `brew install BilalEjaz/locrin/locrin`, `cargo install locrin`, the two installer one-liners.
7. FastLift: open a pull request on BilalEjaz/fastlift replacing `.github/workflows/locrin.yml` with the new example pinned to v0.5.0 (no download token). After it is green and merged, the founder deletes the `LOCRIN_TOKEN` secret and the personal access token.
8. Benchmark CI publishes results for 0.5.0 (plan B).

## Pre-flight record: history rewrite (2026-09-13)

The pre-flight scan on the original history was clean for secrets, but the private-reference sweep found three spike files quoting the founder's other repositories, and earlier plan revisions carrying the founder's local user path, still reachable in history. The founder chose to rewrite.

Commands run on the private repository (a mirror backup was taken first):

```bash
git filter-repo --force --invert-paths \
  --path spike/fingerprint/SPOTCHECK-50.md \
  --path spike/fingerprint/labels-2026-09-05 \
  --replace-text replace.txt   # user paths become <repo>, <home>, <local-appdata>
git push --force origin main
git push --force origin v0.3.0 v0.4.0
```

Result: 335 commits (was 336; the labels-only commit became empty and was dropped), main tree unchanged, zero matches for the private-reference patterns in any blob of any ref, all merged branches deleted from GitHub so only `main` remains. `scripts/scan-history.sh` re-run on the rewritten repository: exit 0 (308 commits by gitleaks, every hit an allowlisted fixture). `scripts/check-private-refs.sh`: exit 0.

Known residual: GitHub keeps commits reachable by SHA through cached views and the 24 closed pull requests until GitHub Support dereferences them or the repository is recreated. First changed commit reported by the rewrite: `13c3d82` (old) became `6da6c98` (new).
