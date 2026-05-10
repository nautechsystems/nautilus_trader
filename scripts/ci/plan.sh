#!/usr/bin/env bash
set -euo pipefail

# Determine which CI jobs should run based on changed files.
#
# Outputs (to $GITHUB_OUTPUT):
#   run_tests       - true if any non-docs code changed
#   run_rust_tests  - true if Rust/Cython code changed
#
# Required env vars:
#   EVENT_NAME   - github.event_name (push or pull_request)
#   BASE_REF     - github.event.pull_request.base.ref (PRs only, e.g. "develop")
#   BEFORE_SHA   - github.event.before (push only, previous HEAD)

run_all() {
  echo "run_tests=true" >> "$GITHUB_OUTPUT"
  echo "run_rust_tests=true" >> "$GITHUB_OUTPUT"
  echo "$1"
  exit 0
}

# Determine changed files
if [[ "$EVENT_NAME" == "push" ]]; then
  # All-zero BEFORE_SHA means new branch; run everything
  if [[ "$BEFORE_SHA" == "0000000000000000000000000000000000000000" ]]; then
    run_all "New branch push: running all jobs"
  fi
  changed_files="$(git diff --name-only "$BEFORE_SHA" HEAD)"
else
  # The PR event payload freezes base.sha at PR creation time, so intervening
  # commits on the base branch would otherwise appear as PR changes. Re-resolve
  # the merge-base against the current base branch head so the diff reflects
  # only what this PR actually changes relative to where it will land.
  if [[ -z "${BASE_REF:-}" ]]; then
    run_all "BASE_REF not set: running all jobs"
  fi
  if ! merge_base="$(git merge-base "origin/${BASE_REF}" HEAD 2> /dev/null)"; then
    run_all "Failed to compute merge-base against origin/${BASE_REF}: running all jobs"
  fi
  if [[ -z "$merge_base" ]]; then
    run_all "Empty merge-base against origin/${BASE_REF}: running all jobs"
  fi
  changed_files="$(git diff --name-only "$merge_base" HEAD)"
fi

code_changed=0
rust_changed=0
while IFS= read -r file; do
  [[ -z "$file" ]] && continue
  # Skip docs subtree
  [[ "$file" =~ ^docs/ ]] && continue
  code_changed=1
  # Rust, Cython, cargo config, or build infrastructure means full Rust tests
  [[ "$file" =~ \.(rs|pyx|pxd)$ ]] && rust_changed=1
  [[ "$file" =~ Cargo\.(toml|lock)$ ]] && rust_changed=1
  [[ "$file" == "rust-toolchain.toml" ]] && rust_changed=1
  [[ "$file" =~ ^\.cargo/ ]] && rust_changed=1
  [[ "$file" =~ ^crates/ ]] && rust_changed=1
  [[ "$file" =~ ^schema/ ]] && rust_changed=1
  [[ "$file" == "Makefile" ]] && rust_changed=1
  [[ "$file" =~ ^\.github/ ]] && rust_changed=1
done <<< "$changed_files"

if [[ $code_changed -eq 0 ]]; then
  echo "run_tests=false" >> "$GITHUB_OUTPUT"
  echo "run_rust_tests=false" >> "$GITHUB_OUTPUT"
  echo "Docs-only changes: skipping build and test jobs"
elif [[ $rust_changed -eq 1 ]]; then
  echo "run_tests=true" >> "$GITHUB_OUTPUT"
  echo "run_rust_tests=true" >> "$GITHUB_OUTPUT"
  echo "Rust/Cython changes detected: running all jobs"
else
  echo "run_tests=true" >> "$GITHUB_OUTPUT"
  echo "run_rust_tests=false" >> "$GITHUB_OUTPUT"
  echo "Python-only changes: skipping Rust tests"
fi
