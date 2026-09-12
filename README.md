# Locrin

Locrin is a deterministic quality gate for code written by people and by
agents. It answers one question about a change, pass, advisory or block, the
same way every time, in under a second once the index is built, with no model
in the loop.

It reads TypeScript and JavaScript, and PHP and Python behind a one-line
opt-in, with tree-sitter, keeps a SQLite index of the repository outside the
working tree, and ships 21 rules whose precision is measured before they ship.
The same binary serves a person on the command line, a git pre-commit hook,
Claude Code's hooks, an MCP client and a GitHub Action.

## Install

    npm i -D locrin            # or: npx locrin --version
    pip install locrin
    brew install BilalEjaz/locrin/locrin
    cargo install locrin
    curl -fsSL https://raw.githubusercontent.com/BilalEjaz/locrin/main/install.sh | bash
    irm https://raw.githubusercontent.com/BilalEjaz/locrin/main/install.ps1 | iex

The installer scripts, the Homebrew formula and the GitHub Action download the
release assets and verify them against `SHA256SUMS`; npm and PyPI carry the same
binaries inside their packages; `cargo install` builds from source. Linux
x86_64, macOS (Intel and Apple silicon) and Windows x86_64 are prebuilt;
anything else builds from source with cargo.

## Thirty seconds

    locrin init          # writes locrin.toml, the baseline, and the hooks
    locrin check         # verdict on the repository; exit 1 blocks, 2 is an engine error

With Claude Code, `init` also wires the post-edit hook, so an agent hears about
a blocking finding before it moves on, and the stop hook, which sends the agent
back, up to three times, rather than letting the session end with a block
outstanding. Without an agent, the pre-commit hook and the GitHub Action give
the same verdict.

## Free and paid

Everything in this repository is free and MIT licensed: the engine, all 21
rules including the ten security rules, the hooks, the MCP server and the
Action. Nothing here needs an account or touches the network except the
dependency advisory lookup, which `--offline` turns off.

Paid, later and separate: a security pack with further framework-specific
checks and compliance reports, unlocked offline by licence key, and a hosted
layer for history across runs and people. Neither will take back anything that
shipped free.

## Benchmark

Precision and recall per rule, measured on a public corpus anyone can rerun:
https://github.com/BilalEjaz/locrin-benchmark

## Limits worth knowing first

Locrin does not type check. Import resolution is heuristic: it reads `tsconfig`
paths and package `exports`, and falls back to sensible guesses. That is enough
for the dead-code and boundary rules to be useful, and it is not a substitute
for `tsc`.

`already-exists`, the duplicate detector, is release two. What exists today is
`find_existing`, the MCP tool that searches the indexed symbols so an agent can
look before it writes. The rule that fails a build over a duplicate is not
shipped yet.

One construct the parser cannot read excludes its file. The grammar has no rule
for an import type carrying an array suffix, `x as import('./t').Seg[]`
(tree-sitter-typescript issue 322, open), and the parser recovers by inventing a
MISSING identifier, a token that is not in the file at all. A MISSING node is
never tolerated, because the engine cannot read a tree the parser made up, so
the file is excluded from every rule with one `warning: parse errors` line on
stderr. Write the type as `Array<import('./t').Seg>`, or import it by name and
use `Seg[]`, and the file parses and is checked like any other. Two
things that look like the same problem are not: a bare `&` in JSX text, as in
`BODY & NUTRITION`, is tolerated, because the scanner's refusal leaves the rest
of the tree intact and the only thing lost is the words after the `&` in that
one text run, which no rule reads today; and a NUL byte in source parses,
because the parser is handed a copy with each NUL replaced by a byte the lexer
does not reserve, one byte for one byte, while every rule reads the file as it
is written.

A stray `>` in JSX text, as in `5 > 3 wins`, is tolerated by that same rule: the
scanner's refusal parents to the element among its text children, and only a run
carrying `<`, `{` or `}` is markup the parser gave up on. What stays excluded is
an `&` whose text run carries on past a full stop or a comma, as in
`tea & toast. Lovely`, because the parser recovers by reading the remainder as a
member expression, which swallows the element and leaves the error at the top of
the file rather than inside it; the file is then excluded from every rule with
one `warning: parse errors in <file>; excluded from rules` line on stderr. The
same text ending at the full stop, `tea & toast.`, is tolerated like any other
ampersand.

## Quick start

```
locrin init
```

`init` wires the repository up in one pass and prints every file it touched. It
is safe to run again: nothing locrin did not write is ever overwritten, and a
second run reports every file unchanged.

What it writes:

- `locrin.toml`, only when the file is absent. Every setting in the template is
  commented out, so the file you find loads as the defaults and reads as the
  documentation of what you may turn on.
- `.claude/settings.json`, merged rather than replaced. It adds a `PostToolUse`
  entry matching `Edit|Write|MultiEdit` running `locrin hook post-edit` with a
  5 second timeout, and a `Stop` entry running `locrin hook stop` with a 10
  second timeout. Your other keys, your other hooks, and a locrin command you
  edited yourself are all left alone.
- `.mcp.json`, merged the same way, adding an `mcpServers.locrin` entry that
  runs `locrin mcp`. An entry that is already there is left exactly as it is.
- The git pre-commit hook, installed only when nothing else owns it. The
  directory comes from git itself (`git rev-parse --git-path hooks`), so a
  linked worktree and a `core.hooksPath` setting both work. If husky is in the
  repository, or a pre-commit hook already exists, init skips the file and
  prints the one line telling you what to add by hand.
- `locrin-baseline.json`, only when absent, after the first scan. Every finding
  the repository has today goes into it, so adopting locrin gates you on what
  you do next rather than on your history.

If a file cannot be written, init still prints the list of what it did write
before it reports the error. Nothing else in the repository names those files as
locrin's.

## The agent loop

Three commands run as hooks. All three are offline, always, because a hook runs
on every edit and must never wait on a network.

`locrin hook post-edit` is the Claude Code `PostToolUse` hook. It reads the tool
event as JSON on stdin, checks the single file that was written, and has a two
second watchdog. What the agent sees:

- Blocking findings: `{"decision": "block", "reason": ...}`, which makes Claude
  address the reason before it moves on.
- Advisory findings only: `{"additionalContext": ...}`, which hands the findings
  over without calling the edit an error.
- A timeout, an engine error, or a panic: `{"systemMessage": ...}` saying it
  passed without checking.

It always exits 0. A hook that fails must never stall the agent over its own
failure.

`locrin hook stop` is the `Stop` hook. Its scope is the working tree against
`HEAD`, or `--changed` when the repository has no HEAD yet, so it sees more than
the one file an edit touched. It sends the agent back at most three times per
session, with `Round n of 3` in the reason. On the fourth stop it stands down
and prints a `systemMessage` for the person, naming the command that shows the
same view it had. A clean verdict resets the counter; the cap, once hit, is
permanent for that session. The round counter lives beside the index, keyed by
session id, never in the repository.

Because the Stop scope is the working tree, uncommitted findings a human left
behind before the session started count against the agent's rounds. Commit or
accept them first if that is not what you want.

`locrin hook pre-commit` is the git hook. It checks the working-tree content of
the staged paths, prints the same verdict `locrin check` prints, and its exit
code is the verdict's. It is the one hook whose failure is not swallowed: a
broken engine stops the commit rather than waving it through as a pass. Use
`git commit --no-verify` when you mean to.

## The MCP server

```
locrin mcp
```

Newline-delimited JSON-RPC over stdio, which is what the `.mcp.json` entry
starts. Stdout carries protocol messages and nothing else; anything for a human
goes to stderr. The server runs offline unless you pass `--online`, so a tool
call never waits on the network.

Five tools:

- `check_changes` takes `paths` (an array, the whole repository when omitted) or
  `base` (a git ref, the pull-request view), never both. It returns the verdict:
  status, counts, and up to ten findings with their ids. No file contents.
- `find_existing` takes `intent`, `name`, `params` and `returns`, and needs at
  least one of `intent` or `name`. It returns up to ten indexed symbols, ranked
  on the parameter count first and shared name tokens second, each with its
  file, line, kind, name, parameter count, export flag and declaration line.
  `returns` is accepted but not yet indexed, and the result says so. Without an
  index, or on an index that has just been rebuilt empty, it refuses to answer
  rather than saying nothing exists.
- `explain_finding` takes an `id` from a verdict. It returns the rule and what
  the rule is for, the span, the evidence, the suggested fix, and whether the
  finding is already accepted in the baseline.
- `accept_finding` takes an `id` and a `reason`, and records the finding in the
  baseline as accepted debt, authored as `agent via mcp` so a reviewer can tell
  an agent's sign-off from a person's. An empty reason is refused.
- `status` returns what locrin knows about the repository: the index path, file
  count and schema, the config, the baseline, and the last verdict recorded. It
  reads only, and never creates an index that is not there.

Every tool answers with one compact JSON object. An argument it will not accept
comes back as a tool error naming the argument, and an engine failure comes back
the same way, so a bad call never takes the server down.

## Commands

`--root <dir>` is global and defaults to the current directory.

- `locrin check [paths...]` checks everything by default and prints a verdict.
  `--changed` limits it to files whose content changed since the last index,
  plus the graph findings their edges reach. `--base <ref>` checks what differs
  from the merge base with a ref plus untracked files, which is the pull-request
  view. `--since <ref>` checks the files changed by the commits in `ref..HEAD`,
  which is the deployment gate. `--json` prints the compact agent form, capped
  at ten findings. `--sarif` prints SARIF 2.1.0 with every finding, for code
  scanning uploads. `--markdown` prints the pull-request summary: a marker line,
  the verdict, and at most ten findings, with a `Details:` link when
  `LOCRIN_RUN_URL` is set. `--sarif-file <path>` writes the full SARIF to a file
  whatever stdout is showing, so one run can both comment and upload. `--offline`
  skips the network. The three output forms are exclusive. Paths and the diff
  scopes cannot be combined: each names its own set of files.
- `locrin scan` indexes the repository and warms the findings cache without
  printing a verdict, so the next check pays only for what changed. `--offline`
  skips the network.
- `locrin baseline create` snapshots every current finding into the baseline.
  `--offline` skips the network.
- `locrin baseline accept <id> --reason <text>` accepts one finding by id.
  `--offline` skips the network.
- `locrin hook post-edit`, `locrin hook stop`, `locrin hook pre-commit` are the
  three hooks above. They have no `--offline`: a hook runs on every edit, so it
  is always offline.
- `locrin init` wires the repository up. `--offline` skips the network on the
  first scan, which is the one scan `init` runs online.
- `locrin mcp` serves the five tools.

## GitHub Action

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
      - uses: BilalEjaz/locrin/action@v0.5.0
        with:
          version: v0.5.0
```

The action downloads the release binary for the runner, verifies it against the
release `SHA256SUMS`, and runs one `locrin check` over the pull-request view. It
posts the verdict as a comment, uploads the SARIF to code scanning, and fails
the job when the verdict is BLOCK.

The three permissions each pay for one thing: `contents: read` for the
checkout, `pull-requests: write` for the comment,
`security-events: write` for the SARIF upload. Drop the last two by setting
`comment: "false"` and `sarif: "false"`. `fetch-depth: 0` is what gives the run
a merge base to diff against: a shallow checkout has none, and the default
pull-request path then exits 2.

The default `${{ github.token }}` is enough for both the download and the
comment, and `uses: BilalEjaz/locrin/action@...` works from any repository.

`version: latest` follows the newest release, so the binary can move ahead of the
action ref; a pinned `version` matches the ref, which is what the examples do.

There is one comment, not a wall of them. The summary starts with the marker
`<!-- locrin-report -->` on its own line, and the action edits the first
pull-request comment whose body starts with that marker. A new comment is
written only when no marked comment exists, so every push edits the same one.

The action sets `LOCRIN_RUN_URL` to the run's own URL, which puts a `Details:`
link at the end of the comment. Set it yourself on any other CI and
`--markdown` does the same thing there.

`action/README.md` documents every input and output, and `action/examples/`
holds ready-to-copy workflows.

## Deployment gate

`locrin check --since <ref>` checks the files changed by the commits in
`ref..HEAD`, which is what a deployment needs: not what one pull request
touched, but everything that has landed since the last release went out.

Tag each deploy, and the ref is the last tag:

```yaml
- id: last
  run: echo "tag=$(git describe --tags --match 'deploy-*' --abbrev=0 2>/dev/null || git rev-list --max-parents=0 HEAD | head -n 1)" >> "$GITHUB_OUTPUT"
- uses: BilalEjaz/locrin/action@v0.5.0
  with:
    since: ${{ steps.last.outputs.tag }}
    comment: "false"
# deploy steps follow; they never run when the gate blocks
```

On a repository with no deploy tag yet the fallback diffs against the root
commit, which checks every file touched since the first commit; run one
whole-repository `locrin check` first if you want a full sweep. The exit codes
below drive the pipeline: a BLOCK fails the step, and the
steps after it do not run, so nothing ships over a blocking finding. Set
`fail-on-block: false` to read the `status` output and decide yourself.

## Exit codes

- `0`: pass, or advisory findings only.
- `1`: block. At least one high-confidence finding.
- `2`: the engine itself failed.

The hooks are the exception. `post-edit` and `stop` always exit 0 and say what
happened in their JSON; `pre-commit` uses the codes above.

## The index

The index is a SQLite database outside the repository, so a scan never dirties
the working tree. It lives in the platform cache directory under `locrin`, keyed
by a hash of the canonical root, so two checkouts of the same project do not
share one database.

Set `LOCRIN_CACHE_DIR` to put it somewhere else. Give a CI job or a sandbox its
own cache so runs do not share incremental state. When an upgraded binary meets
an index an older one wrote, it rebuilds it with one warning on stderr and
carries on.

Restart Claude Code after upgrading locrin, so that `locrin mcp` restarts too. A
long-running server on the old build and a hook on the new one disagree about
the schema, and each would rebuild the index the other had just written.

## Config

`locrin.toml` at the repository root. Every key is optional and an absent key
keeps the default, so a repository with no config file is a repository with all
the defaults. An unknown key, or an invalid glob in any list, fails the run and
names the mistake: a typo that was silently ignored would leave you believing an
override is in force when it is not.

| Key | Default | What it does |
| --- | --- | --- |
| `excludes` | `[]` | Globs the engine never reads, for generated or vendored code. |
| `debug_allowed` | `["**/scripts/**", "**/*.config.*", "**/bin/**"]` | Files where debug output is allowed. |
| `entry_points` | `[]` | Extra entry points for the dead-code rules, beyond package.json `main`, `bin` and `exports`. |
| `boundaries` | `[]` | Import directions. Each entry sets `from` and exactly one of `forbid` or `allow`, with an optional `name`. |
| `framework.auth_middleware` | `[]` | Identifiers that mark an Express route as authenticated. `express-route-without-auth` runs only when you have named one. |
| `framework.server_paths` | `["supabase/functions/**", "server/**", "api/**", "scripts/**", "**/*.server.*", "**/*.test.*", "**/*.spec.*"]` | Globs for code that runs on a server, so a service-role key there is not client exposure. |
| `languages.php` | `false` | Reads `.php` and `.phtml` files too. See "Languages" for what runs on them. |
| `languages.python` | `false` | Reads `.py` files too (`.pyi` stubs are never read). |
| `rules.<id>.enabled` | the rule's own default | Turns one rule on or off. |
| `rules.<id>.severity` | the rule's own default | Overrides one rule's severity. |
| `rules.<id>.languages` | the rule's own per-language defaults | The languages that rule reports on, replacing its defaults. |

`languages` takes the names the engine prints: `typescript`, `tsx`,
`javascript`, `php`, `python`. The list replaces the rule's per-language
defaults rather than adding to them, so it both turns a pair on and narrows a
rule to the languages you name. It is the escape from a per-language off that
`enabled = true` deliberately is not:

```toml
[rules.leftover-commented-code]
# on for Python too, which it ships off for; see "Per-language defaults"
languages = ["typescript", "tsx", "javascript", "python"]
```

A name that is not a language, a language the rule was not written against, or
an empty list fails the run and says what to write instead. `enabled = false`
still wins: this key says where a rule reports, not whether it runs.

`secret-exposed` is locked: it ignores `enabled` and `severity` and refuses
`languages`, because narrowing where a locked rule reports is how it would be
silenced. A repository that wants a locked finding to stop failing the build
accepts it into the baseline, where the acceptance is written down with a
reason.

## Baseline and suppression

`locrin-baseline.json` holds accepted findings by id, each with the rule, the
file, a reason, an author and a date. A finding in the baseline is filtered out
of every verdict. An id identifies whatever the rule that reported it anchored
on: a rule that reports a line anchors on the enclosing symbol, the line's text
and which of the identical lines in that symbol it is, so every occurrence is
accepted on its own; a rule that reports a class, such as a secret's value,
anchors on the class and one acceptance covers every place it appears.

Finding ids changed in 0.2.0, so a baseline written by 0.1.0 is stale in every
entry and suppresses nothing. A line rule's anchor now carries the line's own
text and which of the identical lines inside its symbol it is, and an advisory
carries the installed version, which is what makes two identical debug lines in
one function two findings to accept and three installed versions of one
vulnerable package three. Recreate the file with `locrin baseline create` after
upgrading, from a working tree you are willing to accept as it stands. The
findings cache is keyed by the version as well, so the first run after the
upgrade rebuilds it and pays for one cold pass rather than serving a row that
carries an old id.

For a single line, the text `locrin:allow` on that line suppresses the findings
reported there. The indexer records which lines carry it, so the suppression
holds even for findings on a file the run did not parse.

## Rules

Twenty-one rules ship today. Confidence, not severity, decides whether a finding
blocks: a high-confidence finding blocks, a medium-confidence one is advisory.
That is why a low-severity rule can still stop a build and a high-severity one
may not.

| Rule | Category | Severity | Confidence | On by default |
| --- | --- | --- | --- | --- |
| `leftover-debug` | erosion | high | high | yes |
| `leftover-commented-code` | erosion | medium | medium | yes; off on Python |
| `leftover-agent-marker` | erosion | low | medium | yes |
| `unused-import` | erosion | low | high | yes |
| `unreachable` | erosion | medium | high | yes |
| `dead-export` | erosion | low | medium | yes |
| `dead-file` | erosion | medium | medium | no |
| `boundary-violation` | erosion | high | high | yes |
| `swallowed-error` | erosion | medium | high | no |
| `test-no-assert` | erosion | low | medium | yes |
| `test-newly-skipped` | erosion | medium | high | yes |
| `secret-exposed` | security | high | high | yes (locked) |
| `weak-crypto` | security | high | high | yes |
| `injection-sink` | security | high | high | no |
| `html-injection` | security | high | medium | yes |
| `vulnerable-dependency` | security | high | medium | yes |
| `supabase-service-role-in-client` | security | high | high | yes |
| `supabase-table-without-rls` | security | high | high | yes |
| `express-route-without-auth` | security | high | high | yes |
| `express-cors-wildcard-on-authenticated` | security | high | high | yes |
| `express-cookie-insecure` | security | high | high | yes |

Three rules ship off: `dead-file`, `swallowed-error` and `injection-sink`. Each
was measured against real repositories and did not clear the precision bar the
spec sets, for a reason that is answerable per repository and not in general:
`dead-file` needs that repository's entry points curated, `injection-sink` needs
its fixtures baselined, and `swallowed-error` is a review aid rather than a
gate. Turn any of them on with `rules.<id>.enabled = true`.
`boundary-violation` is on but silent until you write a `[[boundaries]]` entry,
and `express-route-without-auth` is on but silent until you name an auth
middleware.

One rule is on everywhere but one language, which the table says in its column:
`leftover-commented-code` is off on Python. See "Per-language defaults" below
for that measurement and for what overrides it.

## Languages

TypeScript, TSX and JavaScript are read with no configuration, and every rule
runs on them. PHP and Python are read only when you ask:

```toml
[languages]
php = true      # .php and .phtml
python = true   # .py, never .pyi
```

`locrin init` writes that block commented out. Off is not a filter applied
late: a file in a language you have not enabled is never opened, never parsed
and never indexed, so turning the flag on is the only thing that changes what
the engine sees. A scan that walks past such files says so in a note naming the
flag, once per language, rather than silently skipping them.

### What runs on them

Five rules: `leftover-debug` (PHP's dump family and `xdebug_break`; Python's
`breakpoint()`, `pdb` and its relatives, never `print`),
`leftover-commented-code`, `leftover-agent-marker`, `secret-exposed` and
`vulnerable-dependency` (`composer.lock` against Packagist; `requirements.txt`
and `poetry.lock` against PyPI).

The other sixteen rules stay TypeScript-only. Every dead-code, import and
boundary rule (`unused-import`, `unreachable`, `dead-export`, `dead-file`,
`boundary-violation`), the two test rules, `swallowed-error`, `weak-crypto`,
`injection-sink`, `html-injection` and all five framework rules (Supabase and
Express) are written against the TypeScript grammar, and a rule declares the
languages it can read, so they never see a PHP or Python file. That declaration
is machine-readable: each rule in a SARIF report carries
`properties.languages`, so a consumer can tell a rule that was silent because
it found nothing from one that was never asked. PHP and Python symbols go into
the same index as everything else, so a later plan can put the graph rules on
them; today they do not run.

PHP is parsed with the mixed HTML and PHP grammar, so Blade and plain templates
parse rather than error. A file that fails to parse degrades the same way it
does in TypeScript: the file is recorded with its error and the rules that need
a tree skip it.

### Per-language defaults

A rule can ship off for one language while staying on for the others, and two
pairs of the ten were decided that way:

| Pair | Default | Why |
| --- | --- | --- |
| `leftover-commented-code` on Python | **off** | 0 true of 14 on Poetry in round two, every one a prose comment whose header ends in a colon. A colon now counts only behind a suite keyword, which removed thirteen; one prose `with ... :` header remains, and one false positive is still a fail |
| `leftover-agent-marker` on PHP | **on** (was off in round two) | One `XXX` inside `avatars/XXX.jpg` on Monica. A marker word inside a path or a file name no longer counts, and the re-measure is 4 true of 4 |

The other eight pairs ship on. A per-language off is not something
`rules.<id>.enabled = true` overrides: that key says whether the rule runs at
all, and the languages a rule failed on are the engine's measurement rather
than the repository's choice. The knob for that is `rules.<id>.languages`: name
the languages the rule reports on and the list replaces the defaults, which is
how a repository that has measured a pair for itself turns it on. See "Config"
for the key.

### Corpora and numbers

All ten rule-and-language pairs were measured on real repositories over three
rounds, written up in
`docs/superpowers/plans/2026-09-11-php-and-python-precision.md`:

| Round | PHP corpus | Python corpus |
| --- | --- | --- |
| One | BookStack `v26.05.4`, 2152 files | FastSpot (a private bot), 96 files |
| Two | Monica `v4.1.2`, 1800 files | Poetry `2.4.3`, 438 files |
| Three | Monica again, after the fixes | Poetry again, after the fixes |

Round three's numbers, each pair re-measured on the corpus that failed it:

| Pair | Round two | Round three | True | Default |
| --- | --- | --- | --- | --- |
| `leftover-agent-marker` on PHP (Monica) | 5 findings | 4 | 4 of 4 | on |
| `leftover-commented-code` on Python (Poetry) | 14 findings | 1 | 0 of 1 | off |
| `vulnerable-dependency` on PyPI (Poetry) | 25 findings | 13 | 13 of 13 | on |

The PyPI drop from 25 to 13 is not a precision fix but a de-duplication: PyPI
and Packagist advisories arrive under several ids (a GHSA record and the PYSEC
or CVE record that aliases it), and `vulnerable-dependency` now reports one
finding per family, under the GHSA id. Every GHSA in the round-two list is
still reported. Both registries publish `ECOSYSTEM` ranges, which 0.4.0 did not
order, so its findings pointed at the advisory and named no version; the next
version orders them by PEP 440 and by Composer's normaliser, and names the fix
for the range the installed version falls in.

Two limits worth knowing. Five of the ten pairs have never produced five
findings on a real repository, which is the smallest sample the gate scores, so
they ship on fixture evidence and a probe that the path works rather than on a
measured rate. And the locked `secret-exposed` reported a private key in a
BookStack test helper, which is accurate and is not something a maintainer
acts on; a fixture key under `tests/` is accepted into the baseline with a
reason, like any other locked finding.

The public numbers, precision and recall per rule on a corpus anyone can
rerun, live in their own repository:
https://github.com/BilalEjaz/locrin-benchmark

### Speed

Spec 3.4's targets name a TypeScript repository, so the benchmark suite
measures one (the FastLift checkout, 1846 files), and the PHP number is
recorded beside it rather than gated. Release build, one repository at a time,
each cold scan into a cache of its own after a warm-up spawn and a throwaway
scan:

| Benchmark | Target | Measured |
| --- | --- | --- |
| Cold index, whole repository (TypeScript, 1846 files) | under 5 s | 4.71 s |
| Warm single-file check | under 300 ms | 215 ms |
| Warm 30-file check (a typical pull request) | under 1 s | 362 ms |
| PostToolUse hook, end to end | under 300 ms | 237 ms |
| Startup (`--help`) | under 50 ms | 21 ms |
| Cold index, PHP (BookStack, 2152 files, 1773 of them PHP) | recorded, not gated | 6.56 s |

Run them with `cargo test --release -p locrin -- --ignored --nocapture`;
the PHP one needs `LOCRIN_PHP_BENCH_REPO` pointed at a checkout whose
`locrin.toml` enables PHP, and says so and measures nothing without it.

The two cold numbers are 2.6 ms and 3.0 ms per file, so the HTML-and-PHP
grammar is roughly the cost of the TypeScript one and not a different order.
Both cold figures move with the box: the same suite recorded 3.8 s for the
TypeScript cold index the day before, on a quieter machine, and a first read of
a checkout the operating system has just written costs several times either
number while the file cache and the virus scanner catch up. The warm numbers,
which are what a hook and a pull request actually wait for, are stable.

## The network

`vulnerable-dependency` is the only rule that touches the network. It queries
osv.dev for the installed dependency versions and caches the answer, and every
run after the first serves that snapshot.

`--offline` skips it, using the cached snapshot when there is one and warning
when there is not. The three hooks are always offline, and the MCP server is
offline unless you start it with `--online`.
