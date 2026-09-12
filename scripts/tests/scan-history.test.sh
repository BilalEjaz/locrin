#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
command -v locrin >/dev/null || { echo "skip: locrin not on PATH"; exit 0; }
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
export LOCRIN_CACHE_DIR="$tmp/cache" GITLEAKS_CACHE="$tmp/gitleaks"

mk() { git -C "$1" -c user.name=t -c user.email=t@t -c commit.gpgsign=false "${@:2}"; }
# The planted value is a keyboard mash rather than the AWS documentation key:
# gitleaks and locrin both treat a value carrying "EXAMPLE" as a placeholder, so
# the documentation key would leave this test asserting nothing.
plant() { printf 'const key = "AKIA%s";\n' "QWERTYUIOPASDFGH" > "$1"; }

# Every case passes its own allowlist, so none of this depends on what the real
# repository's scripts/scan-history-allow.txt happens to hold.
printf '# nothing is allowlisted for these cases\n' > "$tmp/none.txt"
printf '# synthetic fixture material for this test\nfixtures/secret/\n' > "$tmp/allow.txt"
export SCAN_HISTORY_ALLOW="$tmp/none.txt"

# Clean repo: passes.
git init -q "$tmp/clean"; echo 'export const a = 1;' > "$tmp/clean/a.ts"; mk "$tmp/clean" add a.ts; mk "$tmp/clean" commit -qm one
bash scripts/scan-history.sh "$tmp/clean"

# A secret committed then deleted: still found in history.
git init -q "$tmp/dirty"; echo 'export const a = 1;' > "$tmp/dirty/a.ts"; mk "$tmp/dirty" add a.ts; mk "$tmp/dirty" commit -qm one
plant "$tmp/dirty/k.ts"; mk "$tmp/dirty" add k.ts; mk "$tmp/dirty" commit -qm two
mk "$tmp/dirty" rm -q k.ts; mk "$tmp/dirty" commit -qm three
if bash scripts/scan-history.sh "$tmp/dirty" >"$tmp/out" 2>&1; then cat "$tmp/out"; echo "planted secret must fail"; exit 1; fi
grep -q 'k.ts' "$tmp/out" || { cat "$tmp/out"; echo "report must name the file"; exit 1; }

# The same secret under an allowlisted path: counted, reported, not fatal.
git init -q "$tmp/allowed"; echo 'export const a = 1;' > "$tmp/allowed/a.ts"; mk "$tmp/allowed" add a.ts; mk "$tmp/allowed" commit -qm one
mkdir -p "$tmp/allowed/fixtures/secret"; plant "$tmp/allowed/fixtures/secret/keys.ts"
mk "$tmp/allowed" add "fixtures/secret/keys.ts"; mk "$tmp/allowed" commit -qm two
if ! SCAN_HISTORY_ALLOW="$tmp/allow.txt" bash scripts/scan-history.sh "$tmp/allowed" >"$tmp/allowed-out" 2>&1; then
  cat "$tmp/allowed-out"; echo "an allowlisted path must not fail the scan"; exit 1
fi
grep -Eq '^locrin: allowlisted: [1-9][0-9]* hits under fixture paths$' "$tmp/allowed-out" || {
  cat "$tmp/allowed-out"; echo "locrin half must report the allowlisted count"; exit 1; }
grep -Eq '^gitleaks: allowlisted: [1-9][0-9]* hits under fixture paths$' "$tmp/allowed-out" || {
  cat "$tmp/allowed-out"; echo "gitleaks half must report the allowlisted count"; exit 1; }

# The same secret under a path that allowlist does not cover: still fatal.
git init -q "$tmp/elsewhere"; echo 'export const a = 1;' > "$tmp/elsewhere/a.ts"; mk "$tmp/elsewhere" add a.ts; mk "$tmp/elsewhere" commit -qm one
mkdir -p "$tmp/elsewhere/src"; plant "$tmp/elsewhere/src/keys.ts"
mk "$tmp/elsewhere" add "src/keys.ts"; mk "$tmp/elsewhere" commit -qm two
if SCAN_HISTORY_ALLOW="$tmp/allow.txt" bash scripts/scan-history.sh "$tmp/elsewhere" >"$tmp/elsewhere-out" 2>&1; then
  cat "$tmp/elsewhere-out"; echo "a path outside the allowlist must fail"; exit 1
fi
grep -q 'src/keys.ts' "$tmp/elsewhere-out" || { cat "$tmp/elsewhere-out"; echo "report must name the file"; exit 1; }
