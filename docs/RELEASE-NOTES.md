# Locrin release notes

## Unreleased

### What changes without opting in

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
