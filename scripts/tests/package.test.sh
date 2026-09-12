#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT

mkdir -p "$tmp/bin" "$tmp/out"
printf '#!/bin/sh\necho locrin 9.9.9\n' > "$tmp/bin/locrin"; chmod +x "$tmp/bin/locrin"
asset=$(bash scripts/package.sh x86_64-unknown-linux-gnu v9.9.9 "$tmp/bin" "$tmp/out")
[[ "$asset" == "$tmp/out/locrin-v9.9.9-x86_64-unknown-linux-gnu.tar.gz" ]] || { echo "unexpected asset path: $asset"; exit 1; }
tar -tzf "$asset" | grep -qx 'locrin-v9.9.9-x86_64-unknown-linux-gnu/locrin' || { echo "tarball layout wrong"; tar -tzf "$asset"; exit 1; }

cp "$tmp/bin/locrin" "$tmp/bin/locrin.exe"
asset=$(bash scripts/package.sh x86_64-pc-windows-msvc v9.9.9 "$tmp/bin" "$tmp/out")
[[ "$asset" == "$tmp/out/locrin-v9.9.9-x86_64-pc-windows-msvc.zip" ]] || { echo "unexpected zip path: $asset"; exit 1; }
if command -v unzip >/dev/null; then
  unzip -l "$asset" | grep -q 'locrin-v9.9.9-x86_64-pc-windows-msvc/locrin.exe' || { echo "zip layout wrong"; exit 1; }
else
  7z l "$asset" | grep -q 'locrin.exe' || { echo "zip layout wrong"; exit 1; }
fi

if bash scripts/package.sh riscv64gc-unknown-linux-gnu v9.9.9 "$tmp/bin" "$tmp/out" 2>/dev/null; then
  echo "unknown target must fail"; exit 1
fi
