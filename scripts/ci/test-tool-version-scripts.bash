#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

case_root="$(mktemp -d)"
trap 'rm -rf "$case_root"' EXIT

mkdir -p "${case_root}/scripts"
cp \
  "${REPO_ROOT}/scripts/tool-version.sh" \
  "${REPO_ROOT}/scripts/cargo-tool-version.sh" \
  "${case_root}/scripts/"

printf '%s\n' \
  '[valid-tool]' \
  'version = "nightly-2026-08-14"' \
  '[bad-tool]' \
  'version = "1..2"' \
  > "${case_root}/tools.toml"

printf '%s\n' \
  '[workspace.metadata.tools]' \
  'cargo-good = "1.2.3-beta.1+build.2"' \
  'cargo-bad = "1.2.3-.."' \
  > "${case_root}/Cargo.toml"

expect_output() {
  local script="$1"
  local tool="$2"
  local expected="$3"
  local actual

  actual="$(bash "${case_root}/scripts/${script}" "$tool")"
  if [[ "$actual" != "$expected" ]]; then
    echo "Expected ${script} ${tool} to return ${expected}, got ${actual}" >&2
    exit 1
  fi
}

expect_failure() {
  local script="$1"
  local tool="$2"
  local expected="$3"

  if bash "${case_root}/scripts/${script}" "$tool" \
    > "${case_root}/stdout.txt" 2> "${case_root}/stderr.txt"; then
    echo "Expected ${script} ${tool} to fail" >&2
    exit 1
  fi

  if ! grep -Fq "$expected" "${case_root}/stderr.txt"; then
    echo "Expected ${script} failure reason not found: ${expected}" >&2
    cat "${case_root}/stderr.txt" >&2
    exit 1
  fi
}

expect_output "tool-version.sh" "valid-tool" "nightly-2026-08-14"
expect_failure "tool-version.sh" "valid.tool" "Invalid tool name: valid.tool"
expect_failure "tool-version.sh" "bad-tool" "Invalid version for [bad-tool]: 1..2"

expect_output "cargo-tool-version.sh" "cargo-good" "1.2.3-beta.1+build.2"
expect_failure "cargo-tool-version.sh" "cargo.tool" "Invalid cargo tool name: cargo.tool"
expect_failure "cargo-tool-version.sh" "cargo-bad" "Invalid version for cargo-bad: 1.2.3-.."

echo "Tool version script tests passed"
