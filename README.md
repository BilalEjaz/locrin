# Locrin

Locrin is a deterministic quality gate for TypeScript and JavaScript, built for
code written by people and by agents. It parses with tree-sitter, keeps a
SQLite index of the repository outside the working tree, and answers with a
verdict: pass, advisory, or block. The same engine serves a person on the
command line, a git pre-commit hook, Claude Code's hooks, and an MCP client.

There is no model in the loop and no scoring. A rule either fires on what is in
the file or it does not, so two runs on the same bytes give the same answer.

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
(tree-sitter-typescript issue 322, open), and the error swallows the rest of the
file, so the file is excluded from every rule with one `warning: parse errors`
line on stderr. Write the type as `Array<import('./t').Seg>`, or import it by
name and use `Seg[]`, and the file parses and is checked like any other. Two
things that look like the same problem are not: a bare `&` in JSX text, as in
`BODY & NUTRITION`, is tolerated, because the scanner's refusal leaves the rest
of the tree intact and the only thing lost is the words after the `&` in that
one text run, which no rule reads today; and a NUL byte in source parses,
because the parser is handed a copy with each NUL replaced by a byte the lexer
does not reserve, one byte for one byte, while every rule reads the file as it
is written.

## Install

```
cargo install --path crates/cli
```

That is the only supported install for now. An npm package and a Homebrew
formula are phase two.

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
  scanning uploads. `--offline` skips the network. Paths and the diff scopes
  cannot be combined: each names its own set of files.
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
| `rules.<id>.enabled` | the rule's own default | Turns one rule on or off. |
| `rules.<id>.severity` | the rule's own default | Overrides one rule's severity. |

`secret-exposed` is locked: it ignores both `rules` keys. A repository that
wants a locked finding to stop failing the build accepts it into the baseline,
where the acceptance is written down with a reason.

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
| `leftover-commented-code` | erosion | medium | medium | yes |
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

## The network

`vulnerable-dependency` is the only rule that touches the network. It queries
osv.dev for the installed dependency versions and caches the answer, and every
run after the first serves that snapshot.

`--offline` skips it, using the cached snapshot when there is one and warning
when there is not. The three hooks are always offline, and the MCP server is
offline unless you start it with `--online`.
