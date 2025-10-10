#!/bin/bash
set -eo pipefail

# Function to update Cython version in pyproject.toml
update_cython_version() {
  local old_version="3.1.3"
  local new_version="3.0.11"

  # Create backup of original file
  cp pyproject.toml pyproject.toml.bak

  # Update all occurrences of the pinned Cython version (case-insensitive on "cython")
  # Example matches: "cython==3.1.3" or "Cython==3.1.3"
  if sed -i.tmp \
    -e "s/[cC]ython==${old_version}/cython==${new_version}/g" \
    pyproject.toml; then
    echo "Updated Cython version to ${new_version}"
    rm -f pyproject.toml.tmp
  else
    echo "Error: Failed to update Cython version in pyproject.toml" >&2
    mv pyproject.toml.bak pyproject.toml
    exit 1
  fi
}

# TODO: Temporarily change Cython version in pyproject.toml while we require v3.0.11 for coverage
update_cython_version
uv lock --no-upgrade

export PROFILE_MODE=true
uv sync --all-groups --all-extras
uv run --no-sync pytest \
  --cov-report=term \
  --cov-report=xml \
  --cov=nautilus_trader
