#!/usr/bin/env bash
# Builds every install channel from the release binary just built and installs
# each one on this runner. Run by .github/workflows/ci.yml install-smoke.
# Usage: scripts/ci/install-smoke.sh <target> <npm-platform> <pypi-tag>
set -euo pipefail
cd "$(dirname "$0")/../.."
target="$1"; npm_platform="$2"; pypi_tag="$3"
version=$(grep -m1 '^version' Cargo.toml | sed 's/.*"\(.*\)"/\1/')
tag="v$version"
bin=target/release/locrin; [[ "$target" == *windows* ]] && bin=target/release/locrin.exe
work=$(mktemp -d)
# On Windows the tools this script drives are native, and none of them understand
# the MSYS /tmp path that mktemp prints. Keep the scratch directory in the C:/...
# form, which both this shell and node, python and PowerShell accept.
[[ "$target" == *windows* ]] && work=$(cygpath -m "$work")

echo "== package"
asset=$(bash scripts/package.sh "$target" "$tag" target/release "$work/rel/$tag")
echo "packaged $asset"
# The release job writes SHA256SUMS on Linux, where sha256sum prints the plain
# two space form that install.ps1 matches. The MSYS sha256sum on the Windows
# runner marks binary mode with a star instead, so normalise to the release form.
(cd "$work/rel/$tag" \
  && { if command -v sha256sum >/dev/null; then sha256sum locrin-*; else shasum -a 256 locrin-*; fi; } \
  | sed 's/ \*/  /' > SHA256SUMS)

echo "== npm"
node npm/scripts/stage.js --version "$version" --binary "$bin" --platform "$npm_platform"
mkdir -p "$work/npm-proj" "$work/npm-tgz"
npm pack --pack-destination "$work/npm-tgz" "./npm/platforms/$npm_platform" "./npm/locrin" >/dev/null
(cd "$work/npm-proj" && npm init -y >/dev/null && npm install --no-audit --no-fund "$work"/npm-tgz/*.tgz >/dev/null)
got=$("$work/npm-proj/node_modules/.bin/locrin" --version)
[[ "$got" == "locrin $version" ]] || { echo "npm launcher printed: $got"; exit 1; }
git checkout -- npm  # undo the version stamps

echo "== pypi"
whl=$(python pypi/build_wheel.py --version "$version" --binary "$bin" --platform-tag "$pypi_tag" --out "$work/whl")
python -m venv "$work/venv"
if [[ -d "$work/venv/Scripts" ]]; then vbin="$work/venv/Scripts"; else vbin="$work/venv/bin"; fi
"$vbin/pip" install --quiet "$whl"
got=$("$vbin/locrin" --version)
[[ "$got" == "locrin $version" ]] || { echo "pip launcher printed: $got"; exit 1; }

echo "== installers"
if [[ "$target" == *windows* ]]; then
  powershell -NoProfile -ExecutionPolicy Bypass -File scripts/tests/install.test.ps1
  powershell -NoProfile -ExecutionPolicy Bypass -File install.ps1 -Version "$tag" -BaseUrl "$(cygpath -w "$work/rel")" -InstallDir "$(cygpath -w "$work/inst")"
  got=$("$work/inst/locrin.exe" --version)
else
  bash scripts/tests/install.test.sh
  LOCRIN_BASE_URL="file://$work/rel" LOCRIN_INSTALL_DIR="$work/inst" bash install.sh "$tag"
  got=$("$work/inst/locrin" --version)
  if [[ "$target" == *apple* ]]; then
    # The formula quotes all three non-Windows assets, and this runner built one
    # of them. Pad only the checksums that are missing so the generator's lookup
    # finds exactly one line per asset; the release job generates the real
    # formula from the real checksums.
    zeros=$(printf '0%.0s' {1..64})
    for t in x86_64-unknown-linux-gnu aarch64-apple-darwin x86_64-apple-darwin; do
      grep -q " locrin-$tag-$t.tar.gz\$" "$work/rel/$tag/SHA256SUMS" \
        || printf '%s  locrin-%s-%s.tar.gz\n' "$zeros" "$tag" "$t" >> "$work/rel/$tag/SHA256SUMS"
    done
    bash scripts/homebrew-formula.sh "$version" "$work/rel/$tag/SHA256SUMS" > "$work/locrin.rb"
    ruby -c "$work/locrin.rb"
  fi
fi
[[ "$got" == "locrin $version" ]] || { echo "installer binary printed: $got"; exit 1; }
echo "install smoke passed for $target"
