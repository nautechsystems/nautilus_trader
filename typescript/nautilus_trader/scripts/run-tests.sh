#!/bin/bash
# Run each test file in a separate Bun process to avoid FFI dlopen crashes
# when multiple test files share the same process.

set -e
cd "$(dirname "$0")/.."

PASS=0
FAIL=0
FAILED_FILES=()

for f in $(find tests -name '*.test.ts' | sort); do
  printf "%-50s " "$f"
  if output=$(bun test "$f" 2>&1); then
    # Extract pass count from output line like " 8 pass"
    count=$(echo "$output" | awk '/[0-9]+ pass/{for(i=1;i<=NF;i++) if($i=="pass") print $(i-1)}' | head -1)
    echo "✓ (${count:-0} pass)"
    PASS=$((PASS + ${count:-0}))
  else
    echo "✗ FAILED"
    FAIL=$((FAIL + 1))
    FAILED_FILES+=("$f")
    echo "$output" | tail -5
    echo ""
  fi
done

echo ""
echo "================================"
echo "Total passed: $PASS"
echo "Files failed: $FAIL"
if [ ${#FAILED_FILES[@]} -gt 0 ]; then
  echo "Failed files:"
  for f in "${FAILED_FILES[@]}"; do
    echo "  - $f"
  done
  exit 1
fi
echo "All tests passed!"
