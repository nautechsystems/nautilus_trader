#!/usr/bin/env bash

set -euo pipefail

if ! command -v rg &> /dev/null; then
  echo "ERROR: ripgrep is required for Cargo convention hook tests" >&2
  echo "       install from: https://github.com/BurntSushi/ripgrep#installation" >&2
  exit 1
fi

REPO_ROOT=$(git rev-parse --show-toplevel)
HOOK="$REPO_ROOT/.pre-commit-hooks/check_cargo_conventions.sh"

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
mkdir -p "$valid_case/crates/valid/src"
printf '%s\n' \
  '[workspace]' \
  'members = ["crates/valid"]' \
  '' \
  '[workspace.package]' \
  'version = "0.1.0"' \
  'edition = "2024"' \
  '' \
  '[workspace.dependencies]' \
  'nautilus-alpha = "1.0"' \
  'nautilus-beta = { version = "1.0", optional = true }' \
  '' \
  'alpha = "1.0"' \
  '' \
  '[workspace.metadata.example]' \
  'members = ["not-a-crate"]' > "$valid_case/Cargo.toml"
printf '%s\n' \
  '[package]' \
  'name = "valid"' \
  'version.workspace = true' \
  'edition.workspace = true' \
  'rust-version.workspace = true' \
  'authors.workspace = true' \
  'license.workspace = true' \
  'description = "Valid fixture"' \
  'categories.workspace = true' \
  'keywords.workspace = true' \
  'documentation.workspace = true' \
  'repository.workspace = true' \
  'homepage.workspace = true' \
  '' \
  '[lints]' \
  'workspace = true' \
  '' \
  '[lib]' \
  '' \
  '[package.metadata.docs.rs]' \
  'readme = "README.md"' \
  '' \
  '[dependencies]' \
  'nautilus-alpha = { workspace = true }' \
  'nautilus-beta = { workspace = true, optional = true }' \
  '' \
  'alpha = { workspace = true }' \
  '' \
  'omega = { workspace = true, optional = true }' \
  '' \
  '[[bin]]' \
  'name = "valid-tool"' \
  'doc = false' \
  'test = false' > "$valid_case/crates/valid/Cargo.toml"
run_hook "$valid_case"
if [ "$RUN_STATUS" -ne 0 ]; then
  echo "Expected Cargo convention hook to pass valid input"
  cat "$valid_case/output.txt"
  exit 1
fi
rg -Fq "All Cargo.toml conventions are valid" "$valid_case/output.txt"

invalid_case="$CASE_ROOT/invalid"
mkdir -p "$invalid_case/crates/core/src" "$invalid_case/crates/adapters/demo/src"
printf '%s\n' \
  '[workspace]' \
  'members = [' \
  '  "crates/core",' \
  '  "crates/adapters/demo",' \
  ']' \
  '' \
  '[workspace.package]' \
  'readme = "README.md"' \
  '' \
  '[workspace.dependencies]' \
  'zeta = "1.0"' \
  'alpha = "1.0"' \
  'capnp = "1.0"' \
  'capnpc = "2.0"' \
  'unused = "1.0"' \
  '' \
  '# Adapter dependencies' \
  '# --------------------' \
  'adapter-only = "1.0"' \
  '# --------------------' > "$invalid_case/Cargo.toml"
printf '%s\n' \
  '[package]' \
  'name = "core"' \
  'readme = "README.md"' \
  'description = "Incomplete fixture"' \
  '' \
  '[dependencies]' \
  'zeta = { workspace = true }' \
  'adapter-only = { workspace = true }' \
  'readme.workspace = true' \
  '' \
  '[lints]' \
  'workspace = true' \
  '' \
  '[lib]' \
  'crate-type = ["cdylib", "rlib"]' \
  '' \
  '[package.metadata.cargo-machete]' \
  'ignored = ["ghost"]' \
  '' \
  '[[bin]]' \
  'name = "core_tool"' > "$invalid_case/crates/core/Cargo.toml"
printf '%s\n' \
  '[package]' \
  'name = "demo"' \
  '' \
  '[lib]' \
  '' \
  '[dependencies]' \
  'libfuzzer-sys = "0.4"' > "$invalid_case/crates/adapters/demo/Cargo.toml"
run_hook "$invalid_case"
if [ "$RUN_STATUS" -ne 1 ]; then
  echo "Expected Cargo convention hook to reject violations"
  cat "$invalid_case/output.txt"
  exit 1
fi

for heading in \
  "Dependency ordering violations:" \
  "Section ordering violations:" \
  "Missing [lints] section:" \
  "Missing doc = false or test = false:" \
  "[package] section violations:" \
  "crate-type ordering violations:" \
  "Unused workspace dependencies:" \
  "Version alignment violations:" \
  "Adapter dependency section violations:" \
  "Stale cargo-machete ignored entries:" \
  "Direct adapter libfuzzer-sys dependencies:" \
  "Redundant readme keys:" \
  "Uninherited workspace package fields:" \
  "Non kebab-case [[bin]] names:"; do
  if ! rg -Fq "$heading" "$invalid_case/output.txt"; then
    echo "Expected Cargo convention result not found: $heading"
    cat "$invalid_case/output.txt"
    exit 1
  fi
done

unresolved_case="$CASE_ROOT/unresolved"
mkdir -p "$unresolved_case/crates/real/src"
printf '%s\n' \
  '[workspace]' \
  'members = ["crates/*", "crates/real"]' \
  '' \
  '[workspace.dependencies]' \
  'alpha = "1.0"' > "$unresolved_case/Cargo.toml"
printf '%s\n' \
  '[package]' \
  'name = "real"' \
  'readme = "README.md"' \
  'version.workspace = true' \
  'edition.workspace = true' \
  'rust-version.workspace = true' \
  'authors.workspace = true' \
  'license.workspace = true' \
  'description = "Glob member fixture"' \
  'categories.workspace = true' \
  'keywords.workspace = true' \
  'documentation.workspace = true' \
  'repository.workspace = true' \
  'homepage.workspace = true' \
  '' \
  '[lints]' \
  'workspace = true' \
  '' \
  '[lib]' \
  '' \
  '[dependencies]' \
  'alpha = { workspace = true }' \
  '' \
  '[[bin]]' \
  'name = "snake_tool"' \
  'doc = false' \
  'test = false' > "$unresolved_case/crates/real/Cargo.toml"
run_hook "$unresolved_case"
if [ "$RUN_STATUS" -ne 1 ]; then
  echo "Expected Cargo convention hook to reject an unresolvable workspace member"
  cat "$unresolved_case/output.txt"
  exit 1
fi
for heading in \
  "Unresolvable workspace members:" \
  "Redundant readme keys:" \
  "Non kebab-case [[bin]] names:"; do
  if ! rg -Fq "$heading" "$unresolved_case/output.txt"; then
    echo "Expected unresolved-case result not found: $heading"
    cat "$unresolved_case/output.txt"
    exit 1
  fi
done

optional_group_case="$CASE_ROOT/optional-group"
mkdir -p "$optional_group_case/crates/core/src"
printf '%s\n' \
  '[workspace]' \
  'members = ["crates/core"]' \
  '' \
  '[workspace.package]' \
  'version = "0.1.0"' \
  'edition = "2024"' \
  '' \
  '[workspace.dependencies]' \
  'log = "0.4"' \
  'madsim = "0.2"' \
  'parking_lot = "0.12"' > "$optional_group_case/Cargo.toml"
printf '%s\n' \
  '[package]' \
  'name = "core"' \
  'version.workspace = true' \
  'edition.workspace = true' \
  'rust-version.workspace = true' \
  'authors.workspace = true' \
  'license.workspace = true' \
  'description = "Optional grouping fixture"' \
  'categories.workspace = true' \
  'keywords.workspace = true' \
  'documentation.workspace = true' \
  'repository.workspace = true' \
  'homepage.workspace = true' \
  '' \
  '[lints]' \
  'workspace = true' \
  '' \
  '[lib]' \
  '' \
  '[dependencies]' \
  'log = { workspace = true }' \
  'madsim = { workspace = true, optional = true }' \
  'parking_lot = { workspace = true }' \
  '' \
  '[dev-dependencies]' \
  'anyhow = { workspace = true }' \
  'bytes = { workspace = true, features = [' \
  '  "std",' \
  '], optional = true }' > "$optional_group_case/crates/core/Cargo.toml"
run_hook "$optional_group_case"
if [ "$RUN_STATUS" -ne 1 ]; then
  echo "Expected Cargo convention hook to reject mixed optional grouping"
  cat "$optional_group_case/output.txt"
  exit 1
fi
if ! rg -Fq "Optional grouping violations:" "$optional_group_case/output.txt"; then
  echo "Expected optional grouping heading not found"
  cat "$optional_group_case/output.txt"
  exit 1
fi
if ! rg -Fq "crates/core/Cargo.toml:22 [dependencies] optional 'madsim' must sit in its own group, not with required 'log'" "$optional_group_case/output.txt"; then
  echo "Expected optional grouping violation for madsim not found"
  cat "$optional_group_case/output.txt"
  exit 1
fi
if ! rg -Fq "crates/core/Cargo.toml:27 [dev-dependencies] optional 'bytes' must sit in its own group, not with required 'anyhow'" "$optional_group_case/output.txt"; then
  echo "Expected optional grouping violation for multi-line bytes not found"
  cat "$optional_group_case/output.txt"
  exit 1
fi

echo "Cargo convention hook tests passed"
