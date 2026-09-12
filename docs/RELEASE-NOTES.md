# Locrin release notes

## Unreleased

## 0.5.0

0.5.0 is the first public release. The repository is open under MIT, and the
binary installs from npm (`locrin`), PyPI (`locrin`), Homebrew
(`BilalEjaz/locrin/locrin`), crates.io (`locrin`), or the installer scripts
at the repository root. Every channel carries the same binary from the same
release, verified against `SHA256SUMS`. The `locrin-cli` crate is renamed
`locrin` so `cargo install locrin` works.

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
