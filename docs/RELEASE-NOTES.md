# Locrin release notes

## Unreleased

### What changes without opting in

Three rules are off by default: `leftover-debug`, `html-injection` and
`supabase-service-role-in-client`. The public benchmark's second wave (302
agent-written diffs, labelled by two models) measured each of them far under
the 85 percent precision a rule needs to ship on:

| Rule | Findings | True | Why the rest were not |
| --- | --- | --- | --- |
| `leftover-debug` | 47 | 0 | A command line tool or script whose output is its purpose, a server's start-up line, a Worker's structured `console.log(JSON.stringify(..))` |
| `supabase-service-role-in-client` | 21 | 0 | Server code outside the default `framework.server_paths` (a root `server.ts`, an MCP server, a Playwright config, a `"use server"` file), and the Postgres role name inside SQL text |
| `html-injection` | 8 | 1 | A constant or a static string interpolated, an escaped value, an inert `<template>` |

`leftover-debug` blocked, so this is the change most likely to be noticed: a
`console.log` no longer stops a check. Nothing is removed. A project that
wants a rule back writes

```toml
[rules.leftover-debug]
enabled = true
```

and a project that already has such an entry, or a `severity` override with
`enabled = true` beside it, is unchanged. A baseline entry for one of these
rules is left alone and matches again if the rule is turned back on.

`dead-export` reports more, because a file with a parse error no longer counts
as using everything in every module it imports. Such a file is excluded from
the rules, and until now each of its imports was also widened to the whole
module, on the reasoning that names read from a broken tree cannot be trusted.
That was too broad. The TypeScript grammar errors on a line most mocked Vitest
files carry, `await importOriginal<typeof import("./mod")>()`, and one such
line made a test file an importer of every export of every module it names. On
one measured repository 223 of 2,767 files had a parse error, 219 of them
tests, and between them they marked 620 modules as wholly used: the public
benchmark recorded 49 unused exports in one change to that repository and the
engine reported none of them.

An import statement the parser read from its first token to its last now keeps
its names, whatever else in the file failed to parse. An import statement that
recovery patched (an ERROR or MISSING node inside it, or the statement sitting
inside an ERROR node) still takes the whole module, and so does a module the
text of the file imports more often than the tree read, which is how an import
that recovery dropped shows up. On the repository above the change adds 468
`dead-export` findings and removes none, the 49 recorded misses among them.
Expect new findings in a project that mocks modules with `importOriginal`; a
baseline taken before this release will not hold them.

A deeply nested file no longer ends the run. A union type, a string
concatenation and a chain of calls are trees with one level per member, the
rules walk a tree by recursion, and the default thread stacks end near two
thousand levels: a generated file with a 19,307-member union overflowed the
stack, and the process died with no report and an exit code that is none of 0,
1 and 2. The engine now runs on stacks sized for 100,000 levels, which costs
address space and no memory until a deep tree is met, and such a file is read
and reported on like any other. A file nested deeper than 100,000 levels is
not walked: the run prints `warning: <file> is nested more than 100000 levels
deep; excluded from rules`, carries on, and treats the file as using the whole
of every module its text imports.

## 0.6.1

### What changes without opting in

`leftover-debug` no longer reports on a JavaScript or TypeScript test file. A
file is a test when its name ends in `.test`, `.spec`, `-test` or `_test`
before the extension, or when a directory on its path is exactly `test`,
`tests`, `__tests__` or `spec`. A test's `console.log` is its report, the pass
and fail lines a runner prints, and there is no logger in it to route them
through: on the public benchmark every remaining false positive of this rule,
four of its seven findings, was such a line at the end of
`test/scope-prop-test.mjs`, a file with no shebang that a runner finds with
`readdirSync` rather than a package.json naming it. The three true findings
were in API routes and a component and are still reported. The match is
exact, so `latest.ts`, `contest.ts` and a `src/testing/` directory are not
tests. A `debugger` statement in a test file is exempt with the rest of it;
PHP and Python test files are unchanged, because a `breakpoint()` left in one
is a leftover wherever it sits.

## 0.6.0

### What changes without opting in

`leftover-commented-code` is off. It has yet to produce a true finding on any
measured code: 0 of 5 on the public benchmark, every one a prose comment block,
and on a large application 0 of 99 under 0.5.0 and still 0 of 8 after this
release's prose veto. A repository that wants it as a review aid writes

```toml
[rules.leftover-commented-code]
enabled = true
```

and it comes back on by default when a measurement clears the spec's bar.

`swallowed-error` is on. It shipped off on an older measurement over five
private repositories; the public benchmark
(https://github.com/BilalEjaz/locrin-benchmark, 217 agent-written diffs from
public repositories, every finding labelled by two models) scored it 92 percent
precision and 100 percent recall on 2026-09-16, above the spec's bar of 85, so
it now runs where the config says nothing about it. A repository that would
rather not have it writes

```toml
[rules.swallowed-error]
enabled = false
```

or marks the individual catches it means to keep with `locrin:allow`.
`dead-file` and `injection-sink` scored nothing true on the same corpus and stay
off.

The findings cache is keyed by the version, so the first run after the upgrade
rebuilds it and pays for one cold pass rather than serving rows written by
0.5.0.

`leftover-debug` no longer reports on a JavaScript or TypeScript file that is a
script. A file is a script when its first line starts with `#!`, or when a
package.json in the repository runs it directly, meaning a `scripts` value holds
`node <path>`, `node --<flag> <path>`, `tsx <path>` or `ts-node <path>` naming
it, resolved against that package.json's own directory. Printing is how a
standalone script speaks and there is no logger in it to route the line
through, so every `console.log` in one used to be a false positive. PHP and
Python files are unchanged.

`leftover-debug` also no longer reports on a JavaScript or TypeScript file
holding twenty or more flagged `console` calls. A module that logs that much
through `console` and imports no logger has adopted `console` as its logger, and
reporting it one line at a time says nothing a reader can act on. A file below
that count is reported on exactly as before.

`leftover-commented-code` reports a block only when most of it looks like code,
and a sentence never counts as code however it opens or closes. A statement
keyword now opens a statement only with the shape of one on the same line, so a
paragraph beginning `let the table carry the advice` or `$config is the host`
is prose rather than a disabled assignment. A commented-out function under one
line of explanation is still reported. Prose blocks that 0.5.0 reported
disappear; no block that is genuinely code stops being reported.


## 0.5.0

0.5.0 is the first public release. The repository is open under MIT, and the
binary installs from npm (`locrin`), PyPI (`locrin`), Homebrew
(`BilalEjaz/locrin/locrin`), crates.io (`locrin`), or the installer scripts
at the repository root. The npm and PyPI packages carry the release binaries;
the installer scripts, the Homebrew formula and the GitHub Action download them
and verify against `SHA256SUMS`; `cargo install` builds from source. The
`locrin-cli` crate is renamed `locrin` so `cargo install locrin` works.

### What changes without opting in

Nothing in the verdicts. Rule ids, finding ids and exit codes are unchanged
from 0.4.0. The GitHub Action no longer needs a download token.

PyPI and Packagist findings name a version to move to. Those registries publish
their advisory ranges as `ECOSYSTEM`, which 0.4.0 counted rather than read, so
every such finding pointed at the advisory and no further. The engine now orders
`ECOSYSTEM` ranges with the registry's own rules, PEP 440 for PyPI and
Composer's normaliser for Packagist, and names the fix for the range the
installed version falls in. A range with a boundary the comparator cannot read
is still reported as a range that was never compared, rather than guessed at,
and `GIT` ranges and npm `ECOSYSTEM` ranges are unchanged. `SEMVER` ranges are
read exactly as they were.

`[rules.<id>] languages` says which languages a rule reports on. The list
replaces the rule's per-language defaults rather than adding to them, so it is
the escape from a per-language off that `enabled = true` deliberately is not:
`[rules.leftover-commented-code] languages = ["typescript", "tsx",
"javascript", "python"]` turns the Python pair on, and a shorter list narrows a
rule to the languages you name. `enabled = false` still wins. A name that is
not a language, a language the rule was not written against, or an empty list
fails the run and names what to write instead. The locked `secret-exposed`
refuses the key outright: it reports on every language it reads, and narrowing
that is how a locked rule would be silenced. The key reaches the findings
cache's key, so adding or changing one rebuilds the rows it affects, and a
rule's SARIF `properties.languages` reports what it runs on in this repository.

## 0.4.0

PHP and Python are read when the repository asks for them. `[languages]` in
`locrin.toml` turns each one on (`php = true`, `python = true`); the JavaScript
family is always on and has no flag. A language that is off is not walked, not
parsed and not indexed until the repository opts in.

What changes without opting in: Composer, Poetry and pinned `requirements.txt`
lockfiles are read and queried whenever present, whatever `[languages]` says,
since a `requirements.txt` in a TypeScript repository is still an install, and
an offline run warns once per lockfile with no snapshot. `leftover-agent-marker`
no longer counts marker words inside paths and file names in any language, so a
few advisory findings disappear. Advisories that alias each other collapse to
one finding under the GHSA id, so a baseline entry for the PYSEC copy may stop
matching. And `leftover-commented-code` ships off on Python with no config
override yet; a `[rules.<id>].languages` override is planned.

Five rules run on those files: leftover-agent-marker, leftover-debug,
leftover-commented-code, secret-exposed and vulnerable-dependency. Every other
rule declares the JavaScript family and is never handed a PHP or Python file.

vulnerable-dependency reads Composer (`composer.lock`), Poetry (`poetry.lock`)
and pip (`requirements.txt`) beside the three npm lockfiles, and asks osv.dev
about each package under the registry it was installed from: Packagist, PyPI or
npm. A repository holding several lockfiles is several questions with several
answers, each reported against the file that declares the package. Only pinned
`requirements.txt` lines are read, because a range is not a version, and
Composer branch aliases such as `dev-main` are left out because they name a
branch rather than a release.

The advisory snapshot key now carries the ecosystem as well as the package list,
so every snapshot cached by an earlier version is missed once, npm ones
included. An online run refetches silently; an offline run made before the next
online one warns that there is no cached snapshot and skips the rule, and the
next online run restores it.

tree-sitter moves from 0.23 to 0.24, with the PHP and Python grammars at 0.23.
