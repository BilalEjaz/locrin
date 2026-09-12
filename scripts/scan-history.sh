#!/usr/bin/env bash
# Pre-flight for going public: every commit reachable from any ref is checked out
# into a temporary directory and run through locrin's secret-exposed rule, then
# gitleaks (pinned, checksum-verified) scans the whole history in one pass.
# Usage: scripts/scan-history.sh [repo]   (default: this repository)
set -euo pipefail
repo="${1:-$(cd "$(dirname "$0")/.." && pwd)}"
GITLEAKS_VERSION="8.30.1"
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
status=0

# Git Bash on Windows ships python3 as an App Execution Alias that is not always
# a real interpreter, so take whichever of the two names resolves.
py=$(command -v python3 || command -v python || true)
[[ -n "$py" ]] || { echo "scan-history: no python3 or python on PATH" >&2; exit 2; }

# 7z is on PATH on GitHub runners. A local Windows install often is not, so fall
# back to the default install location and then to unzip, as package.sh does.
unpack_zip() {
  if command -v 7z >/dev/null 2>&1; then
    7z x -y -o"$2" "$1" >/dev/null
  elif [[ -x "/c/Program Files/7-Zip/7z.exe" ]]; then
    "/c/Program Files/7-Zip/7z.exe" x -y -o"$2" "$1" >/dev/null
  elif command -v unzip >/dev/null 2>&1; then
    unzip -oq "$1" -d "$2"
  else
    echo "scan-history: neither 7z nor unzip found, cannot unpack the gitleaks zip" >&2
    return 1
  fi
}

# Part 1: locrin's own rule, one tree per commit.
# --sarif rather than --json: the agent JSON caps at ten findings, and a tree
# with ten other blocking findings would hide the one finding this gate is for.
while read -r commit; do
  tree="$tmp/tree"; rm -rf "$tree"; mkdir -p "$tree"
  git -C "$repo" archive "$commit" | tar -x -C "$tree"
  if [[ -z "$(ls -A "$tree")" ]]; then continue; fi
  out=$(cd "$tree" && LOCRIN_CACHE_DIR="${LOCRIN_CACHE_DIR:-$tmp/cache}" locrin check --sarif --offline . 2>/dev/null || true)
  # A report that does not parse is a scan that did not happen: fail closed.
  if ! hits=$(printf '%s' "$out" | "$py" -c '
import json,sys
try: d=json.load(sys.stdin)
except Exception: sys.exit(1)
for run in d.get("runs",[]):
    for r in run.get("results",[]):
        if r.get("ruleId")=="secret-exposed":
            p=(r.get("locations") or [{}])[0].get("physicalLocation",{})
            print(p.get("artifactLocation",{}).get("uri"), p.get("region",{}).get("startLine"))
'); then
    echo "scan-history: locrin produced no readable report for commit $commit" >&2; status=1
    continue
  fi
  if [[ -n "$hits" ]]; then
    echo "secret-exposed in commit $commit:" >&2; echo "$hits" >&2; status=1
  fi
done < <(git -C "$repo" rev-list --all)

# Part 2: gitleaks over the full history.
cache="${GITLEAKS_CACHE:-$HOME/.cache/locrin-gitleaks}"
bin="$cache/gitleaks"
case "$(uname -s)" in MINGW*|MSYS*|CYGWIN*) bin="$cache/gitleaks.exe" ;; esac
if [[ ! -x "$bin" ]]; then
  mkdir -p "$cache"
  case "$(uname -s)-$(uname -m)" in
    Linux-x86_64) a="linux_x64.tar.gz" ;;
    Linux-aarch64) a="linux_arm64.tar.gz" ;;
    Darwin-arm64) a="darwin_arm64.tar.gz" ;;
    Darwin-x86_64) a="darwin_x64.tar.gz" ;;
    MINGW*|MSYS*|CYGWIN*) a="windows_x64.zip" ;;
    *) echo "scan-history: no gitleaks build for this platform" >&2; exit 2 ;;
  esac
  asset="gitleaks_${GITLEAKS_VERSION}_$a"
  base="https://github.com/gitleaks/gitleaks/releases/download/v$GITLEAKS_VERSION"
  curl -fsSL "$base/$asset" -o "$cache/$asset"
  curl -fsSL "$base/gitleaks_${GITLEAKS_VERSION}_checksums.txt" -o "$cache/sums"
  want=$(grep " $asset\$" "$cache/sums" | awk '{print $1}')
  if command -v sha256sum >/dev/null; then got=$(sha256sum "$cache/$asset" | awk '{print $1}'); else got=$(shasum -a 256 "$cache/$asset" | awk '{print $1}'); fi
  [[ -n "$want" && "$want" == "$got" ]] || { echo "scan-history: gitleaks checksum mismatch" >&2; exit 2; }
  if [[ "$a" == *.zip ]]; then unpack_zip "$cache/$asset" "$cache"; else tar -xzf "$cache/$asset" -C "$cache"; fi
  chmod +x "$bin"
fi
# -v so a gitleaks-only hit names its commit, file and line, which is what this
# script promises; --redact keeps the value itself out of the report.
if ! "$bin" git --no-banner --redact -v --exit-code 1 "$repo"; then
  echo "gitleaks reported findings" >&2; status=1
fi
exit "$status"
