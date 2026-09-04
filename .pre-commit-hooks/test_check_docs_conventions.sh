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

# Doc-section rules resolve windows of lines around each heading, so cover the
# exemption side of every branch here and the violation side in the invalid case:
# suppression markers, return type, panic tokens in the body, and unit functions.
printf '%s\n' \
  '' \
  '/// Parses the input.' \
  '///' \
  '/// # Errors' \
  '///' \
  '/// Returns an error when the input is not a number.' \
  'pub fn parse(input: &str) -> Result<u32, String> {' \
  '    input.parse().map_err(|_| "not a number".to_string())' \
  '}' \
  '' \
  '/// Looks up a value.' \
  '///' \
  '/// # Errors' \
  '///' \
  "/// Returns \`None\` when the key is missing." \
  'pub fn lookup(key: u32) -> Option<u32> {' \
  '    Some(key)' \
  '}' \
  '' \
  '/// Unwraps a value.' \
  '///' \
  '/// # Panics' \
  '///' \
  '/// Panics when the value is missing.' \
  'pub fn unwrap_value(value: Option<u32>) -> Result<u32, String> {' \
  '    Ok(value.unwrap())' \
  '}' \
  '' \
  '/// Logs a value.' \
  '///' \
  '/// # Panics' \
  '///' \
  '/// Unit functions are not checked for panic tokens.' \
  'pub fn log_value(value: u32) {' \
  '    let _ = value;' \
  '}' \
  '' \
  '// panics-doc-ok' \
  '/// Delegates to a callee that panics.' \
  '///' \
  '/// # Panics' \
  '///' \
  '/// Panics inside the callee.' \
  'pub fn delegate() -> Result<(), String> {' \
  '    Ok(())' \
  '}' \
  '' \
  '// errors-doc-ok' \
  '/// Documents a trait contract.' \
  '///' \
  '/// # Errors' \
  '///' \
  '/// Never fails in this implementation.' \
  'pub fn contract() {}' >> "$valid_case/crates/demo/src/lib.rs"
mkdir -p "$valid_case/docs"
printf '%s\n' \
  '| Setting | Value |' \
  '| ------- | ----- |' \
  '| configuration | on-demand |' > "$valid_case/docs/guide.md"
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
mkdir -p "$invalid_case/crates/doc_sections/src" "$invalid_case/docs"
printf '%s\n' \
  '/// Never panics.' \
  '///' \
  '/// # Panics' \
  '///' \
  '/// This function does not panic.' \
  'pub fn calm() -> Result<(), String> {' \
  '    Ok(())' \
  '}' \
  '' \
  '/// Claims a panic.' \
  '///' \
  '/// # Panics' \
  '///' \
  '/// Panics on bad input.' \
  'pub fn quiet(input: u32) -> Result<u32, String> {' \
  '    Ok(input)' \
  '}' \
  '' \
  '/// Claims an error.' \
  '///' \
  '/// # Errors' \
  '///' \
  '/// Returns an error on bad input.' \
  'pub fn infallible(input: u32) -> u32 {' \
  '    input' \
  '}' > "$invalid_case/crates/doc_sections/src/lib.rs"
soft_hyphen=$(printf '\302\255')
printf '%s\n' \
  '| configu- ration | x |' \
  '| word | frag-' \
  "| soft${soft_hyphen}hyphen | y |" > "$invalid_case/docs/tables.md"
printf '%s\n' \
  'See [the guide](https://nautilustrader.io/docs/nightly/integrations/lighter.html#anchor).' > "$invalid_case/docs/links.md"
# Release notes record links as published, so the same shape must not be reported here.
printf '%s\n' \
  '- Added CLI (see [docs](https://nautilustrader.io/docs/nightly/developer_guide/index.html))' > "$invalid_case/RELEASES.md"
# A commit message naming the dead form must not block the next commit.
mkdir -p "$invalid_case/.git"
printf '%s\n' \
  'Add rule rejecting https://nautilustrader.io/docs/nightly/foo.html URLs' > "$invalid_case/.git/COMMIT_EDITMSG"
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
  "Self-contradictory \`# Panics\` doc on \`calm\` in crates/doc_sections/src/lib.rs:3" \
  "False \`# Panics\` doc on \`quiet\` in crates/doc_sections/src/lib.rs:12" \
  "False \`# Errors\` doc on \`infallible\` in crates/doc_sections/src/lib.rs:21" \
  "Possible word split in docs/tables.md:1:| configu- ration | x |" \
  "Trailing hyphen at end of table line in docs/tables.md:2:| word | frag-" \
  "Soft hyphen (U+00AD) in docs/tables.md:3:| soft${soft_hyphen}hyphen | y |" \
  "Dead \`.html\` docs URL in ./docs/links.md:1" \
  "Found 11 documentation convention violation(s)"; do
  if ! rg -Fq "$result" "$invalid_case/output.txt"; then
    echo "Expected documentation convention result not found: $result"
    cat "$invalid_case/output.txt"
    exit 1
  fi
done

echo "Documentation convention hook tests passed"
