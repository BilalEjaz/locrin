# Pull request record (original repository, before the 2026-09-13 recreate)

The repository was recreated on 2026-09-13 to purge cached views of pre-rewrite commits. This file preserves the description of every pull request from the original repository. Numbers below are the original numbers; later plans and notes that cite a pull request number refer to this list.

## PR 1: Fingerprinting spike: result and thresholds

State: merged, merged: 2026-09-06T08:19:32Z, branch: spike/fingerprint into main

## Headline
With signature gate: precision 0.69 at t=0.70 (recall 0.91, 116 labelled pairs predicted at t)
Without signature gate: precision 0.69 at t=0.70 (recall 0.91, 116 labelled pairs predicted at t)
Population-weighted precision at the chosen threshold: 0.61 (sample-pooled 0.69)


Verdict: provisional FAIL against the 0.85 bar. `already-exists` moves to release two (spec 4.2). Version one launches on the remaining rules.

Founder actions before merge:
1. Score `spike/fingerprint/SPOTCHECK-50.md` (mark each pair d / n / u). Below 90 percent agreement with the committed labels in `spike/fingerprint/labels-2026-09-05/`, all 240 pairs are relabelled and the report regenerated.
2. Delete `SPOTCHECK-50.md` from the tree once scored; it embeds product source.

Contents: market research, approved design spec (updated with the spike result), the spike plan, and the throwaway Python spike (69 tests). Full write-up in `spike/fingerprint/REPORT.md`.

## PR 2: Engine core implementation plan (v1, plan 1 of 4)

State: merged, merged: 2026-09-06T10:23:44Z, branch: plan/engine-core into main

Implementation plan for the version-one engine core, plan 1 of 4, argued from the approved design spec (sections 3, 4.1, 7, 9).

Scope: Rust workspace (core, rules, reporters, cli), tree-sitter parsing, SQLite index with content-hash incremental updates, top-level symbols, finding and verdict contract, config and baseline files, three leftover rules (debug, commented-code, agent-marker), terminal and agent JSON reporters, `gate check / scan / baseline`, and benchmark tests for the spec's performance targets. Graph rules, security pack, hooks and MCP are plans 2 to 4. `already-exists` is release two per the spike result.

Stacked on `spike/fingerprint` (PR #1); retarget to main after #1 merges.

Blocked on: Rust toolchain is not installed on the build machine (Task 1 stops with NEEDS_CONTEXT until rustup is installed).

## PR 3: Plan 2 of 4: graph rules and incremental check

State: merged, merged: 2026-09-09T07:00:20Z, branch: plan/graph-rules into main

Implementation plan for Locrin plan 2: import graph in the index, unused-import, unreachable, dead-export, dead-file, boundary-violation (part A, branch engine/graph), then findings cache, check --base/--since, and SARIF (part B, branch engine/incremental). 19 tasks, TDD, fixtures per rule, precision gate on FastLift before part A merges. Spec: docs/superpowers/specs/2026-09-05-agent-native-quality-gate-design.md

## PR 4: Locrin engine: import graph and five rules (plan 2 part A)

State: merged, merged: 2026-09-09T07:00:24Z, branch: engine/graph into main

Plan 2 Part A on top of PR #3 (stacked; retarget to main once #3 merges, then merge with --delete-branch).

What landed (47 commits, 145 tests green):
- Index schema v3: edges, allow_lines, findings_cache, export names on symbols, size/mtime on files.
- Import extraction (static, re-export, dynamic, require) with a text fallback for parse-failed files; module resolver (relative, tsconfig paths/baseUrl with longest-prefix match, workspaces, extension probing, never guesses); entry-point detection (package.json main/bin/exports/jest setup, app.json plugins, wrangler main, framework globs); config sections entry_points and [[boundaries]].
- Rule contract gains Scope (File vs Graph) and enabled_by_default; spec 9 gate enforced via the index for files not parsed this run.
- Five rules: unused-import (Low/High), unreachable (Medium/High), dead-export (Low/Medium), dead-file (Medium/Medium, ships OFF by default, see below), boundary-violation (High/High). Fixtures per rule: must-flag, must-not-flag, edge.
- Whole-repository walk on every run, explicit paths as a scope, one transaction per run, parallel parse and walk, stat-based change shortcut, baseline commands index into memory so a fresh-cache baseline holds every graph finding.

Precision on FastLift (docs/superpowers/plans/2026-09-08-graph-rules-precision.md): unused-import 20/20, dead-export 20/20, dead-file 6/9 after fixes (below the 85 percent gate; the remaining misses are entry points named outside JavaScript), unreachable and boundary-violation produced no findings on this corpus.

Benchmarks (release, FastLift, three runs): cold index 2418 to 3171 ms (target 5000), warm single-file check 175 to 243 ms (target 300), startup 29 to 35 ms (target 50).

Decisions taken during execution are listed under "Deviations recorded during execution" in the plan document; the one for the founder: spec 4.1 does not yet say dead-file ships off by default.

## PR 5: Locrin engine: findings cache, git scope, SARIF (plan 2 part B)

State: merged, merged: 2026-09-09T08:35:50Z, branch: engine/incremental into main

Plan 2 Part B (Tasks 16 to 19 plus controller-added 16b). 22 commits, 168 tests green, final whole-branch review clean after one fix wave.

- Findings cache per file keyed by content hash and config hash; unchanged files served on whole-repository runs; scan warms the cache. Warm full check on FastLift: 6155 ms to about 205 ms.
- Neighbour scope: narrowed runs report graph findings for the files whose edges touch the scope; the index watermark widens --changed only, so a PR verdict is a function of the tree and the ref (recorded in spec 3.2).
- File rules run per file in parallel (Rule: Sync, RuleContext.index: Option<&Index>, run_file_rules).
- check --base <ref> (PR view, working tree plus untracked) and check --since <ref> (commits only); refs shaped like options are refused and --end-of-options is passed; paths conflict with --changed/--base/--since.
- check --sarif: SARIF 2.1.0, uncapped, every rule listed with defaultConfiguration.enabled.
- Benchmarks (release, FastLift): cold 3.0 to 3.3 s (target 5), warm single-file 138 to 147 ms (target 300), warm 30-file 191 to 211 ms (target 1000), startup 17 to 23 ms (target 50).

Deviations with reasons are in the plan document under "Deviations recorded during execution", Part B.

## PR 6: Locrin engine: swallowed-error, test rules, previous-state plumbing (plan 3 part A)

State: merged, merged: 2026-09-09T12:37:43Z, branch: engine/erosion into main

Plan 3 Part A. 29 commits, workspace green, final whole-branch review clean after one fix wave.

- [framework] config section (auth_middleware, server_paths); RuleContext gains root, offline, previous; Rule::locked; --offline on check and scan.
- Test-case extractor in core (cases, skipped, assertions incl. same-file helpers, Testing Library throwing queries, throw statements, body as the last function argument).
- Schema 4: skipped_tests (plus the two OSV tables Part B uses); Previous captured from the index for changed, unchanged-parsed, and repair-pass files, and from git for --base/--since (git pinned to LC_ALL=C).
- Rules: swallowed-error (three forms; ships OFF by default: 0 true positives across 116 corpus findings, all deliberate ignores or non-rejecting promises), test-no-assert (on; zero corpus findings after counting throwing queries and throw), test-newly-skipped (on).
- Precision report docs/superpowers/plans/2026-09-09-error-test-and-security-precision.md. Benchmarks: cold 3750-3790 ms, warm 148-215 ms, warm-30 222-247 ms, startup 17-18 ms.
- Deviations with reasons under "Deviations recorded during execution" in the plan document.

## PR 7: Locrin engine: security pack (plan 3 part B)

State: merged, merged: 2026-09-10T07:00:02Z, branch: engine/security into main

Plan 3 Part B. 42 commits, 338 tests green, final whole-branch review clean after one fix wave.

- secret-exposed (locked: always blocks, cannot be disabled): 159 provider patterns, entropy gate, Supabase JWT role decoder, private-key material anchoring, evidence masked; zero findings on the five corpus repos.
- weak-crypto, injection-sink (ships OFF by default: 2 of 28 corpus findings actionable), html-injection.
- Lockfile parsers (npm, yarn v1, pnpm) and an OSV client with an on-disk snapshot: the engine's only network call, off under --offline (check, scan, baseline), fresh snapshots reused, never blocks; vulnerable-dependency with range-correct fixed versions (54 advisories on FastLift, 0 downgrades), runs on whole-repo runs or when the scope names the lockfile.
- supabase-service-role-in-client and supabase-table-without-rls (reported on whole-repo runs or when the scope names a migration); three Express rules (unmeasured: no corpus Express app).
- Precision report Part B in docs/superpowers/plans/2026-09-09-error-test-and-security-precision.md. Benchmarks against a same-session base control: warm single-file 178 to 184 ms (target 300), warm 30-file about 300 ms (target 1000), startup 19 to 23 ms (target 50); cold 8.9 to 10.0 s against a base control of 9.1 to 9.4 s on a box that measured 3.0 s two days ago (delta +5 percent; target not lowered; re-measure on a quiet machine).
- Deviations with reasons under "Deviations recorded during execution" in the plan document; spec 4.1 records the injection-sink default and CWE-942.

## PR 8: Cold benchmark re-measured on a quiet box; SPOTCHECK-50 scored

State: merged, merged: 2026-09-10T08:01:35Z, branch: docs/remeasure-and-spotcheck into main

Two open items from plan 3, both docs only.

**Cold benchmark, quiet box, on main c26c1e6.** Four sequential runs, fresh cache each, machine idle. Cold 25938 (discarded first run, page cache) / 3815 / 3810 / 3787 ms against 5000; warm single-file 171 to 187 ms against 300; warm 30-file 269 to 290 ms against 1000; startup 18 to 20 ms against 50. Every gate green; Part B's cold cost on a quiet box is inside 65 ms of Part A's numbers. The 9 to 10 s recorded on 2026-09-10 was the machine, as the ledger argued. Section added to the precision report. Follow-up noted for plan 4: a genuinely cold OS cache costs 26 s on an 1846-file repo, so the first scan wants a progress line.

**SPOTCHECK-50.** Scored blind by Claude in a fresh session (labels not in context), not by the founder: 49 of 50 agree with data/labels.jsonl (98 percent, above the 90 percent relabel bar; 48 of 49 with PAIR 3 excluded, whose stored label was seen by accident before scoring). The one disagreement is PAIR 153, two haptic wrappers differing only in the feedback-type constant, marked not-dup here under the report's one-line-wrapper tie-breaker and labelled dup in the set. Verdicts written next to each PAIR heading, note at the top of the file. The founder's own blind pass stays open if wanted.

## PR 9: Plan 4: init, Claude Code hooks, pre-commit hook, MCP server

State: merged, merged: 2026-09-10T08:43:16Z, branch: plan/init-hooks-mcp into main

The last plan of version one, written from spec sections 3, 5, 6, 9, 10.3 and 11 against the Claude Code hooks reference and the MCP 2025-06-18 specification as they read today (the exact contracts are quoted at the top of the plan).

**Part A, branch engine/agent (Tasks 1 to 5):** `locrin hook post-edit` (single-file check, 2 s watchdog, `decision: block` for blocking findings, `additionalContext` for advisories, always exit 0), `locrin hook stop` (working tree against HEAD, three rounds per session tracked beside the index, then a summary for the human), `locrin hook pre-commit` (staged files, verdict exit code), and `locrin init` (config template, idempotent merges into `.claude/settings.json` and `.mcp.json`, pre-commit script only when nothing owns it, first scan with a progress line, baseline). Hook payloads are replayed from recorded fixtures. New benchmark: the PostToolUse hook under 300 ms warm.

**Part B, branch engine/mcp (Tasks 6 to 9):** the `mcp` crate spec 3 names, holding only newline-delimited JSON-RPC over stdio (`initialize`, `tools/list`, `tools/call`, `ping`) with conformance tests; schema 5 adds a parameter count to symbols; `symbols::search` ranks by parameter count then name tokens (the honest version-one reading of "signature vector", recorded as a deviation); `run::check` records `last_verdict` in the index; the five tools; a scripted agent session test that must reach a clean verdict in under three rounds; README.

No new external crates. Corpus checkouts are never written into (init is dogfooded on a temporary copy of fastlift-admin).

## PR 10: Init, Claude Code hooks, pre-commit hook (plan 4 part A)

State: merged, merged: 2026-09-10T14:02:36Z, branch: engine/agent into main

Plan 4 Part A: the agent-facing surface. Plan: docs/superpowers/plans/2026-09-10-init-hooks-and-mcp.md (Tasks 1 to 5). Measurement and dogfood: docs/superpowers/plans/2026-09-10-init-hooks-and-mcp-measurement.md.

**What lands**

- `locrin hook post-edit`: the Claude Code PostToolUse hook. Reads the payload on stdin, checks the one edited file, and answers with `decision: block` for blocking findings, `additionalContext` for advisories, nothing when clean. Two-second watchdog; a timeout, an engine error or a crash becomes a `systemMessage` and a pass. Always exit 0, stdout is one JSON object or nothing.
- `locrin hook stop`: the Stop hook. Checks the working tree against HEAD (or `--changed` without a HEAD), sends the agent back at most three times per session (counter beside the index, never in the repo), then tells the human once and stands down. A clean verdict resets the counter.
- `locrin hook pre-commit`: checks the staged files; exit code is the verdict's, no watchdog.
- `locrin init`: config template, idempotent merges into `.claude/settings.json` and `.mcp.json`, pre-commit script only when nothing else owns it (hooks directory resolved through git, so worktrees and `core.hooksPath` work), validation of every file before the first write, first scan with a progress line, baseline. Prints every file it touched, even when a later step fails.
- Hook payloads are replayed from recorded fixtures (spec 10.3); every hook and init has end-to-end tests through the binary.

**Benchmarks** (idle machine, four runs, first discarded as the page-cache run, all reported in the measurement doc): cold index 3866 to 3888 ms against 5000; warm single-file 182 to 186 ms against 300; PostToolUse hook end to end 182 to 185 ms against 300; warm 30-file 267 to 288 ms against 1000; startup 18 to 19 ms against 50. Every gate green.

**Dogfood**: `init` on a temporary copy of fastlift-admin wrote all five files (baseline 33 findings), the replayed post-edit blocked on an injected `console.log`, four stop rounds gave rounds 1, 2, 3 and then the cap message, and a second `init` read `unchanged` on every line. The copy was deleted; the original was not touched.

**Rulings taken during execution** (all recorded in the plan's deviations section): session ids are percent-encoded before they name a file; a pre-commit script init wrote itself reads `unchanged`, a foreign one `skipped`; a staged non-source file still reaches the check so the lockfile and migration rules gate commits; the hooks directory comes from `git rev-parse --git-path hooks` rather than the plan's literal `.git/hooks`; `init` run in a subdirectory installs into the enclosing repository and prints the absolute path.

**Known and deferred**: `.mcp.json` names `locrin mcp`, which Part B adds, so no release should be cut from main between the two parts. The post-edit hook ignores files the engine does not parse (a lockfile edit is gated at Stop, not at edit); widening that is a follow-up. Session files are never pruned. Full list in the ledger's deferred minors, triaged in the final review.

No new external crates. Corpus checkouts were never written into.

## PR 11: MCP server with the five tools (plan 4 part B)

State: merged, merged: 2026-09-10T14:02:41Z, branch: engine/mcp into main

Plan 4 Part B: the MCP server, and with it the end of version one. Stacked on engine/agent (PR #10); retarget to main once #10 merges. Plan: docs/superpowers/plans/2026-09-10-init-hooks-and-mcp.md (Tasks 6 to 9). Measurement: docs/superpowers/plans/2026-09-10-init-hooks-and-mcp-measurement.md (Part B section).

**What lands**

- `crates/mcp`: the protocol crate the spec names, written by hand against MCP 2025-06-18 with no SDK and no async runtime. Newline-delimited JSON-RPC over stdin and stdout: `initialize`, `notifications/initialized`, `ping`, `tools/list`, `tools/call`. A line that is not JSON or not UTF-8 gets a parse error and the session continues; blank lines are ignored; a tool that panics becomes an `isError` result and the server stays up. Stdout carries protocol messages only.
- Schema 5: a parameter count on every callable symbol, `symbols::search` (parameter count first, name tokens second, no LLM), `meta_get`/`meta_set`, and `check` recording its last verdict in the index.
- `locrin mcp [--online]` with the five tools from spec 6: `check_changes` (paths or a base ref, the capped agent verdict), `find_existing` (intent and/or a name and parameter count, ranked symbols with a one-line summary; refuses to answer from a missing or empty index rather than saying "nothing exists"), `explain_finding`, `accept_finding` (author `agent via mcp`), `status` (index, config, baseline, last verdict; never creates an index). Offline unless `--online`.
- Tests: protocol conformance on the crate; every tool end to end through the real server process; the spec 10.3 scripted agent session, which must reach a clean verdict in exactly two rounds and proves it accepted something; a stale-index rebuild that stays silent on stdout.
- README at the repository root: install, init, the hooks and what Claude sees, every command, every config key with its default, exit codes, the 21 rules in one exact table, and the two stated limits.

**Benchmarks** (four runs, first discarded, all reported): cold index 4403 to 4716 ms against 5000; warm single-file 270 to 293 against 300; PostToolUse hook 274 to 298 against 300; warm 30-file 381 to 398 against 1000; startup 21 to 28 against 50. Every gate green, but read the section: the box was on battery on the Balanced power scheme and the whole set is about 50 percent slower than Part A's numbers, `--help` included, so two gates are thin for a reason that is not in the code (Part B does not touch the hook or the single-file path). Re-measure on mains before reading anything into the margins.

**Rulings taken during execution** (all in the plan's deviations section): a message with neither method nor id gets an invalid-request error; an explicit null id is a notification; the "signature vector" of spec 6 is the parameter count in version one, recorded for release two to replace; `find_existing` refuses an empty index; `status` is described as never creating an index rather than never touching one.

**Known and deferred** (triaged in the final review): `status` on a stale-schema index rebuilds it on the way to counting files; the summary line in `find_existing` reads a whole file for one line; the camelCase tokeniser does not split after a digit. Full list in the ledger.

No new external crates. Corpus checkouts were never written into.

## PR 12: Plan 5: dogfood cleanup

State: merged, merged: 2026-09-10T14:35:44Z, branch: plan/dogfood-cleanup into main

Fixes the four engine defects the first real locrin init on FastLift surfaced: colliding finding ids (advisory anchors without the version; line-rule anchors without the line), a NUL byte ending the parse, a bare ampersand in JSX text excluding a whole file, and init parsing the repository twice. Five tasks, TDD, one benchmark re-run. Ids change, so baselines must be recreated (the README will say so).

## PR 13: Dogfood cleanup: ids, NUL bytes, JSX ampersands, one scan in init (0.2.0)

State: merged, merged: 2026-09-10T17:17:40Z, branch: engine/cleanup into main

Plan 5: the four engine defects the first real `locrin init` on FastLift surfaced. Plan: docs/superpowers/plans/2026-09-10-dogfood-cleanup.md. Measurement: the `## Dogfood cleanup` section of docs/superpowers/plans/2026-09-10-init-hooks-and-mcp-measurement.md.

**This release is 0.2.0 and every finding id changes.** A baseline written by 0.1.0 must be recreated with `locrin baseline create`; the findings cache is rebuilt on first run (the cache key carries the crate version). The FastLift baseline is uncommitted, so nothing shipped breaks.

**What lands**

- Finding ids no longer collide. A line rule's anchor now carries the line's trimmed text and its ordinal among identical lines in the enclosing symbol (still no line number, so ids survive line shifts); an advisory's anchor carries the installed version. On FastLift: 778 findings became 756 baseline entries under 0.1.0; under this branch 766 findings give 766 distinct ids offline (820 of 820 online). Rules that anchor on a class of their own (a secret value, a route, a table) keep one id per repeated occurrence by design.
- A NUL byte in source no longer ends the parse. tree-sitter's lexer reads byte 0 as end of input; the parser now gets a copy with 0x01 in its place, one byte for one byte, while every reader keeps the file as written. Proven on both grammars.
- A bare `&` in JSX text no longer excludes the file. The grammar's scanner refuses `BODY & NUTRITION` (tree-sitter-javascript #366, open upstream); the resulting ERROR node among an element's text children is tolerated when its text holds no `<`, `{` or `}` and none of its children carry an error. MISSING nodes are never tolerated, so `as import('./t').Seg[]` (tree-sitter-typescript #322) still excludes its file with one warning, and the README documents the workaround.
- `init` writes its baseline from the scan it just ran instead of a second cold parse of the whole repository (every warning printed once; the CLI's `baseline create` keeps plan 2's non-recording design).
- On FastLift, read-only: the four fixed files now parse and yield findings (3, 1, 1, 0); only `historyPort.guard.test.tsx` still warns.

**Benchmarks.** Warm single-file 184 to 195 ms, PostToolUse hook 181 to 189 ms, warm 30-file 328 to 349 ms, startup 19 to 20 ms: all green, on mains. The cold index gate FAILED in the mandated four-run set (9857 to 10033 ms against 5000, with a 46-second discarded first run), then measured 5072 and 5587 ms in isolation, and `main` at 7a6f878 measured 5524 and 6275 ms side by side against this branch's 5592 and 5383 ms. The branch is not the cause; the machine was slow for the whole window. The target was not lowered. Re-measure on a quiet box before reading anything into it.

**Rulings taken during execution** (in the plan's deviations section with their costs): version bump to 0.2.0; the NUL fixture bucket is `nul_byte` (a directory named `nul` is a reserved Windows device name); the tolerated shape follows the probed tree rather than the plan's description; the FastLift tracker line was written by the controller rather than the task, because the task rules forbid writing into that checkout.

No new crates. Corpus checkouts were never written into (the FastLift baseline was moved aside and restored byte for byte).

## PR 14: Version-one follow-ups: --version, baseline stamp, hook budget override, docs, symbol cache, bench warm-up

State: merged, merged: 2026-09-11T09:07:52Z, branch: fix/v1-follow-ups into main

Version-one follow-ups, six bounded items plus one review fix, 469 tests, benchmarks green (cold 4.1 s, warm 183 ms, hook 222 ms, warm-30 296 ms, startup 20 ms).

- `locrin --version`.
- Baseline file is stamped with the engine version; loading a file written by another version (or a pre-0.2.0 file with no stamp) warns once per load and suggests `locrin baseline create`. Note: a long-lived MCP server loads the baseline per tool call, so the warning repeats there until the file is refreshed. FastLift's current baseline will warn until re-created.
- `LOCRIN_HOOK_BUDGET_MS` overrides the 2 s post-edit watchdog (zero or garbage ignored); the hook tests scrub it from the child env; the watchdog unit test follows the override.
- README: stray `>` in JSX text is tolerated like a bare `&`; the exclusion needs `<`, `{` or `}` in the text run (the brief's `& word.` claim was wrong and the doc now says what the parser does, pinned by tests).
- `anchor_for` shares one symbol table per parsed file (OnceLock, rayon-safe); the indexer populates it so rules reuse it.
- Cold benchmark does an untimed warm-up (`--version` plus a scan into a throwaway cache) before the timed cold index, so first-spawn-after-compile costs no longer fail the gate; the timed index is still built from empty.

## PR 15: Phase two plan A: GitHub Action, release pipeline and distribution

State: merged, merged: 2026-09-11T09:07:57Z, branch: plan/ci-distribution into main

Phase two, plan A of two, argued from spec sections 8.1, 8.2, 7 and 11.

Seven tasks on branch engine/ci: a Markdown summary reporter for pull-request comments; `locrin check --markdown` and `--sarif-file PATH` so one run feeds the comment, the SARIF upload and the exit code; a CI workflow for the engine (fmt, clippy -D warnings, tests on Ubuntu and Windows, plus an action smoke job against the fixture repo); a tag-triggered release workflow with a four-target build matrix and SHA256SUMS, dry-runnable by dispatch; a composite action under action/ that downloads a release by checksum (or uses a local build), edits one marker comment in place, uploads SARIF, and fails the step on block; example workflows for the pull-request view, the deployment gate and FastLift; README sections; and an end-to-end proof of the comment path on the branch's own PR.

Version bumps to 0.3.0. The npm shim and Homebrew formula are not in this plan. Plan B (PHP and Python behind the config flag) follows separately because the grammar crates need a tree-sitter runtime bump.

## PR 16: Phase two A: GitHub Action, release pipeline, Markdown and SARIF file output

State: merged, merged: 2026-09-11T11:23:17Z, branch: engine/ci into main

Phase two plan A, executed via subagent-driven development on branch engine/ci. Opened early so pull-request events run the new CI workflow (workflow_dispatch is unavailable until the workflow is on main).

Delivered so far: Markdown summary reporter; `locrin check --markdown` and `--sarif-file PATH`; ci.yml (fmt, clippy -D warnings, tests on Ubuntu and Windows); release.yml (four targets, SHA256SUMS, dry-run dispatch; x86_64 macOS builds on macos-15-intel because macos-13 is retired); version 0.3.0. Remaining tasks: composite action with smoke job, examples and README, end-to-end comment proof on this PR.

After merge the founder tags v0.3.0 to publish the first release assets.

## PR 17: ci: temporary install proof against the v0.3.0 release

State: closed, merged: no, branch: ci/install-proof into main

Temporary. Proves the composite action's Install step (release download by checksum, extract, run) on ubuntu, windows and macos using the real v0.3.0 assets. Closed without merging once green; the workflow file is not meant for main.

## PR 18: Phase two plan B: PHP and Python behind the flag

State: merged, merged: 2026-09-11T12:23:13Z, branch: plan/languages into main

Phase two, plan B of two, argued from spec sections 1 (languages v2), 4.3 (precision gate), 9 and 11.

Nine tasks on a future branch engine/languages: tree-sitter runtime 0.23 to 0.24 (trialled today: existing grammar compiles, all tests pass) plus the PHP and Python grammar crates; `[languages] php = true / python = true` in locrin.toml gating the walker and explicit paths, default off so nothing changes for existing users; PHP and Python top-level symbols; a `Rule::languages()` declaration with a per-rule file filter so every TypeScript-only rule stays silent on the new languages; leftover-debug sinks (var_dump, dd, breakpoint, pdb; print is deliberately not a sink) and commented-code vocabularies per language; Composer, Poetry and pinned requirements lockfiles with per-package OSV ecosystems; a 20-sample precision gate per rule-language pair on FastSpot (Python) and a BookStack clone (PHP), shipping any failing pair off for that language; benchmarks, README and SARIF metadata. Version 0.4.0.

## PR 19: Phase two B: PHP and Python behind the flag (0.4.0)

State: merged, merged: 2026-09-11T17:45:07Z, branch: engine/languages into main

Phase two plan B executed via subagent-driven development: 21 commits, 532 tests, version 0.4.0.

PHP and Python behind `[languages]` in locrin.toml (default off; walker, explicit paths, hooks and pre-commit gated). tree-sitter runtime 0.24 with the PHP and Python grammars. Symbols for both languages. `Rule::languages()` with a structural guarantee that only five rules speak on the new languages (input filter plus finding filter, verified empirically with every rule enabled). Per-language debug sinks and commented-code vocabularies. Composer, Poetry and pinned requirements lockfiles with per-package OSV ecosystems, PEP 503 names, honest wording for non-SEMVER ranges, one finding per advisory family. `Rule::enabled_for(Language)` from a three-round precision gate on BookStack, Monica, Poetry and FastSpot (report in docs/superpowers/plans/2026-09-11-php-and-python-precision.md): leftover-commented-code ships off on Python; everything else on. Rules fingerprint in the findings cache key. Release notes in docs/RELEASE-NOTES.md, including what changes for repositories that do not opt in.

Existing TypeScript finding ids verified unchanged against a real 0.3.0 binary. Tag v0.4.0 after merge publishes the assets; examples' pins move to v0.4.0 in a follow-up.

## PR 20: docs: pin the action examples to v0.4.0

State: merged, merged: 2026-09-11T17:51:47Z, branch: docs/pin-0.4.0 into main

Follow-up promised in the 0.4.0 release notes: the example workflows and both READMEs pin the action ref and the binary version to v0.4.0.

## PR 21: Advisory range ordering for PyPI and Composer; [rules.<id>].languages override

State: merged, merged: 2026-09-11T20:41:43Z, branch: fix/advisory-ranges-and-language-override into main

Two engine follow-ups from the phase two reviews, one Fable review plus a fix round, 547 tests.

- PyPI and Packagist advisory ranges are now ordered with PEP 440 and Composer comparators, so those findings name the version to move to. Verified online on the Poetry and Monica corpora: 81 of 81 registry findings name a fix, every spot-checked fix is strictly above the installed version and agrees with the advisory's own ranges. Unparseable boundaries make a range unreadable; the engine never guesses.
- `[rules.<id>].languages` config override: runs a rule on exactly the listed languages it supports, bypassing the per-language default (turns commented-code on for Python). Unknown, unsupported or empty lists are config errors; the locked secrets rule refuses the key. Folded into the cache fingerprint and SARIF rule metadata.

Unreleased section added to docs/RELEASE-NOTES.md; no version bump yet.

## PR 22: spec: public launch design

State: merged, merged: 2026-09-11T23:40:29Z, branch: spec/public-launch into main

Design spec for the first phase-three sub-project: the repository goes public under MIT, one-line install via npm, PyPI, Homebrew, curl and cargo, and a public reproducible benchmark in its own repository. Decisions were taken in conversation on 2026-09-12; this PR holds the written spec for founder review before the implementation plan.

## PR 23: plan: public launch A (pre-flight and install channels)

State: merged, merged: 2026-09-12T11:39:53Z, branch: plan/public-launch-a into main

Implementation plan for sections 3, 4, 6 and 7 of the public launch spec: history secret scan, private reference check, licence and policy files, npm and PyPI platform packages, Homebrew formula generator, curl and PowerShell installers, CI install smoke, the publish stage of the release workflow, README rewrite, and the 0.5.0 bump. The benchmark (spec section 5) is plan B. Nothing in this plan flips visibility, tags, or publishes.

## PR 24: Public launch A: pre-flight and install channels

State: merged, merged: 2026-09-12T19:33:25Z, branch: launch/preflight-and-install into main

This branch is the pre-flight for making Locrin public and the install story that
goes with it. It scans the whole history for secrets, scrubs private references
out of the tree and adds a checker that keeps them out, adds the files a public
repository needs, builds five install channels (npm, PyPI, Homebrew, crates.io
and the two installer scripts) from one release, smoke-tests those channels in
CI on three operating systems, teaches the release workflow to publish them,
rewrites the action and its examples for a public repository with no download
token, rewrites the README for a first-time reader, and bumps the workspace to
0.5.0 with release notes. Nothing here flips the repository to public and
nothing here tags or publishes anything.

## Per task

- Task 1: packaging moved into one shared script (`scripts/package.sh`) with a
  test, so the release workflow and a local build produce the same archive
  layout and the same `SHA256SUMS`.
- Task 2: crates.io metadata on every workspace crate, the binary crate renamed
  from `locrin-cli` to `locrin` so `cargo install locrin` works, and
  `scripts/check-versions.sh` to keep the path dependency versions in step with
  the workspace version.
- Task 3: `scripts/scan-history.sh` scans every commit with the engine's own
  secret rule and with gitleaks, allowlists the secret rule's synthetic fixtures
  through `scripts/scan-history-allow.txt` (exact entry matching), and fails
  closed when gitleaks errors, scans partially or exits 1 with no findings.
- Task 4: `scripts/check-private-refs.sh` fails the build on local paths,
  personal addresses and token prefixes; it runs in CI, every existing hit in
  the plans and the bench was scrubbed, and the three `spike/fingerprint/` data
  files were untracked.
- Task 5: MIT licence, security policy, contributing guide and issue templates,
  with the placeholders retired and a test that the templates stay valid YAML.
- Task 6: npm packaging, one `locrin` entry package that resolves the binary out
  of per-platform packages, plus a staging script and tests.
- Task 7: PyPI platform wheels that carry the binary, built without setuptools,
  with the Unix host marker so pip keeps the executable bit.
- Task 8: a Homebrew formula generator that reads the release checksums, so the
  tap formula is derived from `SHA256SUMS` rather than hand written.
- Task 9: `install.sh` and `install.ps1`, both verifying the download against
  `SHA256SUMS`; an unlisted platform exits 2, `latest` resolves on PowerShell
  5.1, and the failure path keeps the terminal open under `iex`.
- Task 10: a CI install-smoke job that installs from npm, PyPI, the two
  installers and the formula on ubuntu-22.04, macos-14 and windows-2022 and runs
  the binary each way.
- Task 11: the publish stage of `release.yml` pushes npm, PyPI, crates.io and the
  Homebrew tap from one run, with a dry-run job, a skip guard that queries each
  registry before publishing, per-crate verification on a live publish, and
  re-runs that re-upload assets.
- Task 12: the action and its three examples rewritten for a public repository,
  no download token anywhere, and the token wording corrected in the action
  README.
- Task 13: the README rewritten for a first-time reader: what Locrin is, the
  install channels, what is free and what is paid, and the benchmark.
- Task 14: workspace bumped to 0.5.0 (path dependency versions and `Cargo.lock`
  with it), 0.5.0 release notes written with a fresh empty `Unreleased` heading
  above them, and the action pins moved from `v0.4.0` to `v0.5.0`.

## Pre-flight history scan

`bash scripts/scan-history.sh` exits **0** on this branch (measured at
`43e8f56`; the commits that landed since are docs, packaging and workflow
changes, and the scan is re-run from main before the flip).

- 315 commits walked by part 1 (the engine's `secret-exposed` rule per commit
  over `git rev-list --all`), 290 commits scanned by part 2 (gitleaks 8.30.1).
- Every hit falls under the synthetic fixture allowlist
  `scripts/scan-history-allow.txt`: 26857 allowlisted from part 1 and 181 from
  part 2.
- Zero other hits, in either half. Nothing had to be widened to reach exit 0.

## Private reference sweep

FastLift files check, confirming the dogfooding artifacts never entered this
repository:

```
$ git log --all --diff-filter=A --name-only --pretty=format: | sort -u \
    | grep -E 'locrin-baseline\.json|locrin\.toml' || echo "none"
none
```

The checker on the real tree, after the sweep and the untracking:

```
$ bash scripts/check-private-refs.sh; echo "CHECKER EXIT=$?"
CHECKER EXIT=0
```

## CI install smoke

Two runs, both green on ubuntu-22.04, macos-14 and windows-2022:

- https://github.com/BilalEjaz/locrin/actions/runs/34707292887, measured at
  `7a0efb7`, the run that landed the install-smoke job in Task 10.
- https://github.com/BilalEjaz/locrin/actions/runs/34711065288, measured at
  `ab0f5a5`, which ran install-smoke on the 0.5.0 archives.

## Release dry run

Run https://github.com/BilalEjaz/locrin/actions/runs/34709763816 is green,
measured at `231ac51`, so its artifacts are still 0.4.0-named. It is re-run
from main after this merge, before any tag.

## Before the flip

Three spike files were untracked in this branch because they quote private
source: `spike/fingerprint/SPOTCHECK-50.md`,
`spike/fingerprint/labels-2026-09-05/labels.jsonl` and
`spike/fingerprint/labels-2026-09-05/sample_keys.json`. Untracking them takes
them out of the tip, but they still exist in git history, so the founder has to
decide on a history rewrite before the repository goes public.

## Not in this PR

Nothing here flips visibility, tags a release, or publishes to any registry. The
four registry secrets (`NPM_TOKEN`, `PYPI_TOKEN`, `CARGO_REGISTRY_TOKEN`,
`HOMEBREW_TAP_TOKEN`) do not exist yet, which is why the publish stage has only
been exercised as a dry run.

## PR 25: docs: record the history rewrite in the public-launch plan

State: open, merged: no, branch: docs/history-rewrite-record into main

Records the 2026-09-13 history rewrite (git filter-repo, force push, branch cleanup) and the clean re-scan result in the public-launch plan, per spec section 3.1. Docs only.
