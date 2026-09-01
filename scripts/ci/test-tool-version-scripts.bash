#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

for catalog in \
  "${REPO_ROOT}/.nautilus-engineering/tools.toml" \
  "${REPO_ROOT}/tools.toml"; do
  tool_count=0
  while IFS= read -r tool; do
    version=$(bash "${REPO_ROOT}/scripts/tool-version.sh" "$tool")
    if [[ -z "$version" ]]; then
      printf 'Tool version was empty for %s\n' "$tool" >&2
      exit 1
    fi
    tool_count=$((tool_count + 1))
  done < <(awk '
    /^\[[a-z0-9][a-z0-9_-]*\]$/ {
      section=$0
      gsub(/^\[|\]$/, "", section)
      print section
    }
  ' "$catalog")

  if ((tool_count == 0)); then
    printf 'No tool pins were found in %s\n' "$catalog" >&2
    exit 1
  fi
done

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

for tool in cargo-audit cargo-deny cargo-edit cargo-llvm-cov cargo-nextest cargo-vet; do
  if [[ -z $(bash "${REPO_ROOT}/scripts/cargo-tool-version.sh" "$tool") ]]; then
    printf 'Shared Cargo tool version was empty for %s\n' "$tool" >&2
    exit 1
  fi
done

uv_version=$(bash "${REPO_ROOT}/scripts/uv-version.sh")
if [[ "$uv_version" != $(bash "${REPO_ROOT}/scripts/tool-version.sh" uv) ]]; then
  echo "uv-version.sh did not match the shared uv pin" >&2
  exit 1
fi

if [[ -z $(bash "${REPO_ROOT}/scripts/rust-toolchain.sh") ]]; then
  echo "Rust toolchain version was empty" >&2
  exit 1
fi

echo "Tool version consumer tests passed"
