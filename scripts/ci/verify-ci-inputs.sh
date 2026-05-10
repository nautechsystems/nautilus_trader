#!/usr/bin/env bash
set -euo pipefail

# Temporary CI guardrail: compare immutable workflow and binding inputs with the checked-out commit.
stage="${1:-unknown stage}"

targets=(
  .github/actions
  .github/workflows
  python/generate_docstrings.py
  python/generate_stubs.py
  crates/pyo3
  ':(glob)crates/**/src/python/**/*.rs'
)

git update-index -q --refresh

changes="$(git status --short --untracked-files=all -- "${targets[@]}")"

if [ -n "$changes" ]; then
  echo "::error::Detected unexpected modifications in immutable CI inputs during ${stage}"
  echo "Changed files:"
  printf '%s\n' "$changes"
  echo
  echo "Diff stat:"
  git diff --stat HEAD -- "${targets[@]}" || true
  exit 1
fi

echo "Verified immutable CI inputs during ${stage}"
