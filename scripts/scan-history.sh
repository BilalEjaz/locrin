#!/usr/bin/env bash
# Pre-flight for going public: every commit reachable from any ref is checked out
# into a temporary directory and run through locrin's secret-exposed rule, then
# gitleaks (pinned, checksum-verified) scans the whole history in one pass.
#
# Findings whose file path starts with a prefix in the allowlist are counted and
# summarised rather than treated as failures: that file holds the secret rule's
# own synthetic test material. Everything else fails the scan.
#
# Usage: scripts/scan-history.sh [repo]   (default: this repository)
# Env:   SCAN_HISTORY_ALLOW   path to the allowlist file
#                             (default: scripts/scan-history-allow.txt)
#        LOCRIN_CACHE_DIR, GITLEAKS_CACHE
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
repo="${1:-$(cd "$here/.." && pwd)}"
allow="${SCAN_HISTORY_ALLOW:-$here/scan-history-allow.txt}"
GITLEAKS_VERSION="8.30.1"
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
status=0

[[ -f "$allow" ]] || { echo "scan-history: allowlist not found: $allow" >&2; exit 2; }

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
locrin_allowed=0
while read -r commit; do
  tree="$tmp/tree"; rm -rf "$tree"; mkdir -p "$tree"
  git -C "$repo" archive "$commit" | tar -x -C "$tree"
  if [[ -z "$(ls -A "$tree")" ]]; then continue; fi
  out=$(cd "$tree" && LOCRIN_CACHE_DIR="${LOCRIN_CACHE_DIR:-$tmp/cache}" locrin check --sarif --offline . 2>/dev/null || true)
  # A report that does not parse is a scan that did not happen: fail closed.
  # Each finding is printed as "a <file> <line>" when allowlisted, "x ..." when not.
  if ! hits=$(printf '%s' "$out" | "$py" -c '
import json,sys,urllib.parse
pre=[]
with open(sys.argv[1],encoding="utf-8") as fh:
    for line in fh:
        line=line.strip()
        if line and not line.startswith("#"): pre.append(line)
try: d=json.load(sys.stdin)
except Exception: sys.exit(1)
for run in d.get("runs",[]):
    for r in run.get("results",[]):
        if r.get("ruleId")!="secret-exposed": continue
        p=(r.get("locations") or [{}])[0].get("physicalLocation",{})
        f=urllib.parse.unquote(p.get("artifactLocation",{}).get("uri") or "").replace("\\","/")
        while f.startswith("./"): f=f[2:]
        tag="a" if any(f.startswith(x) for x in pre) else "x"
        print(tag,f,p.get("region",{}).get("startLine"))
' "$allow"); then
    echo "scan-history: locrin produced no readable report for commit $commit" >&2; status=1
    continue
  fi
  [[ -n "$hits" ]] || continue
  n=$(printf '%s\n' "$hits" | grep -c '^a ' || true)
  locrin_allowed=$((locrin_allowed + n))
  bad=$(printf '%s\n' "$hits" | sed -n 's/^x //p')
  if [[ -n "$bad" ]]; then
    echo "secret-exposed in commit $commit:" >&2; echo "$bad" >&2; status=1
  fi
done < <(git -C "$repo" rev-list --all)
echo "locrin: allowlisted: $locrin_allowed hits under fixture paths"

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
# The findings go to a JSON report so the allowlist can be applied to them.
# --exit-code 1 means "leaks found", which is not by itself fatal here; what
# matters is whether any of them sits outside the allowlist. --redact keeps the
# values themselves out of both the report and the terminal.
report="$tmp/gitleaks.json"
gl_rc=0
"$bin" git --no-banner --redact --exit-code 1 --report-format json --report-path "$report" "$repo" || gl_rc=$?
[[ -f "$report" ]] || { echo "scan-history: gitleaks wrote no report (exit $gl_rc)" >&2; exit 2; }
if ! gl_out=$("$py" -c '
import json,sys
pre=[]
with open(sys.argv[2],encoding="utf-8") as fh:
    for line in fh:
        line=line.strip()
        if line and not line.startswith("#"): pre.append(line)
try:
    with open(sys.argv[1],encoding="utf-8") as fh: d=json.load(fh)
except Exception: sys.exit(1)
allowed=0; bad=[]
for f in (d or []):
    p=(f.get("File") or "").replace("\\","/")
    if any(p.startswith(x) for x in pre): allowed+=1
    else: bad.append("  {} {}:{} {} ({})".format(
        (f.get("Commit") or "")[:12], p, f.get("StartLine"),
        f.get("RuleID"), f.get("Description")))
print("allowlisted",allowed)
for b in bad: print(b)
' "$report" "$allow"); then
  echo "scan-history: gitleaks report could not be read (exit $gl_rc)" >&2; exit 2
fi
gl_allowed=$(printf '%s\n' "$gl_out" | sed -n '1s/^allowlisted //p')
gl_bad=$(printf '%s\n' "$gl_out" | tail -n +2)
echo "gitleaks: allowlisted: ${gl_allowed:-0} hits under fixture paths"
if [[ -n "$gl_bad" ]]; then
  echo "gitleaks findings outside the allowlist:" >&2
  printf '%s\n' "$gl_bad" >&2
  status=1
fi
exit "$status"
