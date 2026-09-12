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
# Issue forms: an unquoted scalar in a flow mapping splits at the first comma and
# invents a key GitHub's schema rejects. The templates follow one convention:
# every string scalar inside a flow mapping is double quoted. Enforced here
# without a YAML parser by deleting the double quoted strings and then requiring
# every key in what is left to carry no value at all, or a bare boolean or
# number. Anything else is an unquoted string scalar.
for f in .github/ISSUE_TEMPLATE/*.yml; do
  bad=$(sed -E 's/"[^"]*"//g' "$f" | grep -n '{' |
    grep -vE '^[0-9]+:[^{]*\{( *[A-Za-z_-]+: *(true|false|[0-9]+)? *,?)* *\} *$' || true)
  if [[ -n "$bad" ]]; then
    echo "$f: issue template flow mappings must double quote every string scalar"
    echo "$bad"
    exit 1
  fi
done
