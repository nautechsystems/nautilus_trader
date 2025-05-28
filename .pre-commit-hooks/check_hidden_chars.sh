#!/usr/bin/env bash

# Ensure no hidden control or zero-width unicode characters in source
#
# This hook fails if any file contains control chars (U+0001–U+0008, U+000E–U+001F)
# or zero-width spaces (U+200B, U+200C, U+200D) or BOM (U+FEFF).
set -e
# Files to check: all tracked source files
matches=$(grep -R --binary-files=without-match -nP "[\x01-\x08\x0E-\x1F\u200B\u200C\u200D\uFEFF]" --exclude-dir=.git --exclude-dir=target --exclude-dir=build . || true)
if [[ -n "$matches" ]]; then
  echo "Hidden/invisible control or zero-width Unicode characters detected:"
  echo "$matches"
  exit 1
fi
