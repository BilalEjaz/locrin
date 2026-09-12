#!/usr/bin/env bash
# Installs the locrin binary for this machine from a GitHub release.
#   curl -fsSL https://raw.githubusercontent.com/BilalEjaz/locrin/main/install.sh | bash
#   ... | bash -s v0.5.0            pin a version (default: latest)
# LOCRIN_INSTALL_DIR  where the binary goes (default: ~/.local/bin)
# LOCRIN_BASE_URL     asset base (default: the GitHub release download URL)
set -euo pipefail
REPO="BilalEjaz/locrin"
BASE_URL="${LOCRIN_BASE_URL:-https://github.com/$REPO/releases/download}"
VERSION="${1:-latest}"
INSTALL_DIR="${LOCRIN_INSTALL_DIR:-$HOME/.local/bin}"

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64) target=x86_64-unknown-linux-gnu ;;
  Darwin-arm64) target=aarch64-apple-darwin ;;
  Darwin-x86_64) target=x86_64-apple-darwin ;;
  *) echo "locrin: no prebuilt binary for $(uname -s) $(uname -m); see https://github.com/$REPO/releases" >&2; exit 2 ;;
esac

if [[ "$VERSION" == "latest" ]]; then
  VERSION=$(curl -fsSI -o /dev/null -w '%{redirect_url}' "https://github.com/$REPO/releases/latest" || true)
  VERSION="${VERSION##*/}"
  [[ "$VERSION" == v* ]] || { echo "locrin: could not resolve the latest release" >&2; exit 2; }
fi

asset="locrin-$VERSION-$target.tar.gz"
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
curl -fsSL "$BASE_URL/$VERSION/$asset" -o "$tmp/$asset"
curl -fsSL "$BASE_URL/$VERSION/SHA256SUMS" -o "$tmp/SHA256SUMS"

expected=$(awk -v a="$asset" '$2==a || $2=="*"a {print $1}' "$tmp/SHA256SUMS")
[[ -n "$expected" ]] || { echo "locrin: $asset is not listed in SHA256SUMS" >&2; exit 2; }
if command -v sha256sum >/dev/null; then actual=$(sha256sum "$tmp/$asset" | awk '{print $1}'); else actual=$(shasum -a 256 "$tmp/$asset" | awk '{print $1}'); fi
[[ "$actual" == "$expected" ]] || { echo "locrin: checksum mismatch for $asset" >&2; exit 2; }

tar -xzf "$tmp/$asset" -C "$tmp"
mkdir -p "$INSTALL_DIR"
install -m 755 "$tmp/locrin-$VERSION-$target/locrin" "$INSTALL_DIR/locrin.tmp"
mv -f "$INSTALL_DIR/locrin.tmp" "$INSTALL_DIR/locrin"
echo "locrin $VERSION installed to $INSTALL_DIR/locrin"
case ":$PATH:" in *":$INSTALL_DIR:"*) ;; *) echo "add $INSTALL_DIR to your PATH" ;; esac
