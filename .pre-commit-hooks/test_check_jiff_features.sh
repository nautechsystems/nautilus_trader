#!/usr/bin/env bash

set -euo pipefail

for command in cargo jq rg; do
  if ! command -v "$command" &> /dev/null; then
    echo "ERROR: $command is required for Jiff feature hook tests" >&2
    exit 1
  fi
done

REPO_ROOT=$(git rev-parse --show-toplevel)
HOOK="$REPO_ROOT/.pre-commit-hooks/check_jiff_features.sh"

CASE_ROOT=$(mktemp -d)
trap 'rm -rf "$CASE_ROOT"' EXIT
export CARGO_NET_OFFLINE=true

write_package() {
  local directory="$1"
  local name="$2"
  local version="$3"

  mkdir -p "$directory/src"
  touch "$directory/src/lib.rs"
  printf '[package]\nname = "%s"\nversion = "%s"\nedition = "2024"\n' \
    "$name" "$version" > "$directory/Cargo.toml"
}

create_case() {
  local name="$1"
  local case_dir="$CASE_ROOT/$name"
  local workspace="$case_dir/workspace"
  local jiff_declaration='jiff = { workspace = true }'
  local extra_features=''

  case "$name" in
    workspace-violation) jiff_declaration='jiff = { workspace = true, features = ["std"] }' ;;
    feature-violation) extra_features=', "tz-system"' ;;
  esac

  mkdir -p "$workspace/.pre-commit-hooks"
  cp "$HOOK" "$workspace/.pre-commit-hooks/"
  cp "$REPO_ROOT/rust-toolchain.toml" "$workspace/"
  cat > "$workspace/Cargo.toml" << MANIFEST
[workspace]
members = ["crates/*"]
resolver = "3"

[workspace.dependencies]
jiff = { path = "../jiff-02", default-features = false, features = [
  "alloc", "perf-inline", "serde", "std", "tz-fat", "tzdb-bundle-always"${extra_features},
] }
MANIFEST

  write_package "$case_dir/jiff-02" jiff 0.2.0
  cat >> "$case_dir/jiff-02/Cargo.toml" << 'MANIFEST'

[features]
default = ["tz-system", "tzdb-bundle-platform", "tzdb-concatenated", "tzdb-zoneinfo"]
alloc = []
perf-inline = []
serde = []
std = []
tz-fat = []
tz-system = []
tzdb-bundle-always = []
tzdb-bundle-platform = []
tzdb-concatenated = []
tzdb-zoneinfo = []
MANIFEST

  write_package "$case_dir/jiff-1" jiff 1.0.0
  write_package "$case_dir/python-time" python-time 0.1.0
  cat >> "$case_dir/python-time/Cargo.toml" << 'MANIFEST'

[dependencies]
jiff = { path = "../jiff-02" }
jiff-next = { package = "jiff", path = "../jiff-1" }
MANIFEST

  write_package "$workspace/crates/nautilus-trading" nautilus-trading 0.1.0
  printf '\n[dependencies]\n%s\n' "$jiff_declaration" >> "$workspace/crates/nautilus-trading/Cargo.toml"

  write_package "$workspace/crates/nautilus-pyo3" nautilus-pyo3 0.1.0
  cat >> "$workspace/crates/nautilus-pyo3/Cargo.toml" << 'MANIFEST'

[dependencies]
jiff = { workspace = true }
python-time = { path = "../../../python-time" }
MANIFEST

  if [[ "$name" == patch-violation ]]; then
    write_package "$case_dir/patches/example" patched-example 0.1.0
    printf '\n[dependencies]\njiff = { path = "../../jiff-02" }\n' \
      >> "$case_dir/patches/example/Cargo.toml"
    printf 'patched-example = { path = "../../../patches/example" }\n' \
      >> "$workspace/crates/nautilus-pyo3/Cargo.toml"
  fi

  (cd "$workspace" && cargo generate-lockfile --offline)
  printf '%s\n' "$workspace"
}

expect_failure() {
  local workspace="$1"
  local reason="$2"
  local status=0

  bash "$workspace/.pre-commit-hooks/check_jiff_features.sh" \
    > "$workspace/stdout.txt" 2> "$workspace/stderr.txt" || status=$?
  if [[ "$status" != 1 ]] || ! rg -Fq "$reason" "$workspace/stderr.txt"; then
    echo "Expected Jiff feature check to fail with: $reason" >&2
    cat "$workspace/stdout.txt" "$workspace/stderr.txt" >&2
    exit 1
  fi
}

valid_case=$(create_case valid)
bash "$valid_case/.pre-commit-hooks/check_jiff_features.sh"

expect_failure "$(create_case workspace-violation)" \
  "Workspace crates must inherit Jiff without adding features"
expect_failure "$(create_case patch-violation)" \
  "Maintained patches must keep Jiff independent of system time zone sources"
expect_failure "$(create_case feature-violation)" \
  "Jiff feature policy changed for nautilus-trading"

echo "Jiff feature hook tests passed"
