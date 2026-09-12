#!/usr/bin/env bash
# Fails when a tracked file holds local paths, personal addresses or token
# prefixes. The scan is over `git ls-files` rather than the filesystem: tracked
# files are exactly what goes public, so build output, caches, virtualenvs and
# every other gitignored working file are out of scope by construction.
#
# Findings whose path is covered by the allowlist are dropped: that file holds
# the secret rule's own synthetic test material, and it is the same allowlist
# scripts/scan-history.sh reads. An entry ending in "/" covers every path under
# it; any other entry covers that one file and nothing else.
#
# Usage: scripts/check-private-refs.sh [root]
# Env:   SCAN_HISTORY_ALLOW   path to the allowlist file
#                             (default: scripts/scan-history-allow.txt)
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
root="${1:-$(cd "$here/.." && pwd)}"
allow="${SCAN_HISTORY_ALLOW:-$here/scan-history-allow.txt}"

[[ -f "$allow" ]] || { echo "check-private-refs: allowlist not found: $allow" >&2; exit 2; }
git -C "$root" rev-parse --git-dir >/dev/null 2>&1 || {
  echo "check-private-refs: not a git repository: $root" >&2; exit 2; }

# Literals are split so this script never matches itself.
who='ch''ars'; mail='gm''ail'; site='hikmah''learn'
patterns=(
  "Users[\\/]+$who"
  'AppData[\/]+Local'
  "[A-Za-z0-9._%+-]+@$mail\.com"
  "$site"
  'ghp_[A-Za-z0-9]{20,}'
  'github_pat_[A-Za-z0-9_]{10,}'
  'npm_[A-Za-z0-9]{30,}'
  'pypi-AgEIcHlwaS5vcmc[A-Za-z0-9_-]{20,}'
)
regex=$(IFS='|'; echo "${patterns[*]}")

entries=()
while IFS= read -r line || [[ -n "$line" ]]; do
  line="${line#"${line%%[![:space:]]*}"}"
  line="${line%"${line##*[![:space:]]}"}"
  [[ -z "$line" || "$line" == \#* ]] && continue
  entries+=("$line")
done < "$allow"

allowed() {
  local f="$1" e
  for e in ${entries[@]+"${entries[@]}"}; do
    if [[ "$e" == */ ]]; then
      [[ "$f" == "$e"* ]] && return 0
    else
      [[ "$f" == "$e" ]] && return 0
    fi
  done
  return 1
}

# Paths are printed relative to the root because the grep runs from there.
hits=$(cd "$root" && git -C "$root" ls-files -z \
  | xargs -0 -r grep -nEH --binary-files=without-match -- "$regex" || true)

bad=()
while IFS= read -r line; do
  [[ -n "$line" ]] || continue
  allowed "${line%%:*}" || bad+=("$line")
done <<< "$hits"

if [[ ${#bad[@]} -gt 0 ]]; then
  echo "private references found in tracked files:" >&2
  printf '%s\n' "${bad[@]}" >&2
  exit 1
fi
