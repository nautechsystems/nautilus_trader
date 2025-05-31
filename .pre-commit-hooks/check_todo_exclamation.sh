#!/usr/bin/env bash

# Check for TODO! patterns that shouldn't be committed
#
# This hook fails if any file contains "TODO!" which is used to mark
# temporary changes that should not be committed to the repository.
set -e

# Search for TODO! in source files, excluding documentation, virtual envs, and build artifacts
matches=$(grep -R --binary-files=without-match -n "TODO!" \
  --exclude-dir=.git \
  --exclude-dir=target \
  --exclude-dir=build \
  --exclude-dir=.pytest_cache \
  --exclude-dir=__pycache__ \
  --exclude-dir=.venv \
  --exclude-dir=venv \
  --exclude-dir=node_modules \
  --exclude="*.md" \
  --exclude=".pre-commit-config.yaml" \
  --exclude-dir=.pre-commit-hooks \
  . || true)

if [[ -n "$matches" ]]; then
  # Count the number of matches to use proper grammar
  count=$(echo "$matches" | wc -l)
  if [[ $count -eq 1 ]]; then
    echo "TODO! marker detected (should not be committed):"
    echo "$matches"
    echo ""
    echo "Please resolve this TODO! marker before committing."
  else
    echo "TODO! markers detected (should not be committed):"
    echo "$matches"
    echo ""
    echo "Please resolve these TODO! markers before committing."
  fi
  exit 1
fi
