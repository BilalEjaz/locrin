#!/usr/bin/env bash
# Runs every scripts/tests/*.test.sh and fails if any fails.
set -euo pipefail
cd "$(dirname "$0")/../.."
status=0
for t in scripts/tests/*.test.sh; do
  if bash "$t"; then echo "ok   $t"; else echo "FAIL $t"; status=1; fi
done
exit "$status"
