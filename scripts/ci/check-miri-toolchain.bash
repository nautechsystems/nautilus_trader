#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
TOOLS_TOML="${REPO_ROOT}/tools.toml"
CARGO_TOML="${REPO_ROOT}/Cargo.toml"

verify_installed=0
case "${1:-}" in
  "") ;;
  --verify-installed) verify_installed=1 ;;
  *)
    echo "Usage: $0 [--verify-installed]" >&2
    exit 2
    ;;
esac

miri_toolchain="$(bash "${REPO_ROOT}/scripts/tool-version.sh" miri)"
miri_rustc_version="$(awk '
  /^\[miri\]$/ { in_section=1; next }
  /^\[/ { in_section=0 }
  in_section && /^rustc-version[[:space:]]*=/ {
    gsub(/.*=[[:space:]]*"/, "")
    gsub(/".*/, "")
    print
    exit
  }
' "${TOOLS_TOML}")"
workspace_rust_version="$(awk '
  /^\[workspace\.package\]$/ { in_section=1; next }
  /^\[/ { in_section=0 }
  in_section && /^rust-version[[:space:]]*=/ {
    gsub(/.*=[[:space:]]*"/, "")
    gsub(/".*/, "")
    print
    exit
  }
' "${CARGO_TOML}")"
validate_rustc_version() {
  local label="$1"
  local version="$2"

  if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "Error: ${label} must be a numeric major.minor.patch version, was '${version}'" >&2
    exit 1
  fi
}

validate_cargo_rust_version() {
  local version="$1"

  if [[ ! "$version" =~ ^[0-9]+(\.[0-9]+){0,2}$ ]]; then
    echo "Error: Cargo.toml [workspace.package].rust-version must have one to three numeric components, was '${version}'" >&2
    exit 1
  fi
}

version_is_at_least() {
  local candidate_major candidate_minor candidate_patch
  local required_major required_minor required_patch

  IFS=. read -r candidate_major candidate_minor candidate_patch <<< "$1"
  IFS=. read -r required_major required_minor required_patch <<< "$2"
  candidate_minor="${candidate_minor:-0}"
  candidate_patch="${candidate_patch:-0}"
  required_minor="${required_minor:-0}"
  required_patch="${required_patch:-0}"

  if ((candidate_major != required_major)); then
    ((candidate_major > required_major))
  elif ((candidate_minor != required_minor)); then
    ((candidate_minor > required_minor))
  else
    ((candidate_patch >= required_patch))
  fi
}

validate_rustc_version "tools.toml [miri].rustc-version" "$miri_rustc_version"
validate_cargo_rust_version "$workspace_rust_version"

if ! version_is_at_least "$miri_rustc_version" "$workspace_rust_version"; then
  printf 'Error: Miri %s uses rustc %s, below workspace rust-version %s\n' \
    "$miri_toolchain" "$miri_rustc_version" "$workspace_rust_version" >&2
  echo "       Update tools.toml [miri] to a compatible Miri nightly." >&2
  exit 1
fi

if ((verify_installed)); then
  if ! rustc_output="$(rustc +"${miri_toolchain}" --version)"; then
    echo "Error: Could not run rustc for ${miri_toolchain}" >&2
    exit 1
  fi

  installed_rustc_version="$(printf '%s\n' "$rustc_output" | awk '{print $2; exit}')"
  installed_rustc_version="${installed_rustc_version%%-*}"
  validate_rustc_version "installed ${miri_toolchain} rustc version" "$installed_rustc_version"

  if [[ "$installed_rustc_version" != "$miri_rustc_version" ]]; then
    printf 'Error: %s provides rustc %s, expected %s from tools.toml [miri]\n' \
      "$miri_toolchain" "$installed_rustc_version" "$miri_rustc_version" >&2
    exit 1
  fi
fi

printf 'Verified %s rustc %s supports workspace Rust %s\n' \
  "$miri_toolchain" "$miri_rustc_version" "$workspace_rust_version"
