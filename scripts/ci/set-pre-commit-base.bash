#!/usr/bin/env bash
set -euo pipefail

base=$(git rev-parse --verify 'origin/develop^{commit}')
if ! git merge-base --is-ancestor "$base" HEAD; then
  echo "::error::test-pre-commit must contain origin/develop to test the pending develop push" >&2
  exit 1
fi

printf 'CHANGED_BASE_SHA=%s\n' "$base" >> "${GITHUB_ENV:?}"
echo "Testing pre-commit changes against develop at $base"
