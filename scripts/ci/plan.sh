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
#
# Out-of-scope paths (treated as no-ops):
#   - docs/                                 documentation only
#   - Makefile                              tooling: CI invokes named targets
#                                           explicitly, so a broken target
#                                           fails its caller rather than
#                                           silently skewing the build
#   - .github/workflows/<not build*>.yml    scheduled or independent workflows
#                                           that build.yml never triggers
#                                           (nightly-*, dst, performance, ...)
#   - root all-caps *.md                    README, RELEASES, CONTRIBUTING,
#                                           SECURITY, etc.: documentation only
#   - crates/.../README.md                  per-crate README (any depth):
#                                           shipped by cargo publish but content
#                                           does not affect compilation
#   - LICENSE                               legal text only
#   - .gitignore, .gitattributes,           VCS and editor metadata: no effect
#     .editorconfig                         on build or test outcomes
#
# The four rules above only skip when the file is still present on disk so a
# deletion (e.g. removing README.md, which pyproject.toml and Cargo.toml
# declare as package metadata) cannot bypass the build that would catch it.

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
  if [[ -z "${BEFORE_SHA:-}" ]] || ! git cat-file -e "${BEFORE_SHA}^{commit}" 2> /dev/null; then
    run_all "Push base SHA not found: running all jobs"
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
  # Out-of-scope paths (see header for rationale)
  [[ "$file" =~ ^docs/ ]] && continue
  [[ "$file" == "Makefile" ]] && continue
  [[ "$file" =~ ^\.github/workflows/ && ! "$file" =~ ^\.github/workflows/build ]] && continue
  [[ "$file" =~ ^[A-Z][A-Z0-9_]*\.md$ && -f "$file" ]] && continue
  [[ "$file" =~ ^crates/.+/README\.md$ && -f "$file" ]] && continue
  [[ "$file" == "LICENSE" && -f "$file" ]] && continue
  if [[ "$file" == ".gitignore" || "$file" == ".gitattributes" || "$file" == ".editorconfig" ]] && [[ -f "$file" ]]; then
    continue
  fi
  code_changed=1
  # Rust, Cython, cargo config, or build infrastructure means full Rust tests
  [[ "$file" =~ \.(rs|pyx|pxd)$ ]] && rust_changed=1
  [[ "$file" =~ Cargo\.(toml|lock)$ ]] && rust_changed=1
  [[ "$file" == "rust-toolchain.toml" ]] && rust_changed=1
  [[ "$file" =~ ^\.cargo/ ]] && rust_changed=1
  [[ "$file" =~ ^crates/ ]] && rust_changed=1
  [[ "$file" =~ ^schema/ ]] && rust_changed=1
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
