#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

test_root="$(mktemp -d)"
trap 'rm -rf "$test_root"' EXIT

mkdir -p "${test_root}/scripts"
cp "${REPO_ROOT}/scripts/rust-toolchain.sh" "${test_root}/scripts/rust-toolchain.sh"

expect_success() {
  local config="$1"
  local expected="$2"

  printf '%s\n' "$config" > "${test_root}/rust-toolchain.toml"
  actual="$(bash "${test_root}/scripts/rust-toolchain.sh")"
  if [[ "$actual" != "$expected" ]]; then
    echo "Expected Rust toolchain '$expected', was '$actual'" >&2
    exit 1
  fi
}

expect_failure() {
  local config="$1"

  printf '%s\n' "$config" > "${test_root}/rust-toolchain.toml"
  if bash "${test_root}/scripts/rust-toolchain.sh" > "${test_root}/output" 2>&1; then
    echo "Expected Rust toolchain config to fail:" >&2
    printf '%s\n' "$config" >&2
    exit 1
  fi
  grep -Fq "[toolchain].channel must be an exact Rust version" "${test_root}/output"
}

expect_success $'[metadata]\nchannel = "stable"\n\n[toolchain]\nchannel = "1.97.1"' "1.97.1"
expect_failure $'[toolchain]\nversion = "1.97.1"\nchannel = "stable"'
expect_failure $'[toolchain]\nchannel = "stable"'
expect_failure $'[metadata]\nchannel = "1.97.1"\n\n[toolchain]\nprofile = "minimal"'

echo "Rust toolchain parser tests passed"
