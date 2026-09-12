#!/usr/bin/env bash
# Prints the Homebrew formula for one release.
# Usage: scripts/homebrew-formula.sh <version-without-v> <SHA256SUMS>
set -euo pipefail
version="$1"; sums="$2"
base="https://github.com/BilalEjaz/locrin/releases/download/v$version"
sha() {
  local s
  s=$(grep " locrin-v$version-$1.tar.gz\$" "$sums" | awk '{print $1}')
  [[ ${#s} -eq 64 ]] || { echo "homebrew-formula: no checksum for $1 in $sums" >&2; exit 2; }
  echo "$s"
}
arm=$(sha aarch64-apple-darwin); intel=$(sha x86_64-apple-darwin); linux=$(sha x86_64-unknown-linux-gnu)
cat <<EOF
class Locrin < Formula
  desc "Deterministic quality gate for code written by people and agents"
  homepage "https://locrin.com"
  version "$version"
  license "MIT"

  on_macos do
    on_arm do
      url "$base/locrin-v$version-aarch64-apple-darwin.tar.gz"
      sha256 "$arm"
    end
    on_intel do
      url "$base/locrin-v$version-x86_64-apple-darwin.tar.gz"
      sha256 "$intel"
    end
  end

  on_linux do
    on_intel do
      url "$base/locrin-v$version-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "$linux"
    end
  end

  def install
    # The tarball holds one directory with the binary inside; find it either way.
    bin.install Dir["**/locrin"].first
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/locrin --version")
  end
end
EOF
