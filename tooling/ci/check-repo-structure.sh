#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(git rev-parse --show-toplevel)"
cd "$ROOT_DIR"

fail() {
  echo "$1" >&2
  exit 1
}

require_symlink() {
  local path="$1"
  local expected="$2"

  if [[ ! -L "$path" ]]; then
    fail "[repo-structure] $path must be a symlink to $expected."
  fi

  local actual
  actual="$(readlink "$path")"
  if [[ "$actual" != "$expected" ]]; then
    fail "[repo-structure] $path must point to $expected (got $actual)."
  fi
}

require_symlink docs/flux ../systems/flux/docs
require_symlink docs/fluxboard ../apps/fluxboard/docs

mapfile -t legacy_script_entries < <(find scripts \( -type f -o -type l \) ! -path 'scripts/README.md' | sort)
bad_legacy_script_entries=()
for path in "${legacy_script_entries[@]}"; do
  if [[ ! -L "$path" ]]; then
    bad_legacy_script_entries+=("$path")
  fi
done

if (( ${#bad_legacy_script_entries[@]} > 0 )); then
  printf '[repo-structure] Non-symlink entries under scripts/:\n' >&2
  printf '  %s\n' "${bad_legacy_script_entries[@]}" >&2
  fail "[repo-structure] Found real files under legacy scripts/ compatibility tree."
fi

mapfile -t legacy_flux_entries < <(
  find nautilus_trader/flux -mindepth 1 \
    ! -path 'nautilus_trader/flux/__init__.py' \
    ! -path 'nautilus_trader/flux/__pycache__' \
    ! -path 'nautilus_trader/flux/__pycache__/*' \
    | sort
)

if (( ${#legacy_flux_entries[@]} > 0 )); then
  printf '[repo-structure] Unexpected entries under nautilus_trader/flux:\n' >&2
  printf '  %s\n' "${legacy_flux_entries[@]}" >&2
  fail "[repo-structure] Found implementation under legacy nautilus_trader/flux compatibility path."
fi

ACTIVE_REFERENCE_PATHS=(
  README.md
  CONTRIBUTING.md
  docs/repo
  docs/developer_guide
  .github
  .pre-commit-config.yaml
  fluxboard/README.md
  fluxboard/docs
  systems/flux/docs
  deploy/tokenmm
  examples/live/makerv3
  tooling/README.md
  ops/README.md
  scripts/README.md
)

existing_active_reference_paths=()
for path in "${ACTIVE_REFERENCE_PATHS[@]}"; do
  if [[ -e "$path" || -L "$path" ]]; then
    existing_active_reference_paths+=("$path")
  fi
done

LEGACY_PATH_PATTERN='(?<![[:alnum:]_./-])(scripts/ci/|scripts/cli/|scripts/deploy/|docs/fluxboard/|docs/flux/)'

if (( ${#existing_active_reference_paths[@]} > 0 )) && rg -n -P "$LEGACY_PATH_PATTERN" "${existing_active_reference_paths[@]}"; then
  fail "[repo-structure] Found legacy repo path references in active docs/config/workflow files."
fi

LEGACY_HELPER_REFERENCE_PATHS=(
  README.md
  CONTRIBUTING.md
  docs/repo
  docs/developer_guide
  tooling
  .github/actions/common-setup/action.yml
  .github/actions/cargo-tool-install/action.yml
  .github/workflows/build.yml
  .github/workflows/codeql-analysis.yml
  .github/workflows/coverage.yml
  .github/workflows/nightly-docs-features-check.yml
)

existing_legacy_helper_reference_paths=()
for path in "${LEGACY_HELPER_REFERENCE_PATHS[@]}"; do
  if [[ -e "$path" || -L "$path" ]]; then
    existing_legacy_helper_reference_paths+=("$path")
  fi
done

LEGACY_HELPER_PATTERN='(?<![[:alnum:]_./-])(scripts/rust-toolchain\.sh|scripts/python-version\.sh|scripts/pre-commit-version\.sh|scripts/package-version\.sh|scripts/test-coverage\.sh)(?![[:alnum:]_./-])'

if (( ${#existing_legacy_helper_reference_paths[@]} > 0 )) && rg -n -P "$LEGACY_HELPER_PATTERN" "${existing_legacy_helper_reference_paths[@]}"; then
  fail "[repo-structure] Found legacy helper script references in active workflow/docs files."
fi

echo "[repo-structure] OK"
