#!/usr/bin/env bash
# Every `{ path = "...", version = "X" }` dependency must carry the workspace version,
# because crates.io ignores `path` and resolves by `version`.
# Usage: scripts/check-versions.sh [repo-root]
set -euo pipefail
root="${1:-$(cd "$(dirname "$0")/.." && pwd)}"
want=$(grep -m1 '^version' "$root/Cargo.toml" | sed 's/.*"\(.*\)"/\1/')
status=0
for manifest in "$root"/crates/*/Cargo.toml; do
  while IFS= read -r line; do
    got=$(sed -n 's/.*version *= *"\([^"]*\)".*/\1/p' <<<"$line")
    if [[ -z "$got" ]]; then
      echo "$manifest: path dependency without a version: $line" >&2; status=1
    elif [[ "$got" != "$want" ]]; then
      echo "$manifest: path dependency at $got, workspace is $want: $line" >&2; status=1
    fi
  done < <(grep -E '^\s*[A-Za-z0-9_-]+ *= *\{[^}]*path *=' "$manifest" || true)
done
exit "$status"
