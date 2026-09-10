# Init, Claude Code Hooks, and MCP Server Implementation Plan (version one, plan 4 of 4)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the engine agent-native: `locrin init` wires a repository up in one command, the Claude Code PostToolUse and Stop hooks run the gate at the three moments spec 5.2 names, a pre-commit hook covers every other agent, and a stdio MCP server exposes the five tools of spec 6. After this plan, version one is complete and the founder's repositories can dogfood it.

**Architecture:** Part A (branch `engine/agent`) adds a `hook` subcommand family to the CLI (`post-edit`, `stop`, `pre-commit`) that reads Claude Code's hook JSON on stdin, runs the existing `run::check` pipeline over the narrowest honest scope (one file, the working tree against `HEAD`, the staged files), and prints the exact JSON Claude Code acts on; every hook is wrapped in a two-second watchdog and exits 0 no matter what, because a gate that stalls an agent is worse than no gate (spec 9). `init` writes the config, merges the hook entries into `.claude/settings.json` and the server entry into `.mcp.json`, installs the pre-commit script when nothing else owns it, runs the first scan with a progress line, and writes the baseline. Part B (branch `engine/mcp`) adds the `mcp` crate spec 3 names, holding only the protocol (newline-delimited JSON-RPC over stdio, `initialize`, `tools/list`, `tools/call`), and a `Handler` implementation in the CLI that maps the five tools onto `run::check`, a new symbol search in core, the baseline, and a `last_verdict` row the CLI now records in the index's `meta` table. Rules, the index, and the reporters do not change; the one schema bump adds a parameter count to `symbols` so `find_existing` can rank on signature.

**Tech Stack:** Rust 2021, clap 4, serde and serde_json, rusqlite 0.32, tree-sitter 0.23 (all present in the workspace). New: `serde` with `derive` in `locrin-cli` (workspace dep already declared). No new external crates: the MCP protocol is small enough to write by hand against the 2025-06-18 specification, and an SDK would be the first dependency in the workspace that pulls in an async runtime.

**Spec:** `docs/superpowers/specs/2026-09-05-agent-native-quality-gate-design.md`, sections 3 (crate `mcp`; `cli` holds "commands, hook entry points, `init`"), 5 (`init`, the three moments, token thrift, other agents), 6 (the five tools), 7.2 and 7.3 (verdict and agent JSON), 9 (hook timeout: return pass with a warning; never stall the agent), 10.3 (hooks tested by replaying recorded payloads; MCP protocol conformance tests plus a scripted agent session reaching a clean verdict in under three rounds), 11 (phase 2: `init`, Claude Code hooks, MCP server, pre-commit hook). Plans 1 to 3 are the code this builds on; read `crates/cli/src/main.rs`, `crates/cli/src/run.rs`, `crates/cli/src/git.rs`, `crates/core/src/symbols.rs`, `crates/core/src/index.rs` and `crates/reporters/src/agent.rs` before Task 1.

**Contracts this plan was written against (verified 2026-09-10 on the live docs).** Claude Code hooks (`code.claude.com/docs/en/hooks`): a command hook receives one JSON object on stdin with `session_id`, `transcript_path`, `cwd`, `permission_mode`, `hook_event_name`, and per event `tool_name`, `tool_input`, `tool_response`, `tool_use_id` (PostToolUse) or `stop_hook_active`, `last_assistant_message` (Stop). `tool_input.file_path` for `Edit`, `Write` and `MultiEdit` is always absolute and uses the platform's native separators (backslashes on Windows). PostToolUse cannot block the tool (it already ran); its stdout JSON may carry `{"decision": "block", "reason": "..."}`, which Claude is prompted with and must address, or `{"hookSpecificOutput": {"hookEventName": "PostToolUse", "additionalContext": "..."}}`, which is injected as context without an error. Stop's `{"decision": "block", "reason": "..."}` prevents Claude from stopping and hands it the reason; Claude Code itself gives up after 8 consecutive blocks, and `stop_hook_active` is true while a stop hook's continuation is running. `systemMessage` on any hook's JSON is a warning shown to the user. Hook output strings are capped at 10,000 characters. Hooks are configured under `hooks` in `.claude/settings.json` (project, committed) as `{"<Event>": [{"matcher": "Edit|Write|MultiEdit", "hooks": [{"type": "command", "command": "...", "timeout": <seconds>}]}]}`; the default command timeout is 10 minutes, so ours is set explicitly. Project MCP servers live in `.mcp.json` at the repository root as `{"mcpServers": {"<name>": {"command": "...", "args": [...]}}}`; Claude Code asks the user to approve them once. MCP (`modelcontextprotocol.io/specification/2025-06-18`): stdio transport, UTF-8 JSON-RPC 2.0 messages delimited by newlines with no embedded newlines, the server MUST NOT write anything to stdout that is not a message and MAY log to stderr; the client sends `initialize` (`params.protocolVersion`, `capabilities`, `clientInfo`) and the server answers with `protocolVersion`, `capabilities` (`{"tools": {}}` to expose tools) and `serverInfo` (`name`, `version`), then the client sends the `notifications/initialized` notification; `tools/list` returns `{"tools": [{"name", "description", "inputSchema"}]}`; `tools/call` takes `{"name", "arguments"}` and returns `{"content": [{"type": "text", "text": "..."}], "isError": <bool, optional>}`; `ping` returns `{}`.

**What this plan does not do.** `already-exists` and the fingerprint index stay in release two, so `find_existing` ranks on what the index holds (parameter count, kind, name tokens) rather than on a fingerprint; the spec's "signature vector" is that pair, recorded as a deviation in Task 7. Cursor and Codex native hooks (spec 5.4) are after the Claude Code path is proven. The GitHub Action, the hosted layer, an npm installer and a Homebrew tap are phase two and three. An LLM is used nowhere.

## Global Constraints

- Product name Locrin; config `locrin.toml` (`core::config::CONFIG_FILE`); baseline `locrin-baseline.json` (`core::baseline::BASELINE_FILE`). Cache directory from `core::index::cache_path(root)`'s parent, overridable with `LOCRIN_CACHE_DIR`; the hook session files of Task 2 live beside the index there and never in the repository.
- No LLM anywhere. Network access stays in exactly one place, `core::osv::fetch`; every hook and every MCP tool passes `offline = true` by default (the hooks always; the server unless started with `locrin mcp --online`), so a gate on an agent's edit never waits on osv.dev. `init` runs its first scan online unless `--offline` is given, because that scan is the one that fills the advisory snapshot.
- Hook commands (`locrin hook post-edit`, `locrin hook stop`) ALWAYS exit 0. Their stdout is either empty or exactly one JSON object; every diagnostic goes to stderr or into the JSON's `systemMessage`. An engine error (exit 2 territory anywhere else) becomes a `systemMessage` warning and a pass. A check that has not finished after 2000 ms is abandoned the same way (spec 9). `locrin hook pre-commit` is the exception: it is a git hook and its exit code is the verdict's (0 pass or advisory, 1 block, 2 engine error).
- Hook feedback text is the agent payload of spec 5.3 in plain lines: the verdict line first, then at most ten findings, one line each, `rule file:line  evidence  ->  fix`, then `+N more` when truncated. Never a file's contents. `crates/cli/src/hook/text.rs` is the one place that renders it and both hooks call it.
- The MCP server writes nothing to stdout but protocol messages, one per line, flushed after each. `run.rs` warnings already go to stderr; anything new the server has to say goes there too.
- Schema bumps once, to `"5"`, in Task 7 (`symbols.params`); spec 9 rebuilds on mismatch, so an existing index is rebuilt on the first run after upgrade, silently.
- `init` is idempotent: a second run changes nothing and says so per file. It never overwrites a file it did not write: an existing `locrin.toml`, `locrin-baseline.json`, `.git/hooks/pre-commit`, or a `.husky/` directory is left alone with a one-line instruction printed instead. JSON files it merges into (`.claude/settings.json`, `.mcp.json`) keep every key they had.
- `init` prints every file it touched, one per line, as `wrote <path>`, `updated <path>` or `unchanged <path>`, and nothing else on stdout except the scan and baseline lines (spec 5.1: "prints every file it touched").
- Performance targets stay tests (cold under 5 s, warm single-file under 300 ms, warm 30-file under 1 s, startup under 50 ms). New target from spec 5.2: `locrin hook post-edit` on a warm index under 300 ms end to end, measured in Task 5 on the FastLift checkout with the same four-run method as the precision reports.
- Git: Part A on `engine/agent` off `main`; Part B on `engine/mcp` off `main` after A merges (stacked if A is in review). One commit per task, `cargo fmt --all` before every commit, plain `engine: ...` messages, no attribution trailers, never `git add -A`. No em dashes anywhere.
- `export PATH="$HOME/.cargo/bin:$PATH"` before any cargo command. Corpus checkouts (`<home>/fasting-app`, `<home>/strongspan`, `<home>/teyji`, `<home>/autoqa`, `<home>/fastlift-admin`) are never written into: `init` is exercised on a temporary copy of `fastlift-admin`, and the hook benchmark on `fasting-app` uses a fresh `LOCRIN_CACHE_DIR` and `--root`, which reads only.
- Tests that set `LOCRIN_CACHE_DIR` take the module's `ENV_LOCK` as `run.rs` tests do; e2e tests in `crates/cli/tests/` set it per process through `Command::env` and need no lock.

## Deviations recorded during execution

(Empty at planning time. The executor appends every ruling here with its reason, as plans 2 and 3 did.)

- **Task 2, the Stop hook's session id is percent-encoded and capped before it names a file.** The session id arrives in the payload, and payload data must never become a path on its own terms. It is reduced to `[A-Za-z0-9_-]` and cut at 100 characters before the round counter's file is named. The cost if the ruling is wrong is nothing: a UUID, which is what Claude Code sends, passes through untouched.

- **Task 3, a staged file the engine does not parse still reaches the raw scope.** The brief's test `pre_commit_ignores_a_staged_file_that_is_not_source` asserted the opposite, and it was wrong twice over: the fixture's non-source file is `package.json` rather than `locrin.toml`, and a staged `package.json` is exactly what `vulnerable-dependency` and the Supabase RLS rule gate a commit on. The implementation was kept as written and the stderr assertion moved to `pre_commit_says_when_nothing_is_staged`, so what changed was the plan text.

- **Task 3, the `staged_files` unit tests live in `crates/cli/src/git.rs`.** The CLI has no library target, so `tests/cli.rs` cannot reach the function to test it from outside.

- **Task 4, the file count for `init`'s progress line is walked in `init.rs` rather than reported by `run::scan`.** The file structure said `run.rs` would grow a progress callback. `run::scan` can only report its count once it is over, and the first scan on a cold tree is long enough that a person watching a blank line assumes a hang, so `init` walks with `source_files` first and then calls `run::scan` unchanged. The second walk costs milliseconds and leaves the scan pipeline untouched; the cost if the ruling is wrong is one redundant walk per `init`.

- **Task 4, a `.git/hooks/pre-commit` whose text is exactly `PRE_COMMIT_SCRIPT` is locrin's own.** It reports `unchanged`; any other content reports `skipped` with the line telling the operator what to add by hand. The brief's "always skipped" contradicted its own idempotence test, and it would have made a second `init` disown the file the first one wrote a moment earlier. Nothing is overwritten either way, so the ruling costs nothing if it is wrong.

- **Task 5, the hook benchmark passes no `--root`.** The brief asked for one, and the bench's `locrin` helper already sets the process working directory to the checkout, which is the root the hook reads and the place Claude Code runs a hook from.

- **Task 5, the first run of the four-run benchmark set is discarded and reported.** The cold benchmark reads every file of the 1846-file bench checkout, so the first run after a gap measures the operating system's page cache: 4473 ms against 3866, 3888 and 3871 ms, with warm numbers on the same invocation inside a few milliseconds of the counted runs. All four numbers are in the measurement report and every one of them is green, so the discard changes no verdict. This is the method the two precision reports used.

- **Task 5, the pull request is opened by the controller after the whole-branch review, not by this task.** The brief's step 4 was held back deliberately; the measurement and the dogfood are what Task 5 delivers, in `docs/superpowers/plans/2026-09-10-init-hooks-and-mcp-measurement.md`.

- **Final review, the two agent hook arms catch their own panics.** The watchdog covers a panic on the worker thread; a panic anywhere else in `post_edit` or `stop` is on the process's own thread and unwinds into `main`'s `catch_unwind`, which exits 2. Claude Code reads a Stop hook's exit 2 as "block, and show stderr to the agent", the one failure spec 9 exists to prevent, and it would repeat on every stop. `hook::guarded` wraps both arms, answers a panic with the same `systemMessage` shape every other failure uses, and exits 0. `pre_commit` is deliberately not wrapped: its exit code is the verdict's, and a broken engine there must stop the commit.

- **Final review, the pre-commit hook's directory comes from `git rev-parse --git-path hooks`, not from `.git/hooks`.** The hard-coded path was wrong in two ordinary situations: in a linked worktree `.git` is a file, so init reported "not a git repository" to somebody standing in one, and under `core.hooksPath` init wrote a file git never runs and reported `wrote` for it. `git::hooks_dir` asks git, creates the directory when it is not there yet (`core.hooksPath` may name one nobody has made), and returns None for every way of failing, which keeps the existing "not a git repository" skip. Husky is still checked first. The path reported on stdout is relative to the root where the directory is under it and absolute where it is not, which is the ordinary case in a worktree.

- **Task 6, a JSON-RPC message is dispatched on its method before its id.** A message carrying neither is answered `-32600` with a null id rather than being read as a notification and dropped, and an explicit `id: null` is treated as a notification, which is what the JSON-RPC 2.0 text says. Only a malformed client can reach either arm, so the cost if the ruling is wrong is one error shape nobody well-behaved ever sees.

- **Task 8, three tool-argument resolutions.** `find_existing` adds its "return types are not indexed in version one" note only when `returns` was actually supplied, because a note on a call that never mentioned it reads as a search that considered something it did not. Every argument is validated before the index is looked at, so a caller with a bad argument is told about the argument rather than about a missing index. "At least one of `intent` or `name`" is expressed as prose in the schema rather than as an `anyOf` of two `required` lists, which every client renders badly, and the tool repeats it in words when a call arrives empty.

- **Task 8, `find_existing` refuses an index that `Index::open` has just rebuilt empty.** The path check alone was not enough: `open` rebuilds an index whose schema this build does not know, or one that is corrupt, into an empty database, and an upgraded binary meets exactly that on its first run. The tool would then answer "nothing exists" to the one question an agent asks before writing a duplicate. It now checks again once the database is open and says `index is empty: run locrin scan first`. Task 9's `a_stale_index_rebuilds_silently_for_the_server` covers the same rebuild from the server's side, where the rebuild must also not put a line on stdout.

- **Task 9, the cold-index benchmark missed its target on the discarded first run and is recorded rather than tuned away.** Run 1 measured 6054 ms against spec 3.4's 5000 ms; the three counted runs are 4403, 4716 and 4673 ms. Part A's discard changed no verdict because its run 1 was green, and this one does, so it is stated in the measurement rather than left to the table. Nothing was tuned and no target was moved.

- **Task 9, the whole Part B benchmark set is slower than Part A's because the machine was on battery, and the numbers stand as measured.** Every gate moved, `--help` included, which is a process start and a print: a uniform slowdown across a benchmark that reads 1846 files and one that reads none is the machine, not the engine. The laptop was on battery under the Balanced power scheme, with CPU samples at 4 to 13 percent against Part A's 0 to 4. Two gates are consequently thin (the post-edit hook at 298 ms and the warm single-file check at 293 ms, both against 300 ms). They pass as measured and are reported as a concern; re-running until the numbers improve is not a measurement.

- **Task 9, the pull request is opened by the controller after the whole-branch review, not by this task.** As in Task 5, the brief's `gh pr create` step was held back deliberately. What Task 9 delivers is the scripted session and stale-index tests, the README, and the Part B measurement.

## File structure

Part A:
- `crates/cli/Cargo.toml` (modify): add `serde = { workspace = true }`.
- `crates/cli/src/hook/mod.rs` (create): `Input` (the hook stdin), `watchdog`, `emit`, and the three entry points.
- `crates/cli/src/hook/text.rs` (create): `feedback(&Verdict) -> String`, the compact agent text.
- `crates/cli/src/hook/session.rs` (create): the per-session round counter for the Stop hook.
- `crates/cli/src/git.rs` (modify): `has_head(root) -> bool`, `staged_files(root) -> Result<Vec<String>>`.
- `crates/cli/src/init.rs` (create): `run(root, offline) -> Result<Report>` and the JSON merges.
- `crates/cli/src/main.rs` (modify): `Hook { PostEdit | Stop | PreCommit }` and `Init` subcommands.
- `crates/cli/tests/fixtures/hooks/{post_edit_write.json, post_edit_edit_windows.json, post_edit_markdown.json, stop.json, stop_active.json}` (create): recorded payloads.
- `crates/cli/tests/hooks.rs`, `crates/cli/tests/init.rs` (create): end-to-end tests.

Part B:
- `crates/mcp/Cargo.toml`, `crates/mcp/src/lib.rs` (create): `ToolSpec`, `ToolError`, `Handler`, `serve`.
- `crates/core/src/index.rs` (modify): schema `"5"`, `symbols.params`, `meta_get`, `meta_set`.
- `crates/core/src/symbols.rs` (modify): `params` on `Symbol`, `search(index, &Query) -> Result<Vec<Match>>`.
- `crates/cli/src/run.rs` (modify): `check` records `last_verdict`; `baseline_accept_as`.
- `crates/cli/src/mcp_tools.rs` (create): `Tools` implementing `Handler`.
- `crates/cli/src/main.rs` (modify): `Mcp { online }` subcommand.
- `crates/cli/tests/mcp.rs` (create): protocol and scripted-session tests; `README.md` (create).
- `Cargo.toml` (modify): workspace member `crates/mcp`.

---

## Part A: hooks, init, pre-commit (branch `engine/agent`)

### Task 1: `locrin hook post-edit`

**Files:**
- Create: `crates/cli/src/hook/mod.rs`, `crates/cli/src/hook/text.rs`
- Modify: `crates/cli/src/main.rs`, `crates/cli/Cargo.toml`
- Test: `crates/cli/src/hook/text.rs` (unit), `crates/cli/tests/hooks.rs` with `crates/cli/tests/fixtures/hooks/post_edit_write.json`, `post_edit_edit_windows.json`, `post_edit_markdown.json`

**Interfaces:**
```rust
// crates/cli/src/hook/mod.rs
pub mod session;
pub mod text;

use serde::Deserialize;

/// What Claude Code writes to a hook's stdin. Every field this crate reads is
/// optional at the type level so a payload from a newer or older Claude Code
/// still parses; a missing field the hook needs makes the hook a no-op, never
/// an error.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Input {
    pub session_id: String,
    pub cwd: String,
    pub hook_event_name: String,
    pub tool_name: String,
    pub tool_input: ToolInput,
    pub stop_hook_active: bool,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct ToolInput {
    pub file_path: String,
}

/// Reads and parses stdin. Unparseable input is a no-op with a warning: the
/// hook cannot know what it was asked about, and stalling the agent over it
/// would be worse than checking nothing.
pub fn read_input() -> Option<Input>;

/// Runs `f` on a worker thread and waits at most `HOOK_BUDGET_MS`. On timeout
/// returns None; the process exits right after, which is what abandons the
/// worker. See spec 9.
pub const HOOK_BUDGET_MS: u64 = 2000;
pub fn watchdog<T: Send + 'static>(f: impl FnOnce() -> anyhow::Result<T> + Send + 'static) -> Option<anyhow::Result<T>>;

/// Prints one JSON object on stdout (nothing when `v` is None) and returns the
/// exit code, which is always 0.
pub fn emit(v: Option<serde_json::Value>) -> i32;

pub fn post_edit(root: &Path, input: Input) -> i32;
```
```rust
// crates/cli/src/hook/text.rs
/// The agent payload of spec 5.3 as plain lines. Verdict first; at most ten
/// findings (the verdict is already capped by agent::CAP when it reaches here,
/// so this only renders); `+N more` when `v.truncated > 0`.
pub fn feedback(v: &Verdict) -> String;
```
Output format, exactly:
```
BLOCK  2 blocking, 1 advisory, 3 finding(s) in 143 ms
leftover-debug  src/a.ts:12  console.log("x")  ->  Remove the debug statement or mark the line locrin:allow
secret-exposed  src/k.ts:3  AWS access key id, AKIA... (20 chars)  ->  Move the credential to an environment variable and rotate it
unused-import  src/a.ts:1  import { x } from "./x"  ->  Delete the import
```
(`PASS  0 finding(s) in 40 ms` and `ADVISORY  0 blocking, 2 advisory, 2 finding(s) in 90 ms` are the other two first lines; `+3 more` is the last line when truncated. Two spaces separate the columns and `  ->  ` separates evidence from fix.)

Behaviour of `post_edit`:
1. If `input.tool_input.file_path` is empty, or `Language::from_path` says the engine does not parse it (a `.md`, a `.json`), or the path does not exist, or it is outside `root` after `canonical_path`: emit nothing, exit 0. Backslashes need no translation: `PathBuf::from` reads the native form on the platform the hook runs on.
2. Otherwise build `run::Options { root, paths: vec![path], changed_only: false, json: true, offline: true, diff: None }`, run `run::check` inside `watchdog`, and:
   - `None` (timeout): emit `{"systemMessage": "locrin: check of <rel> did not finish in 2 s; passed without checking it"}`.
   - `Some(Err(e))`: emit `{"systemMessage": "locrin: <e:#>; passed without checking <rel>"}`.
   - `Some(Ok(v))` with `v.blocking > 0`: `let v = v.capped(agent::CAP);` emit `{"decision": "block", "reason": text::feedback(&v)}`.
   - `Some(Ok(v))` with findings but no blocking: emit `{"hookSpecificOutput": {"hookEventName": "PostToolUse", "additionalContext": text::feedback(&v)}}`.
   - `Some(Ok(v))` clean: emit nothing.
3. `main.rs`: `Cmd::Hook { cmd: HookCmd::PostEdit }` calls `hook::read_input()` then `hook::post_edit(&root, input)`, where `root` is `--root` or the current directory (Claude Code runs hooks in the project directory). No `--offline` flag on hook commands: they are always offline.

- [ ] Step 1: failing tests. Unit tests in `text.rs`: `renders_verdict_line_then_one_line_per_finding` (build a `Verdict` from three findings via `Verdict::from_findings`, assert the exact string above modulo the ms), `says_how_many_more_when_truncated` (`Verdict::from_findings(15 findings).capped(10)` ends with `+5 more`), `pass_is_one_line`. E2e tests in `tests/hooks.rs` using the `copy_fixture` and `locrin` helpers copied from `cli.rs` (a `mod common;` shared file is fine): `post_edit_blocks_on_a_dirty_file` (feed `post_edit_write.json` with `file_path` rewritten to the temp copy's absolute `src/dirty.ts`; assert exit 0, stdout parses, `decision == "block"`, `reason` starts with `BLOCK`, contains `leftover-debug` and `src/dirty.ts:`); `post_edit_is_silent_on_a_clean_file` (`src/clean.ts`: exit 0, empty stdout); `post_edit_ignores_a_markdown_file` (`post_edit_markdown.json`, empty stdout); `post_edit_survives_garbage_stdin` (stdin `not json`: exit 0, empty stdout, stderr mentions `locrin`); `post_edit_accepts_a_windows_path` (`post_edit_edit_windows.json` carries `C:\\...\\src\\dirty.ts` with backslashes; on Windows it blocks, on other platforms the test is `#[cfg(windows)]`). The fixture payloads are the documented shapes with every field present (`session_id`, `transcript_path`, `cwd`, `permission_mode`, `hook_event_name`, `tool_name`, `tool_input`, `tool_response`, `tool_use_id`) so the parser is tested against a real payload and not a minimal one; the test rewrites only `file_path` and `cwd`.
- [ ] Step 2: run, see them fail (`hook` module absent).
- [ ] Step 3: implement `hook/mod.rs`, `hook/text.rs`, the `Hook` subcommand, `serde` in `Cargo.toml`. `watchdog` is `std::thread::spawn` plus `mpsc::sync_channel(1)` and `recv_timeout(Duration::from_millis(HOOK_BUDGET_MS))`.
- [ ] Step 4: `cargo test --workspace` green; `cargo fmt --all`; commit `engine: the PostToolUse hook, a single-file check behind a two-second watchdog`.

### Task 2: `locrin hook stop`

**Files:**
- Create: `crates/cli/src/hook/session.rs`
- Modify: `crates/cli/src/hook/mod.rs`, `crates/cli/src/git.rs`, `crates/cli/src/main.rs`
- Test: `crates/cli/src/hook/session.rs` (unit), `crates/cli/tests/hooks.rs` with `stop.json`, `stop_active.json`

**Interfaces:**
```rust
// crates/cli/src/hook/session.rs
pub const MAX_ROUNDS: u32 = 3;

/// The Stop hook's memory of one Claude Code session: how many times it has
/// already sent the agent back. Lives beside the index, keyed by session id,
/// never in the repository.
pub struct Session { path: PathBuf, pub rounds: u32 }
impl Session {
    /// `<cache dir>/sessions/<session_id>.json`, where cache dir is
    /// `core::index::cache_path(root).parent()`. A missing or unreadable file is
    /// round zero. An empty session id maps to "anonymous".
    pub fn load(root: &Path, session_id: &str) -> Session;
    pub fn save(&self) -> anyhow::Result<()>;   // creates the directory; writes {"rounds": n}
}
```
```rust
// crates/cli/src/git.rs
/// Whether `root` is inside a git work tree that has at least one commit, which
/// is what a diff against HEAD needs. False for no git, no repository, or an
/// unborn branch.
pub fn has_head(root: &Path) -> bool;
```
Behaviour of `hook::stop(root, input) -> i32`:
1. `let mut s = Session::load(root, &input.session_id)`. If `s.rounds > MAX_ROUNDS`: emit nothing, exit 0 (the human was told when the cap was hit; nothing more to say).
2. Scope: `diff = has_head(root).then(|| DiffScope::Base("HEAD".into()))`; `changed_only = diff.is_none()`. `Options { root, paths: vec![], changed_only, json: true, offline: true, diff }`. `--base HEAD` is the working-tree diff spec 5.2 asks for (files differing from HEAD plus untracked files, which `git::changed_files` already lists).
3. Run inside `watchdog`; timeout and error become a `systemMessage` pass exactly as in Task 1 (text: `locrin: working-tree check did not finish in 2 s; not blocking the stop`).
4. `v.blocking == 0`: `s.rounds = 0; s.save()` (a clean verdict closes the loop for this session), emit nothing.
5. `v.blocking > 0`: `s.rounds += 1; s.save()`. The agent is sent back on rounds 1, 2 and 3 and on the fourth stop the hook stands down (spec 5.2: "hard cap of three rounds per session"). So: if `s.rounds <= MAX_ROUNDS`, emit `{"decision": "block", "reason": format!("{}\nRound {} of {}: fix the blocking findings above, then stop again.", text::feedback(&v.capped(CAP)), s.rounds, MAX_ROUNDS)}`; if `s.rounds == MAX_ROUNDS + 1`, emit `{"systemMessage": format!("locrin: {} blocking finding(s) remain after {} rounds; the agent was not sent back again. Run `locrin check --base HEAD` to see them.", v.blocking, MAX_ROUNDS)}`; step 1's short-circuit is therefore `s.rounds > MAX_ROUNDS`, which is what the saved value reads from then on.
6. `input.stop_hook_active` is read for the record and changes nothing: the round counter is the loop guard, and it is the same guard whether the stop is Claude's own or a continuation.

- [ ] Step 1: failing tests. Unit: `session_round_trips_and_is_zero_when_absent` (temp `LOCRIN_CACHE_DIR`, `ENV_LOCK`). `git::has_head` in `cli.rs`: false in a temp dir with no repo, false after `git init`, true after one commit. E2e in `tests/hooks.rs`, each on a `copy_fixture()` with `git init`, `git add .`, `git commit -m base` (the fixture's `src/dirty.ts` is committed, so the working tree is clean): `stop_is_silent_when_nothing_changed` (empty stdout); `stop_sends_the_agent_back_up_to_three_times_then_tells_the_human` (write a `console.log` into `src/clean.ts`; run the hook four times with the same `session_id`; runs 1 to 3 produce `decision: block` with `Round 1 of 3`, `Round 2 of 3`, `Round 3 of 3` in `reason`; run 4 produces `systemMessage` and no `decision`; run 5 produces nothing); `a_clean_verdict_resets_the_rounds` (one block, then revert the file, run: empty stdout; dirty again: `Round 1 of 3`); `stop_uses_changed_scope_without_a_head` (no `git init` at all: the fixture's dirty file is new to the index, so `--changed` after a first `scan` sees only an edited file; assert the block names `src/clean.ts` and not `src/dirty.ts`).
- [ ] Step 2: run, see them fail.
- [ ] Step 3: implement `session.rs`, `has_head` (`git rev-parse --verify HEAD` exit status, `git_command` with `LC_ALL=C` as the rest of the module), `hook::stop`, the `HookCmd::Stop` arm.
- [ ] Step 4: `cargo test --workspace` green; fmt; commit `engine: the Stop hook, a working-tree check with a three-round cap per session`.

### Task 3: `locrin hook pre-commit`

**Files:**
- Modify: `crates/cli/src/hook/mod.rs`, `crates/cli/src/git.rs`, `crates/cli/src/main.rs`
- Test: `crates/cli/tests/hooks.rs`, `git::staged_files` unit in `cli.rs`

**Interfaces:**
```rust
// crates/cli/src/git.rs
/// Paths staged for the next commit (added, copied, modified, renamed), relative
/// to the repository top level, in git's order. Works on an unborn branch, where
/// git diffs the index against the empty tree.
pub fn staged_files(root: &Path) -> anyhow::Result<Vec<String>>;
// git diff --cached --name-only -z --diff-filter=ACMR, run with current_dir(top_level), rels re-based
// onto `root` the same way changed_files does.
```
Behaviour of `hook::pre_commit(root) -> anyhow::Result<i32>`: `let staged = git::staged_files(root)?; let paths: Vec<PathBuf> = staged.into_iter().map(|r| root.join(r)).filter(|p| p.is_file()).collect();` (a staged deletion names a file that is gone; `explicit_files` would refuse it). Empty `paths`: print `locrin: nothing staged to check` on stderr, return 0. Otherwise `run::check(&Options { root, paths, changed_only: false, json: false, offline: true, diff: None })`, print `terminal::render`, return `verdict.exit_code()`. No watchdog: a commit is not an agent turn, and the human asked for the gate. Engine errors propagate to `main`'s exit 2, so a broken engine never lets a commit through as a pass; the operator can `git commit --no-verify` deliberately.

- [ ] Step 1: failing tests. `staged_files` unit (init, write two files, stage one: exactly that one; stage on an unborn branch works; a staged path under a subdirectory is returned relative to the top level). E2e: `pre_commit_blocks_when_a_staged_file_blocks` (fixture copy, `git init`, `git add src/dirty.ts`: exit 1, stdout starts with `BLOCK`); `pre_commit_passes_a_clean_stage` (`git add src/clean.ts`: exit 0); `pre_commit_ignores_a_staged_file_that_is_not_source` (stage `locrin.toml` only: exit 0, stderr says nothing staged to check); `pre_commit_ignores_unstaged_dirt` (stage `src/clean.ts` while `src/dirty.ts` is only in the working tree: exit 0).
- [ ] Step 2: run, see them fail.
- [ ] Step 3: implement.
- [ ] Step 4: `cargo test --workspace` green; fmt; commit `engine: the pre-commit hook checks what is staged`.

### Task 4: `locrin init`

**Files:**
- Create: `crates/cli/src/init.rs`
- Modify: `crates/cli/src/main.rs`, `crates/cli/src/run.rs` (progress callback on `scan`)
- Test: `crates/cli/src/init.rs` (unit, the merges), `crates/cli/tests/init.rs`

**Interfaces:**
```rust
// crates/cli/src/init.rs
pub enum Touch { Wrote, Updated, Unchanged, Skipped(String) }   // Skipped carries the one-line instruction
pub struct Report { pub touched: Vec<(String, Touch)>, pub files_indexed: usize, pub baseline_entries: Option<usize> }

pub fn run(root: &Path, offline: bool, progress: &mut dyn FnMut(&str)) -> anyhow::Result<Report>;

/// Pure merges, unit-tested on strings. Each returns (new text, changed?).
pub fn merge_settings(existing: Option<&str>) -> anyhow::Result<(String, bool)>;
pub fn merge_mcp(existing: Option<&str>) -> anyhow::Result<(String, bool)>;
pub const CONFIG_TEMPLATE: &str;   // below
pub const PRE_COMMIT_SCRIPT: &str = "#!/bin/sh\n# Installed by locrin init. Remove this file to uninstall.\nexec locrin hook pre-commit\n";
```
`CONFIG_TEMPLATE` (written only when `locrin.toml` is absent; every setting commented out so the file loads as `Config::default()`):
```toml
# Locrin configuration. Every key is optional; an absent key keeps the default.
# Docs: README.md, section "Config".

# Globs the engine never reads (generated code, vendored code).
# excludes = ["src/generated/**"]

# Files where debug output is allowed (scripts, config files, bin).
# debug_allowed = ["**/scripts/**", "**/*.config.*", "**/bin/**"]

# Extra entry points for the dead-code rules, beyond package.json main/bin/exports.
# entry_points = ["src/worker.ts"]

# Import directions. Each entry sets exactly one of forbid or allow.
# [[boundaries]]
# name = "ui never imports the database"
# from = "src/ui/**"
# forbid = ["src/db/**"]

# [framework]
# auth_middleware = ["requireAuth"]
# server_paths = ["server/**"]

# Per-rule overrides. secret-exposed is locked and ignores these.
# [rules.dead-file]
# enabled = true
# [rules.unused-import]
# severity = "medium"
```
`merge_settings`: parse `existing` as a JSON object (an absent file is `{}`; a file that is not a JSON object is an error naming the path, never overwritten). Ensure `hooks.PostToolUse` is an array holding an entry whose `matcher` is `"Edit|Write|MultiEdit"` and whose `hooks` array contains `{"type": "command", "command": "locrin hook post-edit", "timeout": 5}`; ensure `hooks.Stop` holds an entry (no matcher) whose `hooks` contains `{"type": "command", "command": "locrin hook stop", "timeout": 10}`. "Contains" is decided by scanning every `hooks[*].hooks[*].command` string in the whole event array for one that starts with `locrin hook post-edit` (resp. `locrin hook stop`); if one exists anywhere, nothing is added (an operator who moved the command into a wrapper script keeps their arrangement). Serialise with `serde_json::to_string_pretty` plus a trailing newline. `changed` is whether the parsed value changed, so a re-run over an already merged file reports `Unchanged` even if the original was formatted differently (compare values, not text; write only when changed).

`merge_mcp`: same shape over `.mcp.json`: ensure `mcpServers.locrin` exists; if absent set it to `{"command": "locrin", "args": ["mcp"]}`; if present leave it exactly as is.

`run` order, each step pushing to `report.touched`:
1. `locrin.toml`: absent -> write `CONFIG_TEMPLATE`, `Wrote`; present -> `Unchanged`.
2. `.claude/settings.json` via `merge_settings` (create `.claude/` if needed): `Wrote` when the file did not exist, `Updated` when it changed, `Unchanged`.
3. `.mcp.json` via `merge_mcp`, same three outcomes.
4. Pre-commit: if `root/.husky` is a directory -> `Skipped("husky detected: add `locrin hook pre-commit` to .husky/pre-commit")`; else if `root/.git/hooks` is not a directory -> `Skipped("not a git repository: no pre-commit hook installed")`; else if `root/.git/hooks/pre-commit` exists -> `Skipped("a pre-commit hook already exists: add `locrin hook pre-commit` to it")`; else write `PRE_COMMIT_SCRIPT`, `#[cfg(unix)]` set mode `0o755`, `Wrote`.
5. First scan: `progress(&format!("indexing {} source file(s) under {}", n, root.display()))` where `n = source_files(root, &WalkOptions{excludes: Config::load(root)?.excludes}).len()` (a second walk is milliseconds and the count is the progress signal the 26-second first scan needs); then `run::scan(root, offline)`, then `progress(&format!("indexed {} file(s) in {:.1} s", files, elapsed))`. This is the follow-up recorded on 2026-09-10 in the plan 3 precision report: the first scan after a reboot is the OS reading the tree, and a person watching a blank line for 26 seconds assumes a hang.
6. Baseline: `locrin-baseline.json` present -> `Unchanged`, `baseline_entries: None`; absent -> `run::baseline_create(root, offline)`, `Wrote`, `Some(n)`. The scan in step 5 has already warmed the findings cache, so this second pass over the repository is served from it.

`main.rs`: `Cmd::Init { offline }` prints each `touched` line as `wrote <rel>` / `updated <rel>` / `unchanged <rel>` / `skipped <rel>: <instruction>`, passes `|line| eprintln!("{line}")` as the progress sink (progress is not a file the command touched, so it stays off stdout), and ends with `baseline written with N finding(s)` when a baseline was written. Exit 0.

- [ ] Step 1: failing tests. Unit (`init.rs`): `merge_settings_creates_both_hooks_from_nothing` (assert the exact parsed structure: two events, the matcher string, both commands and timeouts); `merge_settings_keeps_existing_hooks_and_keys` (input with a `PreToolUse` entry, a `permissions` key and a `PostToolUse` entry for another command: all preserved, ours appended as a new entry in `PostToolUse`); `merge_settings_is_idempotent` (merge twice, second returns `changed == false`); `an_existing_locrin_command_is_not_duplicated` (a `PostToolUse` entry whose command is `locrin hook post-edit --root .` is present: nothing is added, because the rule is "a command that starts with `locrin hook post-edit`"); `a_wrapped_command_is_not_recognised` (command `sh -c "locrin hook post-edit || true"` does not start with the prefix, so the hook is added beside it: pinned so the rule is explicit rather than accidental); `merge_settings_refuses_a_non_object` (input `[]`: error). `merge_mcp` equivalents (creates, preserves another server, idempotent, leaves an existing `locrin` entry that differs). E2e (`tests/init.rs`, temp dir with a small TS file, `git init`): `init_writes_every_file_and_says_so` (stdout has `wrote locrin.toml`, `wrote .claude/settings.json`, `wrote .mcp.json`, `wrote .git/hooks/pre-commit`, `wrote locrin-baseline.json`; all five exist; settings parse; stderr has `indexing 1 source file(s)` and `indexed 1 file(s) in`); `init_twice_changes_nothing` (second run: every line `unchanged`, file bytes identical); `init_leaves_an_existing_pre_commit_alone` (pre-create the file with known content: `skipped .git/hooks/pre-commit:` line, content unchanged); `init_without_git_skips_the_hook` (no `git init`: `skipped ... not a git repository`, everything else written); `init_does_not_reset_an_existing_baseline` (pre-write a baseline with one entry: `unchanged locrin-baseline.json`, entry still there).
- [ ] Step 2: run, see them fail.
- [ ] Step 3: implement.
- [ ] Step 4: `cargo test --workspace` green; fmt; commit `engine: init wires a repository up in one command`.

### Task 5: Part A measurement, dogfood, pull request

**Files:** `docs/superpowers/plans/2026-09-10-init-hooks-and-mcp-measurement.md` (create), `crates/cli/tests/bench.rs` (modify: one more ignored bench).

- [ ] Step 1: add `post_edit_hook_under_300ms` to `bench.rs`: scan the bench repo into a fresh cache, then time `locrin hook post-edit` with a `post_edit_write.json` payload whose `file_path` is the absolute `app/_layout.tsx`, stdin piped, `--root` pointing at the repo; assert exit 0 and under 300 ms. Run the four-run method (`cargo test --release -p locrin-cli -- --ignored --nocapture` four times, machine idle, first run discarded) and record all four numbers for every benchmark.
- [ ] Step 2: dogfood on a copy. `cp -r <home>/fastlift-admin <temp>`, `cd <temp>`, `locrin init` with a fresh `LOCRIN_CACHE_DIR`; record the printed lines verbatim; open `.claude/settings.json` and `.mcp.json` and paste them into the measurement doc. Then replay: write a `console.log` into one source file through a hand-built PostToolUse payload, run `locrin hook post-edit`, record stdout; run `locrin hook stop` four times, record the four outputs. Delete the copy.
- [ ] Step 3: the measurement doc records the benchmark table, the dogfood transcript, and any deviation appended to this plan. Commit `docs: Part A measurement and the init dogfood transcript`.
- [ ] Step 4: `gh pr create --base main` from `engine/agent`, title `Init, Claude Code hooks, pre-commit hook (plan 4 part A)`, body listing the four commands, the hook contract this was built against (the paragraph at the top of this plan, condensed), and the benchmark line. Founder merges.

---

## Part B: the MCP server (branch `engine/mcp`)

### Task 6: `crates/mcp`, the protocol

**Files:**
- Create: `crates/mcp/Cargo.toml` (`name = "locrin-mcp"`, lib `locrin_mcp`, deps `serde_json`, `anyhow`), `crates/mcp/src/lib.rs`
- Modify: `Cargo.toml` (workspace member)

**Interfaces:**
```rust
pub struct ToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    /// A JSON Schema object for the tool's arguments.
    pub input_schema: serde_json::Value,
}

/// A tool that ran and has something wrong to report. Becomes a result with
/// `isError: true`, which the model sees; a JSON-RPC error is reserved for a
/// request the server could not understand.
pub struct ToolError(pub String);

pub trait Handler {
    fn tools(&self) -> Vec<ToolSpec>;
    fn call(&mut self, name: &str, args: &serde_json::Value) -> Result<String, ToolError>;
}

pub const PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

/// Serves one connection: reads newline-delimited JSON-RPC from `reader`, writes
/// one message per line to `writer`, flushing after each, until EOF. Never
/// panics on input: every malformed line gets a JSON-RPC error and the loop
/// continues. Returns when stdin closes.
pub fn serve<R: BufRead, W: Write>(reader: R, writer: W, handler: &mut dyn Handler, name: &str, version: &str) -> anyhow::Result<()>;

/// One message in, at most one message out. Public so the tests can drive it
/// without a pipe.
pub fn handle(line: &str, handler: &mut dyn Handler, name: &str, version: &str) -> Option<serde_json::Value>;
```
Dispatch in `handle`:
- Not JSON: `{"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"parse error"}}`.
- JSON without a string `method`: `-32600` `invalid request` with the request's `id` (or null).
- No `id` (a notification: `notifications/initialized`, `notifications/cancelled`, anything): return `None`, whatever the method.
- `initialize`: result `{"protocolVersion": <the client's params.protocolVersion when it is in PROTOCOL_VERSIONS, else PROTOCOL_VERSIONS[0]>, "capabilities": {"tools": {}}, "serverInfo": {"name": name, "version": version}}`.
- `ping`: result `{}`.
- `tools/list`: `{"tools": [{"name","description","inputSchema"} ...]}` from `handler.tools()`.
- `tools/call`: `params.name` (string, else `-32602` `invalid params`), `params.arguments` (object, default `{}`); unknown name (not in `handler.tools()`) -> `-32602` `unknown tool: <name>`; `Ok(text)` -> `{"content":[{"type":"text","text":text}]}`; `Err(ToolError(text))` -> `{"content":[{"type":"text","text":text}],"isError":true}`. `handler.call` runs under `std::panic::catch_unwind(AssertUnwindSafe(..))`: a panic is `isError` with `internal engine failure` and the server stays up.
- Any other method with an id: `-32601` `method not found`.
Every response carries the request's `id` verbatim (number or string). Output is `serde_json::to_string` (compact, one line; `serde_json` never emits a raw newline inside a string).

- [ ] Step 1: failing tests in `lib.rs` with a `Fake` handler holding two tools (`echo` returns its `text` argument; `fail` returns `ToolError`; `boom` panics): `initialize_echoes_a_supported_version_and_declares_tools`; `initialize_falls_back_to_the_newest_version_for_an_unknown_one`; `notifications_get_no_response`; `tools_list_describes_every_tool`; `tools_call_wraps_text`; `tool_error_is_is_error_not_a_protocol_error`; `a_panicking_tool_is_is_error_and_the_next_call_still_works`; `unknown_tool_is_invalid_params`; `unknown_method_is_method_not_found`; `garbage_is_a_parse_error_with_null_id`; `serve_runs_a_whole_conversation_over_a_pipe` (a `Cursor` of five lines in: `initialize`, `notifications/initialized`, `tools/list`, `tools/call`, `ping`; exactly four lines out, one per request and none for the notification, each parsing as JSON with the matching `id`).
- [ ] Step 2: run, fail to compile.
- [ ] Step 3: implement.
- [ ] Step 4: `cargo test -p locrin-mcp` green; fmt; commit `engine: the mcp crate, newline-delimited JSON-RPC over stdio with tools`.

### Task 7: Symbol parameters, symbol search, `meta`, `last_verdict`

**Files:**
- Modify: `crates/core/src/index.rs`, `crates/core/src/symbols.rs`, `crates/cli/src/run.rs`
- Test: unit tests in each; `run.rs` test for `last_verdict`

**Interfaces:**
```rust
// core::index
pub const SCHEMA_VERSION: &str = "5";
// symbols gains: params INTEGER   (NULL when the symbol is not callable)
impl Index {
    pub fn meta_get(&self, key: &str) -> anyhow::Result<Option<String>>;
    pub fn meta_set(&mut self, key: &str, value: &str) -> anyhow::Result<()>;   // INSERT OR REPLACE
}

// core::symbols
pub struct Symbol { /* existing */ pub params: Option<u32> }
// extract: params = Some(named children of the `parameters` field) for function_declaration,
// generator_function_declaration, and for a const whose declarator value is an arrow_function
// or function_expression (walk value.child_by_field_name("parameters")); None for everything else.
// store: writes params; the SELECT sites read it.

pub struct Query { pub name: Option<String>, pub intent: Option<String>, pub params: Option<u32> }
pub struct Match { pub rel: String, pub line: u32, pub kind: String, pub name: String, pub params: Option<u32>, pub exported: bool, pub score: (u8, u32) }

/// Ranks every symbol in the index against the query: signature first (the
/// parameter count matches, when both are known), name-token overlap second.
/// Only symbols sharing at least one token with the query are returned, best
/// first, ties by rel then line. `limit` caps the result.
pub fn search(index: &Index, q: &Query, limit: usize) -> anyhow::Result<Vec<Match>>;

/// camelCase, PascalCase, snake_case and kebab-case split to lowercase tokens;
/// `intent` is split on whitespace and punctuation with STOPWORDS removed.
pub fn tokens(s: &str) -> Vec<String>;
pub const STOPWORDS: &[&str] = &["a","an","the","to","for","of","in","on","that","is","with","and","or","function","helper","method"];
```
Scoring: `query_tokens = tokens(name) ∪ tokens(intent)` (deduplicated); for each symbol, `overlap = |tokens(symbol.name) ∩ query_tokens|`; skip when `overlap == 0`; `sig = if q.params.is_some() && q.params == symbol.params { 1 } else { 0 }`; `score = (sig, overlap)`; sort by score descending, then `rel`, then `line`. Deviation to record: spec 6 says "signature vector similarity first and name token overlap second"; without the fingerprint index (release two) the signature vector version one can honestly compute is the parameter count, and this is that, recorded here so release two knows what to replace.

`run::check` records the verdict: after `Verdict::from_findings`, when the run recorded (it always does in `check`), open the index once more (`Index::open(&root)`) and `meta_set("last_verdict", json)` where json is `{"status","blocking","high","medium","low","duration_ms","at": <unix seconds>,"scope": <"repo" | "paths" | "changed" | "base:<ref>" | "since:<ref>">}`. A failure to record is a warning on stderr, never an error: the verdict is the answer, the note is a convenience.

- [ ] Step 1: failing tests. `index.rs`: `meta_round_trip`; `schema_five_rebuilds_a_schema_four_index` (open, downgrade `meta` to `'4'` as the existing test does for `'0'`, reopen, assert empty and `'5'`). `symbols.rs`: `extracts_parameter_counts` (a fixture source with `function a(x, y) {}`, `export const b = (p) => p`, `const c = function () {}`, `class D {}`, `const e = 1`: params `Some(2)`, `Some(1)`, `Some(0)`, `None`, `None`); `tokens_split_every_casing` (`listFoodEntriesBetween` -> `[list, food, entries, between]`; `DAY_RECORD_id` -> `[day, record, id]`; `to-kebab` -> `[to, kebab]`, note `to` survives here because STOPWORDS apply to intent only); `search_ranks_signature_then_overlap` (an in-memory index with `listFoodEntries(2 params)`, `listFoodEntriesBetween(3 params)`, `foodTotal(1 param)`, `unrelated(0)`; query `name: "listEntries", params: Some(3)` returns `listFoodEntriesBetween` first, then `listFoodEntries`, then nothing else... `foodTotal` shares no token with `[list, entries]`, so it is absent; query `intent: "sum the food for a day"` shares the token `food` with `foodTotal` and both `listFood*`: assert exactly that set is returned and that `unrelated` is absent, without asserting an order between three equal scores). `run.rs`: `check_records_the_last_verdict` (`ENV_LOCK`, temp repo with a dirty file, `check`, then `Index::open` and `meta_get("last_verdict")` parses with `status == "block"` and `scope == "repo"`).
- [ ] Step 2: run, fail.
- [ ] Step 3: implement.
- [ ] Step 4: `cargo test --workspace` green; fmt; commit `engine: parameter counts on symbols, a symbol search, and the last verdict in meta`.

### Task 8: The five tools and `locrin mcp`

**Files:**
- Create: `crates/cli/src/mcp_tools.rs`
- Modify: `crates/cli/src/main.rs`, `crates/cli/src/run.rs` (`baseline_accept_as`), `crates/cli/Cargo.toml` (dep `locrin-mcp`)
- Test: `crates/cli/tests/mcp.rs`

**Interfaces:**
```rust
// crates/cli/src/run.rs
/// `baseline_accept` with the author named by the caller. The CLI passes the
/// login name; the MCP server passes "agent via mcp" so a reviewer can tell an
/// agent's sign-off from a person's.
pub fn baseline_accept_as(root: &Path, id: &str, reason: &str, author: &str, offline: bool) -> anyhow::Result<bool>;
// baseline_accept becomes a one-line wrapper that reads USERNAME/USER as today.

// crates/cli/src/mcp_tools.rs
pub struct Tools { root: PathBuf, offline: bool }
impl Tools { pub fn new(root: PathBuf, offline: bool) -> Tools }
impl locrin_mcp::Handler for Tools { .. }
```
The five tools (every result is one compact JSON object as the text content; every argument error is a `ToolError` naming the argument):

| name | inputSchema (properties, required) | behaviour |
| --- | --- | --- |
| `check_changes` | `paths: string[]` (optional, repo-relative or absolute), `base: string` (optional git ref). Both given -> `ToolError("paths and base cannot be combined")`. | `run::check(&Options { root, paths, changed_only: false, json: true, offline, diff: base.map(DiffScope::Base) })`; text = `locrin_reporters::agent::render(&verdict)` (verdict first, capped at ten, `truncated` count: spec 5.3). Engine error -> `ToolError(format!("{e:#}"))`. |
| `find_existing` | `intent: string`, `name: string`, `params: integer`, `returns: string` (all optional; at least one of `intent` or `name` required). | `symbols::search(&Index::open(root)?, &Query{..}, 10)`; for each match read line `start_line` of `root/rel` (`line_text` equivalent: read the file, take the line, trim, cap at 120 chars) as `summary`; text = `{"matches":[{"file","line","kind","name","params","exported","summary"}]}`. `returns` is accepted and ignored with a `"note":"return types are not indexed in version one"` field in the result. An index that does not exist yet -> `ToolError("no index: run locrin init or locrin scan first")` (check `cache_path(root).exists()` before opening, because `open` would create an empty one). |
| `explain_finding` | `id: string` (required) | `run::full_findings(&root, &Options{whole repo, offline}, true)` (make it `pub(crate)`), find `id`; absent -> `ToolError("no current finding with id <id>")`. Text = the finding's fields (`id, rule, category, severity, confidence, file, span, evidence, fix, related, owasp, cwe`) plus `"rule_description": <all_rules() entry's description()>` and `"accepted": {"reason","author","date"} | null` from `Baseline::load(root)`. |
| `accept_finding` | `id: string`, `reason: string` (both required, reason non-empty) | `run::baseline_accept_as(root, id, reason, "agent via mcp", offline)`; false -> `ToolError("no current finding with id <id>")`; true -> `{"accepted": true, "id", "baseline_entries": <count after>}`. |
| `status` | none | `{"index": {"path", "exists", "files": <all_files().len() or 0>, "schema": <SCHEMA_VERSION>}, "config": {"path", "present", "excludes": n, "rule_overrides": n, "boundaries": n}, "baseline": {"path", "entries": n}, "last_verdict": <parsed meta last_verdict or null>}`. Never creates the index. |

`main.rs`: `Cmd::Mcp { online: bool }` (`--online`: "Let vulnerable-dependency query osv.dev; off by default so a tool call never waits on the network") -> `locrin_mcp::serve(stdin().lock(), stdout().lock(), &mut Tools::new(root, !online), "locrin", env!("CARGO_PKG_VERSION"))`. `root` is the current directory or `--root` (Claude Code starts a project server in the project directory).

- [ ] Step 1: failing tests in `tests/mcp.rs`. A helper `session(dir) -> (Child, BufReader<ChildStdout>, ChildStdin)` that spawns `locrin mcp` in the fixture copy with `LOCRIN_CACHE_DIR`, and `rpc(&mut io, id, method, params) -> Value` that writes one line and reads one line. Tests: `initialize_then_list_names_the_five_tools` (names exactly `check_changes, find_existing, explain_finding, accept_finding, status`, each with an object `inputSchema`); `check_changes_returns_the_agent_verdict` (text parses, `status == "block"`, `findings.len() <= 10`, no `source` key); `check_changes_with_paths_narrows` (`src/clean.ts` -> `pass`); `check_changes_rejects_paths_with_base` (`isError`, text names both); `find_existing_ranks_by_name_tokens` (after a `check`, `name: "ok"` on the fixture returns `ok` from `src/clean.ts` with `line` and a `summary` containing `function ok`); `find_existing_without_an_index_says_so` (fresh cache dir, `isError`); `explain_finding_describes_a_finding` (take the first id from `check_changes`, explain: `rule_description` non-empty, `accepted == null`); `accept_finding_then_explain_shows_the_acceptance` (`author == "agent via mcp"`, `reason` echoed); `status_reports_the_index_config_and_last_verdict` (after a check: `index.exists == true`, `files >= 2`, `last_verdict.status == "block"`); `status_never_creates_an_index` (fresh cache dir, `status`: `exists == false`, and the cache path still does not exist afterwards); `stdout_carries_nothing_but_messages` (every line read during the session parses as JSON).
- [ ] Step 2: run, fail.
- [ ] Step 3: implement.
- [ ] Step 4: `cargo test --workspace` green; fmt; commit `engine: the mcp subcommand and its five tools`.

### Task 9: Scripted agent session, README, benchmarks, pull request

**Files:** `crates/cli/tests/mcp.rs` (modify), `README.md` (create), `docs/superpowers/plans/2026-09-10-init-hooks-and-mcp-measurement.md` (modify).

- [ ] Step 1: the scripted session of spec 10.3 as a test, `a_scripted_agent_reaches_a_clean_verdict_in_under_three_rounds`: on the fixture copy, `initialize`, `notifications/initialized`, `tools/list`; round 1 `check_changes {}` -> block, collect every finding id in the capped list; for each, `accept_finding` with reason `scripted session`; round 2 `check_changes {}` -> `status != "block"`; assert rounds used `<= 2`; `status` -> `baseline.entries == <accepted>`, `last_verdict.status` matches round 2. Add `a_stale_index_rebuilds_silently_for_the_server`: write an index at schema `'4'` (downgrade `meta` as the core test does), start the server, `status` reports `schema == "5"` and no error line reached stdout.
- [ ] Step 2: `README.md`: install (`cargo install --path crates/cli` for now; npm and Homebrew are phase two), `locrin init` and the files it writes, the three commands the hooks run and what Claude sees, `locrin check` / `scan` / `baseline` / `hook pre-commit` / `mcp`, the config keys (each with its default), the exit codes, `LOCRIN_CACHE_DIR`, the 21 rules with defaults in one table (id, category, severity, confidence, on by default) generated by hand from `all_rules()` so it is exact, and the two limits worth stating up front (no type checking; `already-exists` in release two).
- [ ] Step 3: re-run the full benchmark set (four runs, idle machine, first discarded) on the branch HEAD and record them in the measurement doc under Part B, beside Part A's, with a one-line ruling per gate. The hook benchmark from Task 5 is part of the set now.
- [ ] Step 4: append every deviation to this plan's section; commit `docs: Part B measurement, README`; `gh pr create --base main` from `engine/mcp`, title `MCP server with the five tools (plan 4 part B)`. Founder merges. Version one is complete when both parts are on `main`.

---

## Self-review against the spec

- 5.1 `init`: Task 4 covers detect (git, lockfile-free: package manager detection is not needed by anything init writes, so it is not performed; recorded here rather than pretended), hook entries, MCP entry, config with defaults, first index, baseline, printed files. Section 9's first-scan follow-up lands in step 5.
- 5.2 moment 1: Task 8 `find_existing`; moment 2: Task 1 (single file, blocking as `decision: block`, advisory as `additionalContext`, 300 ms measured in Task 5, 2 s watchdog from section 9); moment 3: Task 2 (working tree against HEAD, three rounds in a session file, summary for the human).
- 5.3: `hook/text.rs` and `agent::render` (Tasks 1, 8): rule, location, one line of evidence, one line of fix, related on the JSON form; ten-finding cap with the rest counted; verdict first.
- 5.4: Task 3 pre-commit; MCP is agent-agnostic by construction.
- 6: Task 8, all five tools, ranking in Task 7 (deviation recorded there).
- 7.2 to 7.4: reused as is; pre-commit uses the exit codes.
- 9: watchdog (Tasks 1 and 2), engine error to warning (Tasks 1 and 2), schema rebuild (Task 7, tested in Task 9).
- 10.3: replayed payloads (Tasks 1 to 3), protocol conformance (Task 6), scripted session (Task 9).
- 11 phase 2: every item named is a task above; `baseline` existed already.
- Type consistency: `run::Options` fields are the ones in `run.rs` today (`root, paths, changed_only, json, offline, diff`); `Verdict::capped`, `agent::CAP`, `DiffScope::Base`, `Index::open`, `cache_path`, `Language::from_path`, `canonical_path`, `source_files`, `WalkOptions { excludes }`, `Baseline::load`, `all_rules()`, `description()` all exist with these names. New names are introduced once each: `hook::{Input, ToolInput, read_input, watchdog, emit, post_edit, stop, pre_commit, HOOK_BUDGET_MS}`, `hook::text::feedback`, `hook::session::{Session, MAX_ROUNDS}`, `git::{has_head, staged_files}`, `init::{run, Report, Touch, merge_settings, merge_mcp, CONFIG_TEMPLATE, PRE_COMMIT_SCRIPT}`, `locrin_mcp::{ToolSpec, ToolError, Handler, serve, handle, PROTOCOL_VERSIONS}`, `Index::{meta_get, meta_set}`, `symbols::{Query, Match, search, tokens, STOPWORDS}`, `run::{baseline_accept_as, full_findings (pub(crate))}`, `mcp_tools::Tools`.
