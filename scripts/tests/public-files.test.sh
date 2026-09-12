#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
for f in LICENSE SECURITY.md CONTRIBUTING.md .github/ISSUE_TEMPLATE/false-positive.yml .github/ISSUE_TEMPLATE/bug.yml .github/ISSUE_TEMPLATE/rule-request.yml .github/ISSUE_TEMPLATE/config.yml; do
  [[ -f "$f" ]] || { echo "missing $f"; exit 1; }
done
grep -q '^MIT License' LICENSE || { echo "LICENSE is not MIT"; exit 1; }
grep -q 'Raxbi Ltd' LICENSE || { echo "LICENSE holder"; exit 1; }
grep -q 'Report a vulnerability' SECURITY.md || { echo "SECURITY.md must point at private reporting"; exit 1; }
grep -q 'cargo test --workspace' CONTRIBUTING.md || { echo "CONTRIBUTING.md must give the test command"; exit 1; }
[[ ! -d placeholders ]] || { echo "placeholders must be gone"; exit 1; }
