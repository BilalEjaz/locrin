# Part A measurement: init, the Claude Code hooks, and the pre-commit hook

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

## Benchmarks

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

Run 1 is discarded for the reason the Task 7c report gave and not because of
anything it measured. The cold benchmark reads all 1846 files of the checkout,
so the first run after a gap pays for the operating system's page cache rather
than for the engine: 4473 ms against 3866, 3888 and 3871 ms, a spread of 22 ms
across the three counted runs and 602 ms between the first and the fastest of
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
measurements: under 5 ms on every run, and negative on two of the four. That is
the useful reading. The hook adds a stdin read and a thread spawn to work that
already takes 180 ms, and neither is measurable.

It also settles the concern Task 2 deferred, that the two-second watchdog might
be tight for a whole-tree Stop check. The dogfood section below has that check
at 202 to 244 ms on a five-file repository, and the 30-file benchmark, which is
a larger scope than most working trees, at 267 to 288 ms. The budget is roughly
seven times the largest scope measured here. A repository big enough to spend it
is not one this measurement has, so the budget stays where spec 9 put it.

## The dogfood run

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

Five lines for five files, which is spec 5.1's "prints every file it touched".
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
