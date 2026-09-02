#!/usr/bin/env bash

set -euo pipefail

if ! command -v rg &> /dev/null; then
  echo "ERROR: ripgrep is required for documentation convention hook tests" >&2
  echo "       install from: https://github.com/BurntSushi/ripgrep#installation" >&2
  exit 1
fi

REPO_ROOT=$(git rev-parse --show-toplevel)
HOOK="$REPO_ROOT/.pre-commit-hooks/check_docs_conventions.sh"

CASE_ROOT=$(mktemp -d)
trap 'rm -rf "$CASE_ROOT"' EXIT

run_hook() {
  local case_dir="$1"

  set +e
  (cd "$case_dir" && bash "$HOOK") > "$case_dir/output.txt" 2>&1
  RUN_STATUS=$?
  set -e
}

valid_case="$CASE_ROOT/valid"
mkdir -p "$valid_case/crates/demo/src"
printf '%s\n' \
  '[package]' \
  'name = "demo"' \
  '' \
  '  [features] # crate features' \
  'default = ["zeta"]' \
  'zeta = []' \
  '"simd+avx" = []' \
  'alpha = []' \
  'complex = [' \
  '  "alpha", # default-features = false' \
  ']' \
  '  [dependencies] # dependencies' \
  'fake = "1"' > "$valid_case/crates/demo/Cargo.toml"
printf '%s\n' \
  '# Demo' \
  '' \
  '## Feature flags' \
  '' \
  "- \`alpha\`" \
  "- \`complex\`" \
  "- \`simd+avx\`" \
  "- \`zeta\`" > "$valid_case/crates/demo/README.md"
printf '%s\n' \
  '//! Demo.' \
  '//!' \
  '//! # Feature Flags' \
  '//!' \
  "//! - \`alpha\`" \
  "//! - \`complex\`" \
  "//! - \`simd+avx\`" \
  "//! - \`zeta\`" > "$valid_case/crates/demo/src/lib.rs"
run_hook "$valid_case"
if [ "$RUN_STATUS" -ne 0 ]; then
  echo "Expected documentation convention hook to pass valid feature lists"
  cat "$valid_case/output.txt"
  exit 1
fi
rg -Fq "All documentation conventions are valid" "$valid_case/output.txt"

invalid_case="$CASE_ROOT/invalid"
mkdir -p \
  "$invalid_case/crates/mismatch/src" \
  "$invalid_case/crates/missing/src" \
  "$invalid_case/crates/no_section/src"
printf '%s\n' \
  '[package]' \
  'name = "mismatch"' \
  '' \
  '[features]' \
  'alpha = []' \
  'beta = []' > "$invalid_case/crates/mismatch/Cargo.toml"
printf '%s\n' \
  '# Mismatch' \
  '' \
  '## Feature flags' \
  '' \
  "- \`alpha\`" > "$invalid_case/crates/mismatch/README.md"
printf '%s\n' \
  '//! Mismatch.' \
  '//!' \
  '//! # Feature Flags' \
  '//!' \
  "//! - \`beta\`" \
  "//! - \`alpha\`" > "$invalid_case/crates/mismatch/src/lib.rs"
printf '%s\n' \
  '[package]' \
  'name = "missing"' \
  '' \
  '[features]' \
  'alpha = []' > "$invalid_case/crates/missing/Cargo.toml"
printf '%s\n' \
  '//! Missing README.' \
  '//!' \
  '//! # Feature Flags' \
  '//!' \
  "//! - \`alpha\`" > "$invalid_case/crates/missing/src/lib.rs"
printf '%s\n' \
  '[package]' \
  'name = "no-section"' \
  '' \
  '[features]' \
  'alpha = []' > "$invalid_case/crates/no_section/Cargo.toml"
printf '%s\n' \
  '# No section' > "$invalid_case/crates/no_section/README.md"
printf '%s\n' \
  '//! No section.' \
  '//!' \
  '//! # Feature Flags' \
  '//!' \
  "//! - \`alpha\`" > "$invalid_case/crates/no_section/src/lib.rs"
run_hook "$invalid_case"
if [ "$RUN_STATUS" -ne 1 ]; then
  echo "Expected documentation convention hook to reject invalid feature lists"
  cat "$invalid_case/output.txt"
  exit 1
fi

for result in \
  "Feature flag list mismatch in crates/mismatch/README.md" \
  "Feature flag list mismatch in crates/mismatch/src/lib.rs" \
  "Missing feature flag documentation file crates/missing/README.md" \
  "Missing \`## Feature flags\` section in crates/no_section/README.md" \
  "Found 4 documentation convention violation(s)"; do
  if ! rg -Fq "$result" "$invalid_case/output.txt"; then
    echo "Expected documentation convention result not found: $result"
    cat "$invalid_case/output.txt"
    exit 1
  fi
done

echo "Documentation convention hook tests passed"
