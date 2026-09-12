#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT

# The check scans tracked files, so every fixture has to be a git repository.
mkdir -p "$tmp/clean/docs" "$tmp/dirty/docs"
echo 'The cache lives in the user cache directory.' > "$tmp/clean/docs/a.md"
git -C "$tmp/clean" init -q
git -C "$tmp/clean" config core.autocrlf false
git -C "$tmp/clean" add docs
bash scripts/check-private-refs.sh "$tmp/clean"

# The literals are split so this test file never matches its own patterns.
who='ch''ars'; mail='gm''ail'; tok='github_pat_''11ABCDEFG0abcdefghij'; bs='\'
gh='ghp_''0123456789abcdefghijABCDEF'
echo "see C:${bs}Users${bs}${who}${bs}x" > "$tmp/dirty/docs/a.md"
echo "mail someone@$mail.com" > "$tmp/dirty/docs/b.md"
echo "token $tok" > "$tmp/dirty/docs/c.md"
echo "from /c/Users/$who/x" > "$tmp/dirty/docs/d.md"
# Tracked but allowlisted: the secret rule's own synthetic fixtures.
fixture="crates/rules/tests/fixtures/secret_exposed"
mkdir -p "$tmp/dirty/$fixture"
echo "export const KEY = \"$gh\";" > "$tmp/dirty/$fixture/keys.ts"
# Untracked, with a hit: what is not tracked never goes public, so it must not
# be reported.
echo "from /c/Users/$who/x" > "$tmp/dirty/untracked.md"
git -C "$tmp/dirty" init -q
git -C "$tmp/dirty" config core.autocrlf false
git -C "$tmp/dirty" add docs
git -C "$tmp/dirty" add "$fixture/keys.ts"

if bash scripts/check-private-refs.sh "$tmp/dirty" > "$tmp/out" 2>&1; then echo "dirty tree must fail"; exit 1; fi
for f in a.md b.md c.md d.md; do grep -q "$f" "$tmp/out" || { cat "$tmp/out"; echo "must report $f"; exit 1; }; done
if grep -q 'untracked\.md' "$tmp/out"; then cat "$tmp/out"; echo "must not report an untracked file"; exit 1; fi
if grep -q 'keys\.ts' "$tmp/out"; then cat "$tmp/out"; echo "must not report an allowlisted path"; exit 1; fi

# The real tree, including this test and the plan that describes it, must be clean.
bash scripts/check-private-refs.sh "$(pwd)"
