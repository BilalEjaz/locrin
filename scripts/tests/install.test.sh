#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
case "$(uname -s)-$(uname -m)" in
  Linux-x86_64) target=x86_64-unknown-linux-gnu ;;
  Darwin-arm64) target=aarch64-apple-darwin ;;
  Darwin-x86_64) target=x86_64-apple-darwin ;;
  *) echo "skip: install.sh has no build for this platform"; exit 0 ;;
esac
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
rel="$tmp/releases/v9.9.9"; mkdir -p "$rel" "$tmp/bin"
printf '#!/bin/sh\necho locrin 9.9.9\n' > "$tmp/bin/locrin"; chmod +x "$tmp/bin/locrin"
bash scripts/package.sh "$target" v9.9.9 "$tmp/bin" "$rel" >/dev/null
(cd "$rel" && if command -v sha256sum >/dev/null; then sha256sum locrin-* > SHA256SUMS; else shasum -a 256 locrin-* > SHA256SUMS; fi)

LOCRIN_BASE_URL="file://$tmp/releases" LOCRIN_INSTALL_DIR="$tmp/out" bash install.sh v9.9.9
[[ "$("$tmp/out/locrin" --version)" == "locrin 9.9.9" ]] || { echo "installed binary wrong"; exit 1; }

sed -i.bak 's/^./0/' "$rel/SHA256SUMS"
if LOCRIN_BASE_URL="file://$tmp/releases" LOCRIN_INSTALL_DIR="$tmp/out2" bash install.sh v9.9.9 2>"$tmp/err"; then echo "mismatch must fail"; exit 1; fi
grep -q 'checksum mismatch' "$tmp/err" || { cat "$tmp/err"; exit 1; }
[[ ! -e "$tmp/out2/locrin" ]] || { echo "must not install on mismatch"; exit 1; }

: > "$rel/SHA256SUMS"
if LOCRIN_BASE_URL="file://$tmp/releases" LOCRIN_INSTALL_DIR="$tmp/out3" bash install.sh v9.9.9 2>"$tmp/err"; then echo "missing line must fail"; exit 1; fi
grep -q 'not listed' "$tmp/err" || { cat "$tmp/err"; exit 1; }
