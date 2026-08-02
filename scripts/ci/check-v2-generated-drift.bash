#!/usr/bin/env bash
set -euo pipefail

targets=(
  python/generate_docstrings.py
  python/generate_stubs.py
  crates/pyo3
  ':(glob)python/nautilus_trader/**/*.pyi'
  ':(glob)crates/**/src/python/**/*.rs'
)

git update-index -q --refresh

untracked="$(git ls-files --others --exclude-standard -- "${targets[@]}")"

if git diff --quiet -- "${targets[@]}" && [ -z "$untracked" ]; then
  echo "No generated v2 file drift detected"
  exit 0
fi

echo "::error::Generated v2 files are out of sync"
echo "Run \`make py-stubs-v2\` and stage or commit the result."
echo
echo "Changed files:"
git status --short --untracked-files=all -- "${targets[@]}"
echo
echo "Diff stat:"
git diff --stat -- "${targets[@]}" || true

if [ -n "$untracked" ]; then
  echo
  echo "Untracked generated files:"
  printf '%s\n' "$untracked"
fi

echo
git diff --exit-code -- "${targets[@]}" || true
exit 1
