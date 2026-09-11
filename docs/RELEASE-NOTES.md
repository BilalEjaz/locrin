# Locrin release notes

## 0.4.0

PHP and Python are read when the repository asks for them. `[languages]` in
`locrin.toml` turns each one on (`php = true`, `python = true`); the JavaScript
family is always on and has no flag. A language that is off is not walked, not
parsed and not indexed, so nothing changes until a repository opts in.

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
