# Public launch: design spec

Date: 2026-09-12
Status: APPROVED IN CONVERSATION, awaiting founder read of this file
Parent spec: docs/superpowers/specs/2026-09-05-agent-native-quality-gate-design.md (section 11, phase three)
Scope: the first phase-three sub-project. Licence-key packs, the hosted layer and the editor extension are separate sub-projects and get their own specs.

## 1. Decisions locked

| Decision | Choice | Why |
|---|---|---|
| Repository visibility | github.com/BilalEjaz/locrin goes public | Open core was decided in the parent spec. A private engine cannot be installed, tried or benchmarked by anyone outside. Distribution is the wedge. |
| Owner | Stays under BilalEjaz for now | GitHub redirects every link, clone and Action reference on a later transfer to an organisation, so the move is safe whenever it happens. Decided 2026-09-12; organisation deferred. |
| Licence | MIT | Matches what the crates and registry placeholders already declare and what Ruff, Biome and oxc ship. Copyleft or source-available licences cut against the strict-workplace audience. |
| Free line for security | The three current rules (injection-sink, secret-exposed, vulnerable-dependency) stay free | Free must be genuinely useful or the tool never gets installed. The paid pack adds depth, never removes what shipped free. |
| Install channels at launch | npm, PyPI, Homebrew tap, curl installer, cargo | All fed by the existing release pipeline; no new compilation. winget and Scoop deferred. |
| npm and PyPI shape | Platform packages, no post-install download | Many teams disable install scripts and mirror registries. The esbuild and Biome pattern needs neither network nor scripts. |
| Benchmark | Public, reproducible, on public repositories, in its own repo | Numbers nobody can rerun carry no weight with sceptical engineers. Head-to-head vendor comparison deferred. |
| First public version | 0.5.0 | The Unreleased notes already exist; the public flip is the release event. |

## 2. What stays closed

These never enter the public repo or the benchmark repo:

- The private corpus mined from the founder's repositories and its labels and precision reports. It keeps gating every engine pull request at 85 percent precision per rule and per rule-language pair.
- The extended security pack: framework-specific auth checks, the deeper sink catalogue for PHP and Python, and the compliance PDF (parent spec 8.4).
- The hosted layer (parent spec 8.3).
- Any token, secret or private path.

## 3. Pre-flight before the flip

Going public is a one-way door. Every item below is done and checked while the repository is still private.

### 3.1 History scan
- Locrin's own secret-exposed rule runs over every commit in the history, not just the working tree, using a script that checks out each tree into a temporary directory.
- A second, independent scanner, gitleaks, runs over the full history.
- Any hit means a history rewrite before the flip. Zero hits is recorded in the plan ledger with the command used.

### 3.2 Private reference sweep
- Grep the whole tree, including docs, plans, ledgers and comments, for local absolute paths, the founder's other repository names in contexts that reveal private detail, e-mail addresses, and personal data.
- Rewrite or remove hits. Plans and specs stay public: they are the honest record of how the tool was built.
- The FastLift baseline and config are not in this repo and never were; confirm by grep.

### 3.3 Files a public repo needs
- LICENSE: MIT, copyright Raxbi Ltd, 2026.
- SECURITY.md: private reporting address, response promise, supported versions (latest minor only).
- CONTRIBUTING.md: build, test, fmt and clippy commands; the precision gate and the rule that a new rule needs a labelled corpus sample before merge; no AI attribution trailers in commits.
- Issue templates: false positive (the report we most want, with rule id, finding id, snippet and why it is wrong), bug, rule request.
- README rewritten for a stranger: what it is in two sentences, one-line install for each channel, the hook loop in thirty seconds, the free versus paid line in plain words, the benchmark link, the limits stated in the parent spec section 2.

### 3.4 After the flip
- The Actions access setting on the repo returns to its default.
- The FastLift workflow drops the download-token input and pins the new version.
- The LOCRIN_TOKEN secret on BilalEjaz/fastlift and the fine-grained personal access token are deleted by the founder.
- The README of every registry placeholder is replaced by the real package.

## 4. Install channels

### 4.1 Release pipeline
The existing release.yml builds four targets (Linux x86_64, macOS Intel, macOS Apple silicon, Windows x86_64) and SHA256SUMS on every tag. A new publish job runs after all four builds succeed and publishes every channel from the same run. If any channel fails, the workflow fails, the GitHub release is marked pre-release, and no channel advertises a version another channel lacks. Re-running the job after a fix completes the release.

### 4.2 npm
- Packages: `locrin` (the entry point, already held by the founder's `raxbi` npm account) and four platform packages `@raxbi/locrin-linux-x64`, `@raxbi/locrin-darwin-x64`, `@raxbi/locrin-darwin-arm64`, `@raxbi/locrin-win32-x64`. The `@raxbi` scope belongs to the founder's npm account automatically, so no organisation is created. Users only ever type `locrin`; the platform packages are plumbing. Decided 2026-09-12.
- Each platform package carries the binary for its `os` and `cpu` fields. The entry package lists all four under `optionalDependencies`; npm installs only the matching one.
- The entry package's `bin` is a tiny Node launcher that resolves the platform package, executes the binary with the same arguments, and forwards stdin, stdout, stderr and the exit code unchanged, so exit codes 0, 1 and 2 keep their meaning.
- No `postinstall`, no download at install time, no network.
- Unsupported platform: the launcher prints one line naming the platform and pointing at the curl installer and the release page, then exits 2.

### 4.3 PyPI
- Package `locrin`, built as one wheel per platform tag (manylinux x86_64, macosx x86_64, macosx arm64, win_amd64) each carrying the binary, with a console entry point that executes it the same way as the npm launcher. No source distribution that compiles Rust; a plain `sdist` is not published, so `pip install locrin` on an unsupported platform fails at resolution with pip's own clear message.

### 4.4 Homebrew
- Tap repository BilalEjaz/homebrew-locrin with `Formula/locrin.rb` pointing at the macOS and Linux tarballs by URL and sha256.
- The publish job rewrites the formula with the new version and checksums and pushes it to the tap. Users install with `brew install BilalEjaz/locrin/locrin`.

### 4.5 curl installer
- `install.sh` at the repo root, served through the raw GitHub URL and later through locrin.com. Detects OS and architecture, downloads the matching asset and SHA256SUMS from the GitHub release, verifies the checksum, and places the binary in `~/.local/bin` or a directory given by `LOCRIN_INSTALL_DIR`. Accepts a version argument; defaults to latest.
- `install.ps1` does the same on Windows PowerShell 5.1 (no `&&`, no ternaries).
- Both refuse clearly on unsupported platforms and on checksum mismatch, and never leave a half-written binary on the path.

### 4.6 cargo
- `cargo install locrin` works once the crates are published from the tag. The publish job publishes the five crates in dependency order (locrin-core, locrin-rules, locrin-reporters, locrin-mcp, locrin-cli).

### 4.7 Version discipline
All channels publish the version in the root Cargo.toml. The publish job reads it once and refuses to run if the tag does not match it.

## 5. The public benchmark

Repository BilalEjaz/locrin-benchmark, public, MIT.

### 5.1 Corpus
- A few hundred diffs from public open-source projects with visible agent activity, chosen only from licences that allow redistribution (MIT, Apache-2.0, BSD, ISC). Each diff is stored with the source repository, commit hash, licence, language, and the file contents before and after.
- Labels: one JSON record per diff listing, per rule, the expected findings (rule id, file, anchor line) marked true, false positive, or not applicable. Every label is written by one pass and confirmed by a second before it counts; unconfirmed labels are excluded from scoring.
- No proprietary code ever enters the corpus. The founder's private repositories are not sampled.

### 5.2 Harness
- One script (`run.sh` plus a small Python scorer) that installs a named Locrin version via the curl installer, runs `locrin check` over every diff in the corpus, matches findings against the labels by rule id, file and anchor line, and prints precision and recall per rule and per rule-language pair.
- Deterministic: same version, same corpus, same numbers, on any machine. The engine's `--json` output is the only interface used.
- A fixture corpus of ten diffs with known labels ships in the repo with a test that asserts the scorer's exact numbers, so a scoring bug fails a test rather than a results page.

### 5.3 Published results
- A results table in the benchmark README, regenerated by CI on every Locrin release and committed with the Locrin version it was measured against.
- Rules below the 85 percent precision line are marked, including rules that ship off by default, with the ships-off status shown beside them. The page is honest, not flattering.
- The engine's own precision gate keeps running on the private corpus. The public corpus does not gate releases, so a public label dispute can never block a release.

### 5.4 Out of scope
The harness never runs or compares other vendors' tools. Head-to-head comparison is a separate later decision.

## 6. Testing

- Install tests in CI on Ubuntu, macOS and Windows: install the freshly built npm package from a local tarball and the PyPI wheel from the local build, run `locrin --version`, assert the workspace version and exit 0. The same for `install.sh` and `install.ps1` against the built assets served from a local directory.
- The npm launcher has a unit test for exit-code forwarding (0, 1, 2) and for the unsupported-platform message.
- The publish job has a dry-run mode (workflow_dispatch) that builds every package and formula without publishing, used before the first real tag.
- The benchmark scorer has the fixture-corpus test in section 5.2.

## 7. Order of operations

1. Pre-flight (section 3.1 to 3.3) on a branch; merged while private.
2. Packaging (section 4) built and dry-run tested while private.
3. Benchmark repo created and its harness tested against 0.4.0 while the engine repo is still private (the benchmark repo can be public from day one; it contains no engine code).
4. Founder creates an npm automation token and stores it as the NPM_TOKEN secret on the repository (the local npm login has expired; CI publishes with the secret). The same for PyPI (PYPI_TOKEN) and crates.io (CARGO_REGISTRY_TOKEN).
5. Public flip: one command, after 1 to 4 are green and the founder says "make it public".
6. Tag 0.5.0 as the first public release; the publish job populates every channel; the benchmark CI publishes results for 0.5.0.
7. Section 3.4 after-the-flip steps.

## 8. Out of scope for this spec

Licence-key packs, the hosted layer, the editor extension, head-to-head vendor benchmarks, winget and Scoop, the organisation move, and any marketing beyond the README.
