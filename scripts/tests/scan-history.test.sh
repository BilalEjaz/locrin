#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
command -v locrin >/dev/null || { echo "skip: locrin not on PATH"; exit 0; }
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
export LOCRIN_CACHE_DIR="$tmp/cache" GITLEAKS_CACHE="$tmp/gitleaks"

mk() { git -C "$1" -c user.name=t -c user.email=t@t -c commit.gpgsign=false "${@:2}"; }
# Clean repo: passes.
git init -q "$tmp/clean"; echo 'export const a = 1;' > "$tmp/clean/a.ts"; mk "$tmp/clean" add a.ts; mk "$tmp/clean" commit -qm one
bash scripts/scan-history.sh "$tmp/clean"

# A secret committed then deleted: still found in history. The planted value is a
# keyboard mash rather than the AWS documentation key: gitleaks and locrin both
# treat a value carrying "EXAMPLE" as a placeholder, so the documentation key
# would leave this test asserting nothing.
git init -q "$tmp/dirty"; echo 'export const a = 1;' > "$tmp/dirty/a.ts"; mk "$tmp/dirty" add a.ts; mk "$tmp/dirty" commit -qm one
printf 'const key = "AKIA%s";\n' "QWERTYUIOPASDFGH" > "$tmp/dirty/k.ts"; mk "$tmp/dirty" add k.ts; mk "$tmp/dirty" commit -qm two
mk "$tmp/dirty" rm -q k.ts; mk "$tmp/dirty" commit -qm three
if bash scripts/scan-history.sh "$tmp/dirty" >"$tmp/out" 2>&1; then cat "$tmp/out"; echo "planted secret must fail"; exit 1; fi
grep -q 'k.ts' "$tmp/out" || { cat "$tmp/out"; echo "report must name the file"; exit 1; }
