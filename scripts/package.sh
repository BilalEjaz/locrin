#!/usr/bin/env bash
# Package a built locrin binary into the release asset for one target.
# Usage: scripts/package.sh <target> <tag> <bin-dir> <out-dir>
# Prints the asset path. The archive holds one directory, locrin-<tag>-<target>,
# with the binary inside, which is what action/action.yml and install.sh expect.
set -euo pipefail
target="$1"; tag="$2"; bin_dir="$3"; out_dir="$4"
case "$target" in
  x86_64-unknown-linux-gnu|aarch64-apple-darwin|x86_64-apple-darwin) ext=tar.gz; bin=locrin ;;
  x86_64-pc-windows-msvc) ext=zip; bin=locrin.exe ;;
  *) echo "package.sh: unknown target $target" >&2; exit 2 ;;
esac

# 7z is on PATH on GitHub runners. A local Windows install often is not on PATH,
# so fall back to the default install location before giving up.
sevenzip() {
  if command -v 7z >/dev/null 2>&1; then
    7z "$@"
  elif [[ -x "/c/Program Files/7-Zip/7z.exe" ]]; then
    "/c/Program Files/7-Zip/7z.exe" "$@"
  else
    echo "package.sh: 7z not found, cannot build a zip asset" >&2
    return 1
  fi
}

name="locrin-${tag}-${target}"
mkdir -p "$out_dir/$name"
cp "$bin_dir/$bin" "$out_dir/$name/$bin"
if [[ "$ext" == "zip" ]]; then
  (cd "$out_dir" && rm -f "$name.zip" && sevenzip a -tzip "$name.zip" "$name" >/dev/null)
else
  chmod +x "$out_dir/$name/$bin"
  tar -C "$out_dir" -czf "$out_dir/$name.tar.gz" "$name"
fi
rm -rf "${out_dir:?}/$name"
echo "$out_dir/$name.$ext"
