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

# An allowlist entry with no trailing slash is one exact file, not a prefix.
printf '# one exact file, no trailing slash\nsrc/keys.ts\n' > "$tmp/exact.txt"

# The file the entry names: allowlisted, so the scan passes.
if ! SCAN_HISTORY_ALLOW="$tmp/exact.txt" bash scripts/scan-history.sh "$tmp/elsewhere" >"$tmp/exact-out" 2>&1; then
  cat "$tmp/exact-out"; echo "an exact allowlist entry must cover the file it names"; exit 1
fi

# A sibling the entry only shares a prefix with: not allowlisted, so it fails.
git init -q "$tmp/bak"; echo 'export const a = 1;' > "$tmp/bak/a.ts"; mk "$tmp/bak" add a.ts; mk "$tmp/bak" commit -qm one
mkdir -p "$tmp/bak/src"; plant "$tmp/bak/src/keys.ts.bak"
mk "$tmp/bak" add "src/keys.ts.bak"; mk "$tmp/bak" commit -qm two
if SCAN_HISTORY_ALLOW="$tmp/exact.txt" bash scripts/scan-history.sh "$tmp/bak" >"$tmp/bak-out" 2>&1; then
  cat "$tmp/bak-out"; echo "src/keys.ts must not allowlist src/keys.ts.bak"; exit 1
fi
grep -q 'src/keys\.ts\.bak' "$tmp/bak-out" || { cat "$tmp/bak-out"; echo "report must name the file"; exit 1; }

# A gitleaks that errors must never read as a clean history. The double stands in
# for the pinned binary through GITLEAKS_CACHE; FAKE_RC, FAKE_REPORT and
# FAKE_STDERR drive it. The script resolves gitleaks.exe on Windows, so both
# names are written.
mkdir -p "$tmp/fakebin"
cat > "$tmp/fakebin/gitleaks" <<'FAKE'
#!/usr/bin/env bash
set -euo pipefail
rp=""; prev=""
for a in "$@"; do
  if [[ "$prev" == "--report-path" ]]; then rp="$a"; fi
  prev="$a"
done
if [[ -n "${FAKE_STDERR:-}" ]]; then printf '%s\n' "$FAKE_STDERR" >&2; fi
if [[ -n "${FAKE_REPORT:-}" && -n "$rp" ]]; then printf '%s' "$FAKE_REPORT" > "$rp"; fi
exit "${FAKE_RC:-0}"
FAKE
cp "$tmp/fakebin/gitleaks" "$tmp/fakebin/gitleaks.exe"
chmod +x "$tmp/fakebin/gitleaks" "$tmp/fakebin/gitleaks.exe"

# Control: the double wired up as a clean scan still passes, so the three cases
# below fail for the reason they name and not because the double is broken.
if ! FAKE_RC=0 FAKE_REPORT='[]' GITLEAKS_CACHE="$tmp/fakebin" \
  bash scripts/scan-history.sh "$tmp/clean" >"$tmp/fake-ok" 2>&1; then
  cat "$tmp/fake-ok"; echo "a clean gitleaks run must still pass"; exit 1
fi

# $1 label, $2 FAKE_RC, $3 FAKE_REPORT, $4 FAKE_STDERR.
fake_must_fail() {
  local rc=0
  FAKE_RC="$2" FAKE_REPORT="$3" FAKE_STDERR="$4" GITLEAKS_CACHE="$tmp/fakebin" \
    bash scripts/scan-history.sh "$tmp/clean" >"$tmp/fake-out" 2>&1 || rc=$?
  [[ $rc -eq 2 ]] || { cat "$tmp/fake-out"; echo "$1: expected exit 2, got $rc"; exit 1; }
  grep -q 'gitleaks' "$tmp/fake-out" || { cat "$tmp/fake-out"; echo "$1: the failure must name gitleaks"; exit 1; }
}

# A rejected invocation: exit 126, no report at all.
fake_must_fail "gitleaks exit 126" 126 '' ''
# Exit 1 with an empty report: the other meaning of 1, a scan that failed.
fake_must_fail "gitleaks exit 1 with no findings" 1 '[]' ''
# Exit 1 with findings, but a partial scan, so the count cannot be trusted.
fake_must_fail "gitleaks partial scan" 1 \
  '[{"File":"src/a.ts","StartLine":1,"Commit":"deadbeef","RuleID":"r","Description":"d"}]' \
  '3:00PM WRN 1 leaks found in partial scan'
