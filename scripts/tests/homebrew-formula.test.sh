#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
out=$(bash scripts/homebrew-formula.sh 9.9.9 scripts/tests/fixtures/SHA256SUMS)
need() { grep -qF -- "$1" <<<"$out" || { echo "missing: $1"; echo "$out"; exit 1; }; }
need 'class Locrin < Formula'
need 'version "9.9.9"'
need 'license "MIT"'
need 'url "https://github.com/BilalEjaz/locrin/releases/download/v9.9.9/locrin-v9.9.9-aarch64-apple-darwin.tar.gz"'
need 'sha256 "1111111111111111111111111111111111111111111111111111111111111111"'
need 'url "https://github.com/BilalEjaz/locrin/releases/download/v9.9.9/locrin-v9.9.9-x86_64-apple-darwin.tar.gz"'
need 'sha256 "2222222222222222222222222222222222222222222222222222222222222222"'
need 'url "https://github.com/BilalEjaz/locrin/releases/download/v9.9.9/locrin-v9.9.9-x86_64-unknown-linux-gnu.tar.gz"'
need 'sha256 "4444444444444444444444444444444444444444444444444444444444444444"'
need 'bin.install'
need 'locrin --version'
if grep -q 3333 <<<"$out"; then echo "windows asset must not appear"; exit 1; fi
if command -v ruby >/dev/null; then ruby -c <(echo "$out") >/dev/null; fi
if bash scripts/homebrew-formula.sh 9.9.9 /dev/null 2>/dev/null; then echo "missing checksum must fail"; exit 1; fi
