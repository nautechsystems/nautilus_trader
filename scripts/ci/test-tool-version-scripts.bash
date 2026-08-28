#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

case_root="$(mktemp -d "${TMPDIR:-/tmp}/nautilus-tool-version-test.XXXXXX")"
trap 'rm -rf "$case_root"' EXIT

mkdir -p "${case_root}/scripts"
cp "${REPO_ROOT}/scripts/tool-version.sh" "${case_root}/scripts/"

printf '%s\n' \
  '[valid-tool]' \
  'version = "nightly-2026-08-14"' \
  '[bad-tool]' \
  'version = "1..2"' \
  > "${case_root}/tools.toml"

expect_output() {
  local tool=$1 expected=$2 actual

  actual=$(bash "${case_root}/scripts/tool-version.sh" "$tool")
  if [[ "$actual" != "$expected" ]]; then
    printf 'Expected tool-version.sh %s to return %s, was %s\n' "$tool" "$expected" "$actual" >&2
    exit 1
  fi
}

expect_failure() {
  local tool=$1 expected=$2

  if bash "${case_root}/scripts/tool-version.sh" "$tool" \
    > "${case_root}/stdout.txt" 2> "${case_root}/stderr.txt"; then
    printf 'Expected tool-version.sh %s to fail\n' "$tool" >&2
    exit 1
  fi
  if ! grep -Fq "$expected" "${case_root}/stderr.txt"; then
    printf 'Expected tool-version.sh failure reason not found: %s\n' "$expected" >&2
    cat "${case_root}/stderr.txt" >&2
    exit 1
  fi
}

expect_output valid-tool nightly-2026-08-14
expect_failure valid.tool 'Invalid tool name: valid.tool'
expect_failure bad-tool 'Invalid version for [bad-tool]: 1..2'

cargo_tool_count=0
while IFS= read -r tool; do
  version=$(bash "${REPO_ROOT}/scripts/cargo-tool-version.sh" "$tool")
  if [[ -z "$version" ]]; then
    printf 'Cargo tool version was empty for %s\n' "$tool" >&2
    exit 1
  fi
  cargo_tool_count=$((cargo_tool_count + 1))
done < <(awk '
  /^\[workspace\.metadata\.tools\]/ { in_section=1; next }
  /^\[/ { in_section=0 }
  in_section && /^[a-z0-9][a-z0-9_-]*[[:space:]]*=/ { print $1 }
' "${REPO_ROOT}/Cargo.toml")

if ((cargo_tool_count == 0)); then
  echo "No local Cargo tool pins were found" >&2
  exit 1
fi
if [[ -z $(bash "${REPO_ROOT}/scripts/rust-toolchain.sh") ]]; then
  echo "Rust toolchain version was empty" >&2
  exit 1
fi

echo "Tool version consumer tests passed"
