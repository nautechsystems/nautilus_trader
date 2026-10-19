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
mkdir -p "$valid_case/crates/custom" "$valid_case/crates/demo/src"
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
  '/// Returns an error if the input is not a number.' \
  'pub fn parse(input: &str) -> Result<u32, String> {' \
  '    input.parse().map_err(|_| "not a number".to_string())' \
  '}' \
  '' \
  '/// Looks up a value.' \
  '///' \
  '/// # Errors' \
  '///' \
  "/// Returns \`None\` if the key is missing." \
  'pub fn lookup(key: u32) -> Option<u32> {' \
  '    Some(key)' \
  '}' \
  '' \
  '/// Unwraps a value.' \
  '///' \
  '/// # Panics' \
  '///' \
  '/// Panics if the value is missing.' \
  'pub fn unwrap_value(value: Option<u32>) -> Result<u32, String> {' \
  '    Ok(value.unwrap())' \
  '}' \
  '' \
  '/// Logs a value.' \
  '///' \
  '/// # Panics' \
  '///' \
  '/// Panics if the logger rejects the value.' \
  'pub fn log_value(value: u32) {' \
  '    let _ = value;' \
  '}' \
  '' \
  '// panics-doc-ok' \
  '/// Delegates to a callee that panics.' \
  '///' \
  '/// # Panics' \
  '///' \
  '/// Panics if the callee rejects the value.' \
  'pub fn delegate() -> Result<(), String> {' \
  '    Ok(())' \
  '}' \
  '' \
  '// errors-doc-ok' \
  '/// Documents a trait contract.' \
  '///' \
  '/// # Errors' \
  '///' \
  '/// Returns an error if the trait contract is violated.' \
  'pub fn contract() {}' >> "$valid_case/crates/demo/src/lib.rs"
printf '%s\n' \
  '[package]' \
  'name = "custom"' \
  '' \
  '[lib]' \
  'path = "lib.rs"' \
  '' \
  '[features]' \
  'test-support = []' > "$valid_case/crates/custom/Cargo.toml"
printf '%s\n' \
  '# Custom' \
  '' \
  '## Feature flags' \
  '' \
  "- \`test-support\`" > "$valid_case/crates/custom/README.md"
printf '%s\n' \
  '//! Custom.' \
  '//!' \
  '//! # Feature Flags' \
  '//!' \
  "//! - \`test-support\`" > "$valid_case/crates/custom/lib.rs"
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
  '/// Panics if this function does not panic.' \
  'pub fn calm() -> Result<(), String> {' \
  '    Ok(())' \
  '}' \
  '' \
  '/// Claims a panic.' \
  '///' \
  '/// # Panics' \
  '///' \
  '/// Panics if the input is invalid.' \
  'pub fn quiet(input: u32) -> Result<u32, String> {' \
  '    Ok(input)' \
  '}' \
  '' \
  '/// Claims an error.' \
  '///' \
  '/// # Errors' \
  '///' \
  '/// Returns an error if the input is invalid.' \
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

run_rustdoc_case() {
  local name="$1"
  local expected="$2"
  local count="$3"
  local case_dir="$CASE_ROOT/$name"

  mkdir -p "$case_dir/crates/demo/src" "$case_dir/docs"
  cat > "$case_dir/crates/demo/src/lib.rs"
  run_hook "$case_dir"
  if [ "$count" -eq 0 ]; then
    if [ "$RUN_STATUS" -eq 0 ]; then
      return
    fi
  elif [ "$RUN_STATUS" -eq 1 ] &&
    rg -Fq "$expected" "$case_dir/output.txt" &&
    rg -Fq "Found $count documentation convention violation(s)" "$case_dir/output.txt"; then
    return
  fi

  echo "Unexpected documentation convention result for $name"
  cat "$case_dir/output.txt"
  exit 1
}

run_rustdoc_case valid_sections '' 0 << 'RUST'
/// # Errors
///
/// Returns an error if:
/// - The market price cannot be
///   found.
/// - The conversion rate calculation fails.
///
/// # Panics
///
/// Panics if:
/// - The quantity is zero.
/// - The quantity exceeds the supported
///   range.
///
/// # Notes
///
/// - A note without a period
pub fn convert() -> Result<(), String> { panic!("invalid quantity") }

/// # Errors
///
/// Returns `None` if:
/// - The key is absent.
/// - The value has expired.
pub fn lookup() -> Option<u32> { None }

/// # Errors
///
/// Returns an error
/// if the input is invalid.
///
/// ```rust
/// - This is example code without a period
/// # Panics
/// Nonstandard wording inside an example
/// ```
///
/// ~~~~text
/// - Another example without a period
/// ~~~~
pub fn parse() -> Result<(), String> { Ok(()) }
RUST

run_rustdoc_case return_types '' 0 << 'RUST'
/// # Errors
///
/// Returns an error if the input is invalid.
pub fn parse(input: Option<u32>) -> std::result::Result<Option<u32>, String> { Ok(input) }

/// # Errors
///
/// Returns `None` if no result is available.
pub fn lookup() -> std::option::Option<Result<u32, String>> { None }

/// # Errors
///
/// Returns an error if conversion fails.
pub fn convert<T: Fn() -> Option<u32>>(
    source: T,
) -> PyResult<Option<u32>> { Ok(source()) }

/// # Errors
///
/// Returns `None` if the key is missing.
pub fn lookup_with(source: impl Fn() -> Result<u32, String>) -> Option<u32> { None }
RUST

run_rustdoc_case option_error_lead "Incorrect \`# Errors\` lead-in for Option in crates/demo/src/lib.rs:3" 1 << 'RUST'
/// # Errors
///
/// Returns an error if the key is missing.
pub fn lookup() -> Option<Result<u32, String>> { None }
RUST

run_rustdoc_case result_none_lead "Incorrect \`# Errors\` lead-in for Result in crates/demo/src/lib.rs:3" 1 << 'RUST'
/// # Errors
///
/// Returns `None` if parsing fails.
pub fn parse() -> std::result::Result<Option<u32>, String> { Ok(None) }
RUST

run_rustdoc_case callback_result_none_lead "Incorrect \`# Errors\` lead-in for Result in crates/demo/src/lib.rs:3" 1 << 'RUST'
/// # Errors
///
/// Returns `None` if conversion fails.
pub fn convert<T: Fn() -> Option<u32>>(
    source: T,
) -> PyResult<Option<u32>> { Ok(source()) }
RUST

run_rustdoc_case result_alias_none_lead "Incorrect \`# Errors\` lead-in for Result in crates/demo/src/lib.rs:3" 1 << 'RUST'
/// # Errors
///
/// Returns `None` if the request fails.
pub fn request() -> BinanceSpotHttpResult<Vec<u32>> { Ok(Vec::new()) }
RUST

run_rustdoc_case bare_result_alias '' 0 << 'RUST'
/// # Errors
///
/// Returns an error if opening the backend fails.
fn open_backend() -> BackendOpenResult { Ok(()) }

type BackendOpenResult = Result<(), String>;
RUST

run_rustdoc_case bare_result_alias_none_lead "Incorrect \`# Errors\` lead-in for Result in crates/demo/src/lib.rs:3" 1 << 'RUST'
/// # Errors
///
/// Returns `None` if opening the backend fails.
fn open_backend() -> BackendOpenResult { Ok(()) }

type BackendOpenResult = Result<(), String>;
RUST

run_rustdoc_case long_errors_section "Incorrect \`# Errors\` lead-in for Option in crates/demo/src/lib.rs:3" 1 << 'RUST'
/// # Errors
///
/// Returns an error if:
/// - The key is missing.
///
/// # Examples
///
/// ```rust
/// // Examples may span enough lines to put the signature outside
/// // the original 20-line window after the Errors heading.
///
/// let first = 1;
/// let second = 2;
/// let third = 3;
/// let fourth = 4;
///
/// assert_eq!(first + second, third);
/// assert_eq!(second + second, fourth);
///
/// assert!(lookup().is_none());
/// ```
pub fn lookup() -> Option<u32> { None }
RUST

run_rustdoc_case errors_lead "Nonstandard \`# Errors\` lead-in in crates/demo/src/lib.rs:3" 1 << 'RUST'
/// # Errors
///
/// Returns an error when the input is invalid.
pub fn parse() -> Result<(), String> { Ok(()) }
RUST

run_rustdoc_case panics_lead "Nonstandard \`# Panics\` lead-in in crates/demo/src/lib.rs:3" 1 << 'RUST'
/// # Panics
///
/// Panics when the input is invalid.
pub fn parse() { panic!("invalid input") }
RUST

run_rustdoc_case none_lead "Nonstandard \`# Errors\` lead-in in crates/demo/src/lib.rs:3" 1 << 'RUST'
/// # Errors
///
/// Returns `None` when the key is missing.
pub fn lookup() -> Option<u32> { None }
RUST

run_rustdoc_case list_lead "Nonstandard \`# Errors\` condition list lead-in in crates/demo/src/lib.rs:3" 1 << 'RUST'
/// # Errors
///
/// Returns an error if the input fails validation:
/// - The input is empty.
/// - The input is too long.
pub fn parse() -> Result<(), String> { Ok(()) }
RUST

run_rustdoc_case missing_colon "Nonstandard \`# Panics\` condition list lead-in in crates/demo/src/lib.rs:3" 1 << 'RUST'
/// # Panics
///
/// Panics if the input is invalid.
/// - The input is empty.
/// - The input is too long.
pub fn parse() { panic!("invalid input") }
RUST

run_rustdoc_case inline_colon "Nonstandard \`# Panics\` condition list lead-in in crates/demo/src/lib.rs:3" 1 << 'RUST'
/// # Panics
///
/// Panics if: the input is empty. The input is too long.
pub fn parse() { panic!("invalid input") }
RUST

run_rustdoc_case empty_condition_list "Nonstandard \`# Errors\` condition list lead-in in crates/demo/src/lib.rs:3" 1 << 'RUST'
/// # Errors
///
/// Returns an error if:
pub fn parse() -> Result<(), String> { Ok(()) }
RUST

run_rustdoc_case wrapped_bullet "Missing period on \`# Errors\` condition bullet in crates/demo/src/lib.rs:4" 1 << 'RUST'
/// # Errors
///
/// Returns an error if:
/// - The first sentence ends here.
///   The continuation has no period
/// - The second condition ends correctly.
pub fn parse() -> Result<(), String> { Ok(()) }
RUST

run_rustdoc_case separated_bullet "Missing period on \`# Panics\` condition bullet in crates/demo/src/lib.rs:4" 1 << 'RUST'
/// # Panics
///
/// Panics if:
/// - The condition has no period
///
/// Additional explanation with a period.
pub fn parse() { panic!("invalid input") }
RUST

run_rustdoc_case final_bullet "Missing period on \`# Errors\` condition bullet in crates/demo/src/lib.rs:5" 1 << 'RUST'
/// # Errors
///
/// Returns `None` if:
/// * The key is missing.
/// + The value has expired
pub fn lookup() -> Option<u32> { None }
RUST

run_rustdoc_case section_boundary "Missing period on \`# Errors\` condition bullet in crates/demo/src/lib.rs:4" 1 << 'RUST'
/// # Errors
///
/// Returns an error if:
/// - The input is empty
///
/// # Panics
///
/// Panics if the input is too long.
pub fn parse() -> Result<(), String> { panic!("invalid input") }
RUST

run_rustdoc_case suppressed_panics "Missing period on \`# Panics\` condition bullet in crates/demo/src/lib.rs:5" 1 << 'RUST'
// panics-doc-ok
/// # Panics
///
/// Panics if:
/// - The callee rejects the input
pub fn delegate() -> Result<(), String> { Ok(()) }
RUST

run_rustdoc_case suppressed_errors "Nonstandard \`# Errors\` lead-in in crates/demo/src/lib.rs:4" 1 << 'RUST'
// errors-doc-ok
/// # Errors
///
/// Fails when the trait contract is violated.
pub fn contract() {}
RUST

run_rustdoc_case empty_section "Nonstandard \`# Errors\` lead-in in crates/demo/src/lib.rs:1" 1 << 'RUST'
/// # Errors
///
/// # Notes
/// Some context.
pub fn parse() -> Result<(), String> { Ok(()) }
RUST

echo "Documentation convention hook tests passed"
