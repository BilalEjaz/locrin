# Plan 4 measurement: init, hooks, and the MCP server

## Part A measurement: init, the Claude Code hooks, and the pre-commit hook

Branch `engine/agent`, HEAD `92b2886` ("engine: the PostToolUse hook joins the
benchmark set"), 10 September 2026. Plan 4 of 4,
`docs/superpowers/plans/2026-09-10-init-hooks-and-mcp.md`, Task 5.

Part A added four commands over Tasks 1 to 4: `locrin hook post-edit` (the
Claude Code PostToolUse hook, one file, two-second watchdog), `locrin hook stop`
(the Stop hook, the working tree against HEAD, three rounds per session),
`locrin hook pre-commit` (the git hook, the staged files, the verdict's exit
code) and `locrin init` (config, the two JSON merges, the git hook, the first
scan, the baseline). This document is the evidence that they hit their targets
and that `init` works on a repository nobody wrote it against.

### Benchmarks

`cargo test --release -p locrin-cli -- --ignored --nocapture`, on `92b2886`.
Bench repository `<home>/fasting-app` (1846 files), a fresh temporary
cache per benchmark, every run `--offline`, on an idle machine: no cargo, rustc
or locrin process running before the first run (`tasklist` empty on all three
names), and six CPU samples over eighteen seconds reading 1, 1, 4, 1, 0, 1
percent. Four sequential runs of the whole ignored set, the first discarded, as
in the two precision reports.

| Benchmark | Target | Run 1 (discarded) | Run 2 | Run 3 | Run 4 | Verdict |
| --- | --- | --- | --- | --- | --- | --- |
| cold index (`scan`, empty cache) | under 5000 ms | 4473 ms | 3866 ms | 3888 ms | 3871 ms | PASS |
| warm single-file check | under 300 ms | 183 ms | 182 ms | 184 ms | 186 ms | PASS |
| post-edit hook (`hook post-edit`) | under 300 ms | 188 ms | 183 ms | 185 ms | 182 ms | PASS |
| warm 30-file check | under 1000 ms | 274 ms | 288 ms | 267 ms | 286 ms | PASS |
| startup (`--help`) | under 50 ms | 20 ms | 18 ms | 19 ms | 19 ms | PASS |

All five gates are green, and green on the discarded run as well: every number
in the table is inside its target, so the discard changes no verdict.

Run 1 is discarded for the reason
`docs/superpowers/plans/2026-09-09-error-test-and-security-precision.md` gave and
not because of anything it measured. The cold benchmark reads all 1846 files of the checkout,
so the first run after a gap pays for the operating system's page cache rather
than for the engine: 4473 ms against 3866, 3888 and 3871 ms, a spread of 22 ms
across the three counted runs and 607 ms between the first and the fastest of
them. The rest of run 1 confirms that reading. Its warm numbers (183 / 188 / 274
/ 20 ms) sit inside the same few milliseconds as the counted runs, so the
machine was not loaded; only the benchmark that touches every file on disk moved.

### The new gate: the PostToolUse hook

Spec 5.2's number is the hook end to end, which is what an agent waits for:
process start, the payload read off stdin, the watchdog thread, the check, and
the JSON printed on stdout. `post_edit_hook_under_300ms` measures exactly that,
from spawning `locrin hook post-edit` to the process exiting, with the payload
being `tests/fixtures/hooks/post_edit_write.json` rewritten (through
`serde_json`, so a Windows path keeps its backslashes escaped) to name
`app/_layout.tsx` in the bench checkout.

Counted runs: 183, 185, 182 ms against a 300 ms target, a spread of 3 ms. The
warm single-file check on the same file and the same warm cache is 182, 184, 186
ms, so the hook's own overhead over a plain `check` is inside the noise of both
measurements: at most 5 ms on every run, and negative on one of the four. That is
the useful reading. The hook adds a stdin read and a thread spawn to work that
already takes 180 ms, and neither is measurable.

It also settles the concern Task 2 deferred, that the two-second watchdog might
be tight for a whole-tree Stop check. The dogfood section below has that check
at 202 to 244 ms on a five-file repository, and the 30-file benchmark, which is
a larger scope than most working trees, at 267 to 288 ms. The budget is roughly
seven times the largest scope measured here. A repository big enough to spend it
is not one this measurement has, so the budget stays where spec 9 put it.

### The dogfood run

`init` was run on a copy of `<home>/fastlift-admin` (a Cloudflare Worker,
five source files, a real git repository with commits). The copy is a temporary
directory, `LOCRIN_CACHE_DIR` points at a second temporary directory, and both
were deleted afterwards. The original checkout was never written to: `git status`
in it is empty and it holds no `locrin.toml`, `.mcp.json` or `.claude/`.

### First `init`

Command: `locrin init` (no `--offline`, so the first scan filled the advisory
snapshot). Exit code 0.

stdout, verbatim:

```
wrote locrin.toml
wrote .claude/settings.json
wrote .mcp.json
wrote .git/hooks/pre-commit
wrote locrin-baseline.json
baseline written with 33 finding(s)
```

stderr, verbatim (progress, which names no file the command touched):

```
indexing 5 source file(s) under <home>\<local-appdata>\Temp\claude\C--Users-chars\a63b6673-05bd-485b-a2b0-126e2b7498aa\scratchpad\dogfood\fastlift-admin
indexed 5 file(s) in 5.0 s
```

The 5.0 s is almost all osv.dev: this is the one scan `init` runs online, and the
same repository re-indexes in 0.0 s on the second run below.

One line per touched file, which is spec 5.1's "prints every file it touched".
Nothing was skipped: the repository has no `locrin.toml`, no baseline, no
`.husky/`, and a `.git/hooks` holding only git's own `.sample` files, so every
step had a free hand.

### `.claude/settings.json`, as written

```json
{
  "hooks": {
    "PostToolUse": [
      {
        "hooks": [
          {
            "command": "locrin hook post-edit",
            "timeout": 5,
            "type": "command"
          }
        ],
        "matcher": "Edit|Write|MultiEdit"
      }
    ],
    "Stop": [
      {
        "hooks": [
          {
            "command": "locrin hook stop",
            "timeout": 10,
            "type": "command"
          }
        ]
      }
    ]
  }
}
```

The keys inside each entry are alphabetical because `serde_json`'s map is, which
Task 4 recorded as lossless and left alone. The two timeouts are the plan's, set
explicitly because Claude Code's default is ten minutes.

### `.mcp.json`, as written

```json
{
  "mcpServers": {
    "locrin": {
      "args": [
        "mcp"
      ],
      "command": "locrin"
    }
  }
}
```

`locrin mcp` is Part B's command and does not exist yet, which the plan says
outright: the entry is written now so that the repository is configured once and
the server starts working the day Part B lands.

### `.git/hooks/pre-commit`, as written

```sh
#!/bin/sh
# Installed by locrin init. Remove this file to uninstall.
exec locrin hook pre-commit
```

### The PostToolUse hook, replayed

`console.log("dogfood");` was appended to `src/moderation.ts` in the copy and a
PostToolUse payload built for it from `tests/fixtures/hooks/post_edit_write.json`
with `cwd` and `tool_input.file_path` rewritten to the copy. `locrin hook
post-edit` with that on stdin, exit 0, nothing on stderr, stdout verbatim:

```json
{"decision":"block","reason":"BLOCK  1 blocking, 0 advisory, 1 finding(s) in 64 ms\nleftover-debug  src/moderation.ts:359  console.log(\"dogfood\");  ->  Remove the debug statement or route it through the project logger"}
```

One object, one line, the finding named with its file, line, evidence and fix,
and no file contents. The 64 ms inside the reason is the check; the hook process
around it is the 182 to 188 ms of the benchmark. The other 33 findings in the
repository are in the baseline written a minute earlier and are silent, which is
the point of writing a baseline during `init`: a repository adopting the engine
is gated on what it does next, not on its history.

### The Stop hook, four rounds

Four `locrin hook stop` invocations with `session_id: "dogfood"`, the first with
`stop_hook_active: false` and the three after it true, against the same working
tree with the debug line still in it. Every one exited 0 with nothing on stderr.
stdout, verbatim:

Round 1:

```json
{"decision":"block","reason":"BLOCK  1 blocking, 0 advisory, 1 finding(s) in 244 ms\nleftover-debug  src/moderation.ts:359  console.log(\"dogfood\");  ->  Remove the debug statement or route it through the project logger\nRound 1 of 3: fix the blocking findings above, then stop again."}
```

Round 2:

```json
{"decision":"block","reason":"BLOCK  1 blocking, 0 advisory, 1 finding(s) in 202 ms\nleftover-debug  src/moderation.ts:359  console.log(\"dogfood\");  ->  Remove the debug statement or route it through the project logger\nRound 2 of 3: fix the blocking findings above, then stop again."}
```

Round 3:

```json
{"decision":"block","reason":"BLOCK  1 blocking, 0 advisory, 1 finding(s) in 202 ms\nleftover-debug  src/moderation.ts:359  console.log(\"dogfood\");  ->  Remove the debug statement or route it through the project logger\nRound 3 of 3: fix the blocking findings above, then stop again."}
```

Round 4:

```json
{"systemMessage":"locrin: 1 blocking finding(s) remain after 3 rounds; the agent was not sent back again. Run `locrin check --base HEAD` to see them."}
```

Spec 5.2's cap behaves as designed on a repository it was not tested against:
three blocks, then the hook stands down and hands the problem to the person, with
the command that shows them the same view it had. `stop_hook_active` changed
nothing, which is deliberate and documented on the field: the hook counts its own
rounds, and that counter is the same guard whether a stop is the agent's own or a
continuation of one. The check itself is 202 to 244 ms over the whole working
tree, an eighth of the two-second budget.

### Second `init`

Run immediately after, in the same copy, with the debug line still present. Exit
code 0, stdout verbatim:

```
unchanged locrin.toml
unchanged .claude/settings.json
unchanged .mcp.json
unchanged .git/hooks/pre-commit
unchanged locrin-baseline.json
```

stderr:

```
indexing 5 source file(s) under <home>\<local-appdata>\Temp\claude\C--Users-chars\a63b6673-05bd-485b-a2b0-126e2b7498aa\scratchpad\dogfood\fastlift-admin
indexed 5 file(s) in 0.0 s
```

Every line reads `unchanged` and there is no baseline line, because no baseline
was written. Idempotence holds on a real repository, including for the git hook,
which is the one file whose ownership `init` has to decide by reading its text.

## Part B benchmarks

Branch `engine/mcp`, HEAD `ff0daf9` ("engine: the scripted agent session and the
stale-index rebuild, as tests"), 10 September 2026. Part B added `crates/mcp`,
the symbol search, and `locrin mcp` with its five tools over Tasks 6 to 8. One
thing Part B added is on the path these benchmarks measure: Task 7's
`last_verdict`, which every `check` writes and therefore every PostToolUse hook
pays for. As it stood when this table was taken it was a second `Index::open`
plus a `meta_set` after the run had already closed its own; the final-review fix
below makes it one `meta_set` on the connection the run recorded through, and
nothing else in Part B is on the path at all. So this is a regression check
rather than a new gate: the same five targets, re-run on the branch head, beside
Part A's numbers.

Method as in Part A: `cargo test --release -p locrin-cli -- --ignored
--nocapture`, bench repository `<home>/fasting-app` (1846 files, the
same count Part A measured), a fresh temporary cache per benchmark, every run
`--offline`, four sequential runs of the whole ignored set with the first
discarded as the page-cache run. `tasklist` was empty on `cargo`, `rustc` and
`locrin` before the first run, and the release test binaries were built before
that check so no compile overlapped a measurement.

| Benchmark | Target | Run 1 (discarded) | Run 2 | Run 3 | Run 4 | Part A (counted) | Verdict |
| --- | --- | --- | --- | --- | --- | --- | --- |
| cold index (`scan`, empty cache) | under 5000 ms | 6054 ms | 4403 ms | 4716 ms | 4673 ms | 3866 to 3888 ms | PASS on the counted runs; the discarded run missed |
| warm single-file check | under 300 ms | 238 ms | 270 ms | 292 ms | 293 ms | 182 to 186 ms | PASS, 7 ms of margin at worst |
| post-edit hook (`hook post-edit`) | under 300 ms | 282 ms | 279 ms | 274 ms | 298 ms | 182 to 185 ms | PASS, 2 ms of margin at worst |
| warm 30-file check | under 1000 ms | 363 ms | 391 ms | 381 ms | 398 ms | 267 to 288 ms | PASS |
| startup (`--help`) | under 50 ms | 23 ms | 24 ms | 21 ms | 28 ms | 18 to 19 ms | PASS |

All five gates are green on the three counted runs. Two things in that table are
worth stating plainly rather than leaving to the reader.

The first is that the discard changed a verdict this time. Part A's run 1 was
green on every gate, so discarding it was bookkeeping. Here run 1's cold index
is 6054 ms against a 5000 ms target and the benchmark failed the assertion; runs
2, 3 and 4 are 4403, 4716 and 4673 ms. The reading is the same one Part A gave
(the cold benchmark reads all 1846 files, so the first run after a gap pays for
the operating system's page cache, and the gap of 1651 ms between run 1 and the
fastest counted run is the same order as Part A's 607 ms), but the honest
statement is that on this machine, in this state, a genuinely cold first scan of
a 1846-file checkout does not meet spec 3.4's five seconds. Nothing was tuned
and the target was not moved.

The second is that every number in this table is materially slower than Part A's,
including `--help`, which starts a process and prints text. A uniform slowdown
across a benchmark that touches 1846 files and one that touches none is not the
engine: it is the machine. The measurement was taken with the laptop on battery
under the Balanced power scheme, and six CPU samples over eighteen seconds read
13, 11, 4, 11, 12 and 6 percent against Part A's 1, 1, 4, 1, 0 and 1 percent.
Two gates are consequently thin: the post-edit hook came in at 298 ms against
300 ms on run 4, and the warm single-file check at 293 ms against 300 ms. They
pass as measured and they are recorded as measured. A re-measurement on mains
power would very likely restore Part A's margins, and re-running until the
numbers improve is not what this document is for.

The post-edit hook's own overhead, which is the reading spec 5.2's budget is
really about, is unchanged: the hook is 274 to 298 ms and the warm single-file
check on the same file and the same warm cache is 270 to 293 ms, so the stdin
read and the watchdog thread are still inside the noise of both measurements.
Hook minus warm check, run by run, is 9, -18 and 5 ms on the counted runs, which
is to say at most 9 ms and negative on one of the three. Run 1's 44 ms is the
larger figure, and it belongs to the discarded run. That is the same result
Part A reported on a faster machine, which is the useful part: the overhead
tracks the check rather than sitting on top of it.

## Part B re-measurement after the final-review fixes

Branch `engine/mcp`, HEAD `3f07c1b` ("engine: accept_finding reuses the recorded
index, the verdict is recorded on the run's own connection, and a tool panic is
logged"), 10 September 2026.

Only the two gates the fix can move are re-run: the warm single-file check and
the post-edit hook. The fix is on the `check` path, which is what both measure,
and it removes a second `Index::open` and a second commit per check. The cold
scan, the 30-file check and `--help` are untouched by it and their numbers above
stand.

Method as above and as in Part A, narrowed to one benchmark per invocation:
`cargo test --release -p locrin-cli --test bench <name> -- --ignored --nocapture`,
bench repository `<home>/fasting-app`, a fresh temporary cache per
benchmark, `--offline`, four sequential runs with the first discarded. The
release test binary was built before the first run, and `tasklist` was empty on
`cargo`, `rustc` and `locrin` at that point.

Power state, read before the runs and again after them with
`(Get-CimInstance Win32_Battery).BatteryStatus`: `1` both times, which is
discharging. **The laptop was on battery for this re-measurement, as it was for
the table above, so these margins are not settled.** They are better than the
battery numbers above rather than worse, which is the direction the fix predicts,
but a battery measurement cannot separate the fix from the machine's state and
this document does not claim it does. A mains re-measurement is what would settle
them.

| Benchmark | Target | Run 1 (discarded) | Run 2 | Run 3 | Run 4 | Before the fix (counted) | Verdict |
| --- | --- | --- | --- | --- | --- | --- | --- |
| warm single-file check | under 300 ms | 218 ms | 194 ms | 200 ms | 210 ms | 270 to 293 ms | PASS, 90 ms of margin at worst |
| post-edit hook (`hook post-edit`) | under 300 ms | 204 ms | 205 ms | 207 ms | 205 ms | 274 to 298 ms | PASS, 93 ms of margin at worst |

Both gates are green on all four runs, discarded one included. The two thin
margins the table above reported as a concern (298 ms and 293 ms against 300 ms)
are gone: the worst counted number here is 210 ms.

The hook's own overhead over a plain `check` is unchanged and still inside the
noise of both measurements. Hook minus warm check, run by run on the counted
runs, is 11, 7 and -5 ms: at most 11 ms, and negative on one of the three, which
is the same reading Part A and the table above both gave.

## Deviations recorded during execution

The rulings the controller made over Tasks 1 to 4, as they stand in the SDD
ledger, restated as what the code does and why. The same bullets are appended to
the plan.

- **Task 2, the Stop hook's session id is percent-encoded and capped before it
  names a file.** The session id arrives in the payload, and payload data must
  never become a path on its own terms. It is reduced to `[A-Za-z0-9_-]` and cut
  at 100 characters before the round counter's file is named. The cost if the
  ruling is wrong is nothing: a UUID, which is what Claude Code sends, passes
  through untouched.

- **Task 3, a staged file the engine does not parse still reaches the raw scope.**
  The brief's test `pre_commit_ignores_a_staged_file_that_is_not_source` asserted
  the opposite, and it was wrong twice over: the fixture's non-source file is
  `package.json` rather than `locrin.toml`, and a staged `package.json` is
  exactly what `vulnerable-dependency` and the Supabase RLS rule gate a commit
  on. The implementation was kept as written and the stderr assertion moved to
  `pre_commit_says_when_nothing_is_staged`, so what changed was the plan text.

- **Task 3, the `staged_files` unit tests live in `crates/cli/src/git.rs`.** The
  CLI has no library target, so `tests/cli.rs` cannot reach the function to test
  it from outside.

- **Task 4, the file count for `init`'s progress line is walked in `init.rs`
  rather than reported by `run::scan`.** The file structure said `run.rs` would
  grow a progress callback. `run::scan` can only report its count once it is
  over, and the first scan on a cold tree is long enough that a person watching a
  blank line assumes a hang, so `init` walks with `source_files` first and then
  calls `run::scan` unchanged. The second walk costs milliseconds and leaves the
  scan pipeline untouched; the cost if the ruling is wrong is one redundant walk
  per `init`.

- **Task 4, a `.git/hooks/pre-commit` whose text is exactly `PRE_COMMIT_SCRIPT`
  is locrin's own.** It reports `unchanged`; any other content reports `skipped`
  with the line telling the operator what to add by hand. The brief's "always
  skipped" contradicted its own idempotence test, and it would have made a second
  `init` disown the file the first one wrote a moment earlier. Nothing is
  overwritten either way, so the ruling costs nothing if it is wrong.

- **Task 5, the hook benchmark passes no `--root`.** The brief asked for one, and
  the bench's `locrin` helper already sets the process working directory to the
  checkout, which is the root the hook reads and the place Claude Code runs a
  hook from.

- **Task 5, the first run of the four-run benchmark set is discarded and
  reported.** The cold benchmark reads every file of the 1846-file bench
  checkout, so the first run after a gap measures the operating system's page
  cache: 4473 ms against 3866, 3888 and 3871 ms, with warm numbers on the same
  invocation inside a few milliseconds of the counted runs. All four numbers are
  in the table and every one of them is green, so the discard changes no verdict.
  This is the method the two precision reports used.

- **Task 5, the pull request is opened by the controller after the whole-branch
  review, not by this task.** The brief's step 4 was held back deliberately; the
  measurement and the dogfood are what Task 5 delivers.

- **Final review, the two agent hook arms catch their own panics.** The watchdog
  covers a panic on the worker thread; a panic anywhere else in `post_edit` or
  `stop` is on the process's own thread and unwinds into `main`'s
  `catch_unwind`, which exits 2. Claude Code reads a Stop hook's exit 2 as
  "block, and show stderr to the agent", the one failure spec 9 exists to
  prevent, and it would repeat on every stop. `hook::guarded` wraps both arms,
  answers a panic with the same `systemMessage` shape every other failure uses,
  and exits 0. `pre_commit` is deliberately not wrapped: its exit code is the
  verdict's, and a broken engine there must stop the commit.

- **Final review, the pre-commit hook's directory comes from
  `git rev-parse --git-path hooks`, not from `.git/hooks`.** The hard-coded path
  was wrong in two ordinary situations: in a linked worktree `.git` is a file, so
  init reported "not a git repository" to somebody standing in one, and under
  `core.hooksPath` init wrote a file git never runs and reported `wrote` for it.
  `git::hooks_dir` asks git, creates the directory when it is not there yet
  (`core.hooksPath` may name one nobody has made), and returns None for every way
  of failing, which keeps the existing "not a git repository" skip. Husky is
  still checked first. The path reported on stdout is relative to the root where
  the directory is under it and absolute where it is not, which is the ordinary
  case in a worktree.

- **Task 6, a JSON-RPC message is dispatched on its method before its id.** A message carrying neither is answered `-32600` with a null id rather than being read as a notification and dropped, and an explicit `id: null` is treated as a notification, which is what the JSON-RPC 2.0 text says. Only a malformed client can reach either arm, so the cost if the ruling is wrong is one error shape nobody well-behaved ever sees.

- **Task 6 review, `serve` answers `-32700` on a line that is not UTF-8 and skips a blank one, instead of returning `Err`.** The function's own docstring already promised a parse error and a live session, but the code ended the loop, so one stray byte from a client took the server down mid-conversation. It now reads lines as bytes, replies `-32700` with a null id to a line it cannot decode, ignores a line that is only whitespace, and carries on. Only a malformed client provokes either arm, so the cost if the ruling is wrong is one error frame nobody well-behaved ever sees.

- **Task 8, three tool-argument resolutions.** `find_existing` adds its "return types are not indexed in version one" note only when `returns` was actually supplied, because a note on a call that never mentioned it reads as a search that considered something it did not. Every argument is validated before the index is looked at, so a caller with a bad argument is told about the argument rather than about a missing index. "At least one of `intent` or `name`" is expressed as prose in the schema rather than as an `anyOf` of two `required` lists, which every client renders badly, and the tool repeats it in words when a call arrives empty.

- **Task 8, `find_existing` refuses an index that `Index::open` has just rebuilt empty.** The path check alone was not enough: `open` rebuilds an index whose schema this build does not know, or one that is corrupt, into an empty database, and an upgraded binary meets exactly that on its first run. The tool would then answer "nothing exists" to the one question an agent asks before writing a duplicate. It now checks again once the database is open and says `index is empty: run locrin scan first`. Task 9's `a_stale_index_rebuilds_silently_for_the_server` covers the same rebuild from the server's side, where the rebuild must also not put a line on stdout.

- **Task 8 review, `status` no longer claims it never builds the index.** Its description read "it never builds the index", which `Index::open` makes untrue: opening an index whose schema this build does not know rebuilds it empty. It now reads "it never creates an index that does not exist", which is the promise that actually holds. This is the half of the empty-index fix above that is about what the tool tells its caller rather than what it does.

- **Task 9, the cold-index benchmark missed its target on the discarded first run and is recorded rather than tuned away.** Run 1 measured 6054 ms against spec 3.4's 5000 ms; the three counted runs are 4403, 4716 and 4673 ms. Part A's discard changed no verdict because its run 1 was green, and this one does, so it is stated in the measurement rather than left to the table. Nothing was tuned and no target was moved.

- **Task 9, the whole Part B benchmark set is slower than Part A's because the machine was on battery, and the numbers stand as measured.** Every gate moved, `--help` included, which is a process start and a print: a uniform slowdown across a benchmark that reads 1846 files and one that reads none is the machine, not the engine. The laptop was on battery under the Balanced power scheme, with CPU samples at 4 to 13 percent against Part A's 0 to 4. Two gates are consequently thin (the post-edit hook at 298 ms and the warm single-file check at 293 ms, both against 300 ms). They pass as measured and are reported as a concern; re-running until the numbers improve is not a measurement.

- **Task 9, the pull request is opened by the controller after the whole-branch review, not by this task.** As in Task 5, the brief's `gh pr create` step was held back deliberately. What Task 9 delivers is the scripted session and stale-index tests, the README, and the Part B measurement.

- **Final review, `accept_finding` runs its pass with `record` on and `locrin baseline accept` runs its pass with `record` off.** `baseline_accept_as` grew a `record` parameter because its two callers owe opposite things. The MCP tool is called straight after a `check_changes` that recorded, so the index is already current, and a non-recording pass there opens a throwaway in-memory index and parses the whole repository from cold, once per acceptance and up to ten times in one agent round. The command line's `baseline accept` is not a check, and the index is where `--changed` keeps its watermark: a recording pass there would answer for every pending edit and leave the next `locrin check --changed` nothing to report. `run.rs`'s `an_agent_accept_records_the_index_and_a_command_line_accept_leaves_it_alone` pins the split by the changed count of the pass that follows each accept.

- **Final review, `check` writes `last_verdict` through the connection its own pass recorded on.** `record_verdict` called `Index::open` a second time, paying for `init` and a second commit on the path a PostToolUse hook waits on, which is the path Part B's benchmarks measure. `pass` now hands its `Index` back, `Some` exactly when it recorded, and `check` calls `meta_set` on that. A failure to record is still a warning on stderr and never an error: the verdict is the answer and the caller already has it. `check_records_the_last_verdict` covers it unchanged, and the two gates the change can move were re-measured.

## Dogfood cleanup

Branch `engine/cleanup`, HEAD `d7f423e` ("engine: version 0.2.0, finding ids
changed"), 10 September 2026. Plan
`docs/superpowers/plans/2026-09-10-dogfood-cleanup.md`, Task 5. The first real
`locrin init` on a repository nobody wrote the engine against, the FastLift run
that reported 778 findings and wrote 756 baseline entries, is recorded in
`docs/superpowers/plans/2026-09-10-dogfood-cleanup.md` rather than here; Part A's
own dogfood run above is a different, five-file one on `fastlift-admin`. This
section is what the FastLift run turned up, what was done about it, and the
evidence that the fixes hold on the same repository.

### What the dogfood showed, and the fix

- **Ids collided.** `init` reported 778 findings and wrote 756 baseline entries.
  Thirteen ids were shared by 33 `vulnerable-dependency` findings, because three
  installed versions of `@xmldom/xmldom` match one advisory and the anchor was
  the package name and the advisory id with no version; one id was shared by two
  `leftover-commented-code` findings, two identical commented lines inside one
  function, because a line rule anchored on the enclosing symbol's name alone.
  `Baseline::accept` dedupes by id, so accepting one instance silently accepted
  the others. Fixed in `c32f4ae` and `527bff9`: a line rule's anchor carries the
  line's trimmed text and its ordinal among the identical lines in that symbol,
  an advisory carries the installed version, and the ordinal pass extracts the
  symbol table once rather than once per earlier line.
- **A NUL byte ended the parse.** `src/domain/food/collapseFoodDuplicates.ts` and
  `src/domain/sync/quarantineNotice.ts` hold a literal NUL inside a template
  literal as a composite-key separator, and tree-sitter's lexer reserves byte 0
  as end of input, so both files were excluded from every rule. Fixed in
  `8d48c5f` and `ca905e8`: the parser is handed a copy with each NUL replaced by
  0x01, one byte for one byte so every span still indexes the original, and the
  substitute is proven on both grammars.
- **A bare ampersand in JSX text excluded the file.** `app/settings.tsx` and
  `src/features/today/YourNumbersPanel.tsx` write `BODY & NUTRITION` as JSX
  text, and the external scanner's `html_character_reference` stops at any `&`
  that does not open a valid entity (tree-sitter-javascript issue 366, open),
  which exempted a 2000-line screen from every rule. Fixed in `90393e4`: that one
  error shape is tolerated and the file counts as parsed.
- **`init` parsed the repository twice.** It printed every parse warning twice
  and took 13.9 s, because the scan parsed 1846 files and then `baseline_create`
  ran a second, non-recording pass over an in-memory index with a cold findings
  cache. Fixed in `69b8768`: `baseline_create` takes a `record` flag, `init`
  passes true and the command line's `baseline create` keeps false, so a
  baseline command still never moves the `--changed` watermark.
- **The fifth file is not fixable here.**
  `src/theme/historyPort.guard.test.tsx:393` writes
  `as import('../domain/signals/types').SleepStageSegment[]`, and the grammar has
  no rule for an import type with an array suffix (tree-sitter-typescript issue
  322, open). It stays excluded with one warning, and the README now says so and
  names the two ways to write the type that parse.

Ids changed for every rule, which invalidates every baseline. `0.1.0` became
`0.2.0` in `d7f423e` for that reason and not only as an announcement: the
findings cache key mixes in `CARGO_PKG_VERSION`, so without the bump a warm row
written by the old binary would be served with the old id. With it, the first run
after an upgrade rebuilds the cache and pays one cold pass.

### Benchmarks

Method as in Part A and Part B: `cargo test --release -p locrin-cli -- --ignored
--nocapture`, bench repository `<home>/fasting-app` (1846 files, the same
count both earlier parts measured and the count this branch's scan reports), a
fresh temporary cache per benchmark, every run `--offline`, four sequential runs
of the whole ignored set with the first discarded as the page-cache run. The
release binary and the release test binaries were built before the measurement,
and `tasklist` was empty on `cargo`, `rustc` and `locrin` after that build and
before run 1, so no compile overlapped a run.

Power state, read with `(Get-CimInstance Win32_Battery).BatteryStatus` before
run 1 and again after run 4: `2` both times, which is mains. This is the mains
re-measurement Part B asked for.

| Benchmark | Target | Run 1 (discarded) | Run 2 | Run 3 | Run 4 | Verdict |
| --- | --- | --- | --- | --- | --- | --- |
| cold index (`scan`, empty cache) | under 5000 ms | 46561 ms | 10032 ms | 10033 ms | 9857 ms | FAIL on all four runs |
| warm single-file check | under 300 ms | 193 ms | 195 ms | 184 ms | 186 ms | PASS, 105 ms of margin at worst |
| post-edit hook (`hook post-edit`) | under 300 ms | 199 ms | 181 ms | 189 ms | 184 ms | PASS, 111 ms of margin at worst |
| warm 30-file check | under 1000 ms | 347 ms | 337 ms | 349 ms | 328 ms | PASS |
| startup (`--help`) | under 50 ms | 21 ms | 19 ms | 20 ms | 19 ms | PASS |

Four gates pass with more margin than Part B's measurements gave, including the
two the plan said to watch. Task 1 adds a line scan per finding and the
warm single-file check is the gate that pays for it: 184 to 195 ms against Part
B's post-fix 194 to 210 ms, so the scan does not show. The
post-edit hook is 181 to 189 ms and the hook's own overhead over the warm check
on the same file is -14, 5 and -2 ms on the counted runs, which is noise in both
directions, the same reading Part A and Part B gave.

The cold gate fails, and it failed on every run rather than only on the
discarded one. Nothing was tuned and the target was not moved. What follows is
what was measured afterwards to find out what the failure is, because a 10 s
number against Part A's 3866 ms is far too large to read as the two per-parse
costs this branch added.

The same benchmark, alone in its own process, on the same binary and the same
machine an hour later: `cargo test --release -p locrin-cli --test bench
cold_index -- --ignored --nocapture` twice, 5587 ms and 5072 ms. A fifth run of
the whole ignored set at that point read 5189 ms cold, with 216 ms warm, 213 ms
hook, 440 ms for the 30-file check and 21 ms startup. So the 10 s plateau was a
state the machine was in during the four mandated runs and not what the code
costs, and the 46561 ms of run 1 is the same state at its worst.

The attribution is the useful part. Four release binaries were built from four
commits into one target directory and each ran `locrin scan --offline` against
the FastLift checkout with a fresh cache, twice each, interleaved, on an idle
machine:

| Binary | Commit | Cold scan, pass 1 | Cold scan, pass 2 |
| --- | --- | --- | --- |
| main, before this branch | `7a6f878` | 5524 ms | 6275 ms |
| end of Task 1 (ids) | `527bff9` | 5456 ms | 5420 ms |
| end of Task 3 (NUL, ampersand) | `90393e4` | 5699 ms | 5676 ms |
| branch head | `d7f423e` | 5592 ms | 5383 ms |

The branch head and the commit it branched from are the same speed inside the
spread of repeated runs of either one, and the two commits between them are too.
So this branch did not make the cold scan slower: the byte scan Task 2 adds per
parse, the error walk Task 3 adds per parse and the symbol extract Task 1 adds
per finding are all inside the noise of a 5 s measurement over 1846 files.

What the table above does say, and what stands as the concern, is that a cold
scan of this checkout on this machine is now around 5.4 to 6.3 s against spec
3.4's 5000 ms, on mains, for `main` as much as for the branch. Part A measured
3866 to 3888 ms for the same benchmark on 10 September and Part B measured 4403
to 4716 ms on battery the same day. The gate is missed by every binary tested and
by roughly the same amount, so it is either the machine or something that landed
before this branch, and finding out which is not this task's to do. It is
recorded here and reported to the controller.

### The read-only FastLift check

`<home>/fasting-app`, nothing written into it. `locrin-baseline.json` was
moved to a scratch directory before the run and moved back after, so every
finding the repository has is reported rather than filtered, and
`git status --short` was taken before and after and both listings were saved:
`diff` of the two saved files prints nothing, so they are identical byte for
byte, including the untracked `locrin-baseline.json` and `locrin.toml`
that the dogfood `init` left there. The baseline came back at the same 177789
bytes it went out as. `locrin.toml` in that checkout is the `init` template with
every setting still commented out, so this is the engine on its defaults.

The run: `LOCRIN_CACHE_DIR=<fresh temp dir> locrin check --sarif --offline`, the
release binary of `d7f423e`.

- 766 findings, 766 distinct `partialFingerprints["locrin/id"]` values, no id
  shared by two findings and therefore no rule to list as still colliding. By
  rule: 597 `dead-export`, 99 `leftover-commented-code`, 59 `unused-import`, 6
  `leftover-debug`, 4 `leftover-agent-marker`, 1 `test-newly-skipped`.
- stderr, in full, is two lines: `warning: parse errors in
  src/theme/historyPort.guard.test.tsx; excluded from rules` and `warning: no
  cached advisory snapshot; vulnerable-dependency skipped`. Four of the five
  files the dogfood excluded are gone from it, the fifth is still there, and the
  warning appears once rather than twice.
- The four fixed files are checked rather than exempt: `app/settings.tsx` 3
  findings, `src/domain/food/collapseFoodDuplicates.ts` 1,
  `src/domain/sync/quarantineNotice.ts` 1, and
  `src/features/today/YourNumbersPanel.tsx` 0, which is a file that now parses
  and has nothing to report. `src/theme/historyPort.guard.test.tsx` has 0
  because it is still excluded, which is the documented limit.

A fresh cache offline means no advisory snapshot, so that run skips
`vulnerable-dependency`, which is the rule Task 1's other half was for. One
further run, fresh cache and online, was made for that half alone: 820 findings,
820 distinct ids, no collisions, 54 of them `vulnerable-dependency`. The dogfood
run this section opens with reported 778 findings and could only write 756
baseline entries; the same repository now yields an id per finding, with the
three installed versions of one package separated, which is what Task 1 was for.

### Deviations recorded during execution

The rulings the controller made over Tasks 1 to 5, as they stand in the SDD
ledger. The same bullets are appended to the plan.

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
  review, not by this task.** As in Part A's Task 5 and Part B's Task 9, the
  brief's `gh pr create` step was held back deliberately. What Task 5 delivers is
  the version bump, the documentation, the benchmarks and the read-only FastLift
  evidence.
- **Task 5, the cold benchmark is reported as failed and attributed rather than
  re-run until it passes.** The four mandated runs read 46561, 10032, 10033 and
  9857 ms against a 5000 ms target. Instead of tuning or moving the target, three
  earlier commits were built and measured beside the branch head, which put every
  binary including `main` at 5.4 to 6.3 s and the four mandated runs' plateau
  down to the machine's state at the time. The gate is missed either way and it
  is missed by `main` too, so it is recorded as a concern and handed to the
  controller.

- **Task 5, the FastLift tracker entry D9 line was written by the controller, not
  by this task.** The plan's Global Constraints ask for a line in
  `<home>/fasting-app/.planning/ROADMAP-SMART-2026-08-25.md`; the
  controller's read-only amendment forbade this task writing anything into that
  checkout beyond moving the baseline out and back, so the controller wrote the
  tracker line itself. The constraint is met, by the controller's hand rather
  than this task's.
