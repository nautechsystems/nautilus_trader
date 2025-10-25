#!/usr/bin/env bash
set -euo pipefail

# Validate wheel count matches expected count
# Usage: validate-wheel-count.sh <expected_count>

if [ $# -ne 1 ]; then
  echo "Usage: $0 <expected_count>" >&2
  exit 1
fi

expected_count=$1

if ! [[ "$expected_count" =~ ^[0-9]+$ ]]; then
  echo "ERROR: expected_count must be a positive integer, got: $expected_count" >&2
  exit 1
fi

echo "Validating wheel count in dist/ directory..."

wheel_count=$(find dist/ -name "*.whl" -type f | wc -l)

if [ "$wheel_count" -ne "$expected_count" ]; then
  echo "ERROR: Expected $expected_count wheels, found $wheel_count" >&2
  echo "Downloaded wheels:" >&2
  find dist/ -name "*.whl" -type f -ls >&2
  exit 1
fi

echo "✓ Validated: Found all $expected_count wheels"
