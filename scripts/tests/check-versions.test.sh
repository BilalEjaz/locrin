#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT

# The real tree must pass.
bash scripts/check-versions.sh

# A tree with a drifted path dependency must fail.
mkdir -p "$tmp/crates/a" "$tmp/crates/b"
cat > "$tmp/Cargo.toml" <<'EOF'
[workspace]
members = ["crates/a", "crates/b"]
[workspace.package]
version = "1.2.3"
EOF
cat > "$tmp/crates/a/Cargo.toml" <<'EOF'
[package]
name = "a"
version.workspace = true
EOF
cat > "$tmp/crates/b/Cargo.toml" <<'EOF'
[package]
name = "b"
version.workspace = true
[dependencies]
a = { path = "../a", version = "1.2.2" }
EOF
if bash scripts/check-versions.sh "$tmp" 2>/dev/null; then echo "drift must fail"; exit 1; fi
