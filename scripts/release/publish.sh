#!/usr/bin/env bash
# Stages and publishes every channel from the release assets.
# Usage: scripts/release/publish.sh <assets-dir> <dry|live>
# <assets-dir> holds the four archives and SHA256SUMS. Publishing order: npm
# platform packages, npm entry, PyPI wheels, crates in dependency order, the
# Homebrew tap. A failure stops the script; the caller leaves the release marked
# pre-release, and re-running after a fix completes it (every step is idempotent
# for an already-published version except crates.io, which is skipped when
# cargo info finds the version on the crates-io registry; the query must name the
# registry, because inside the workspace a bare cargo info matches the local
# member and would skip every crate). Live mode publishes each crate with cargo's
# packaged-build verification on, so the four dependent crates are verified only
# on the live path: cargo waits for each crate to reach the index before the next
# publish, which is what lets their path dependencies resolve.
set -euo pipefail
assets="$1"; mode="$2"
[[ "$mode" == "live" || "$mode" == "dry" ]] || { echo "publish.sh: mode must be live or dry, got '$mode'" >&2; exit 2; }
cd "$(dirname "$0")/../.."
version=$(grep -m1 '^version' Cargo.toml | sed 's/.*"\(.*\)"/\1/')
tag="v$version"
work=$(mktemp -d)
# 7z is on PATH on GitHub runners. A local Windows install often is not on PATH,
# so fall back to the default install location, the same way scripts/package.sh
# does, or a local dry run cannot open the Windows zip.
sevenzip() {
  if command -v 7z >/dev/null 2>&1; then
    7z "$@"
  elif [[ -x "/c/Program Files/7-Zip/7z.exe" ]]; then
    "/c/Program Files/7-Zip/7z.exe" "$@"
  else
    echo "publish.sh: 7z not found, cannot open a zip asset" >&2
    return 1
  fi
}
extract() { # <target> -> prints the binary path
  local t="$1" d="$work/$1" bin=locrin p; mkdir -p "$d"
  if [[ "$t" == *windows* ]]; then
    bin=locrin.exe
    (cd "$d" && sevenzip x -y "$assets/locrin-$tag-$t.zip" >/dev/null)
  else
    tar -xzf "$assets/locrin-$tag-$t.tar.gz" -C "$d"
  fi
  # The archive holds one directory with the binary inside. A dry run dispatch
  # builds that directory under a suffixed tag, so match it by shape, not name.
  p=$(ls "$d"/*/"$bin")
  [[ -f "$p" ]] || { echo "publish.sh: no $bin in the $t archive" >&2; return 1; }
  echo "$p"
}
linux=$(extract x86_64-unknown-linux-gnu); darm=$(extract aarch64-apple-darwin); dx64=$(extract x86_64-apple-darwin); win=$(extract x86_64-pc-windows-msvc)
if [[ "$mode" == "live" ]]; then dry=""; else dry="--dry-run"; fi

echo "== npm $version"
node npm/scripts/stage.js --version "$version" --binary "$linux" --platform linux-x64 --binary "$darm" --platform darwin-arm64 --binary "$dx64" --platform darwin-x64 --binary "$win" --platform win32-x64
for p in linux-x64 darwin-arm64 darwin-x64 win32-x64; do
  if [[ "$mode" == "live" ]] && [[ "$(npm view "@raxbi/locrin-$p@$version" version 2>/dev/null)" == "$version" ]]; then echo "npm @raxbi/locrin-$p@$version exists, skipping"; continue; fi
  (cd "npm/platforms/$p" && npm publish --access public $dry)
done
if [[ "$mode" == "live" ]] && [[ "$(npm view "locrin@$version" version 2>/dev/null)" == "$version" ]]; then echo "npm locrin@$version exists, skipping"; else (cd npm/locrin && npm publish --access public $dry); fi

echo "== pypi $version"
python pypi/build_wheel.py --version "$version" --binary "$linux" --platform-tag manylinux_2_35_x86_64 --out "$work/whl"
python pypi/build_wheel.py --version "$version" --binary "$darm" --platform-tag macosx_11_0_arm64 --out "$work/whl"
python pypi/build_wheel.py --version "$version" --binary "$dx64" --platform-tag macosx_10_12_x86_64 --out "$work/whl"
python pypi/build_wheel.py --version "$version" --binary "$win" --platform-tag win_amd64 --out "$work/whl"
python -m twine check "$work"/whl/*.whl
if [[ "$mode" == "live" ]]; then python -m twine upload --skip-existing "$work"/whl/*.whl; fi

echo "== crates $version"
for c in locrin-core locrin-rules locrin-reporters locrin-mcp locrin; do
  if [[ "$mode" == "live" ]]; then
    if cargo info "$c@$version" --registry crates-io >/dev/null 2>&1; then echo "$c $version exists, skipping"; continue; fi
    cargo publish -p "$c"
  elif [[ "$c" == "locrin-core" ]]; then
    cargo publish -p "$c" --dry-run
  else
    # cargo package resolves the path dependencies against the registry, so it
    # refuses while locrin-core is not on crates.io ("no matching package named
    # locrin-core found"). List the package contents instead. The live path
    # publishes in dependency order and cargo waits for each crate to reach the
    # index before the next one.
    echo "dry run: $c depends on unpublished crates; listing the package contents"
    cargo package --list -p "$c" >/dev/null
  fi
done

echo "== homebrew $version"
bash scripts/homebrew-formula.sh "$version" "$assets/SHA256SUMS" > "$work/locrin.rb"
if [[ "$mode" == "live" ]]; then
  git clone --depth 1 "https://x-access-token:${HOMEBREW_TAP_TOKEN}@github.com/BilalEjaz/homebrew-locrin.git" "$work/tap"
  mkdir -p "$work/tap/Formula" && cp "$work/locrin.rb" "$work/tap/Formula/locrin.rb"
  (cd "$work/tap" && git -c user.name=locrin-release -c user.email=release@locrin.com add Formula/locrin.rb && (git -c user.name=locrin-release -c user.email=release@locrin.com commit -qm "locrin $version" || echo "formula unchanged") && git push -q)
else
  cat "$work/locrin.rb"
fi
echo "published $version ($mode)"
