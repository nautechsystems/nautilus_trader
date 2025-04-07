#!/bin/bash
set -eo pipefail

# Function to update Cython version in pyproject.toml
update_cython_version() {
    local old_version="3.1.0b1"
    local new_version="3.0.11"

    # Create backup of original file
    cp pyproject.toml pyproject.toml.bak

    # Update Cython version in both dependencies and build-system sections
    if sed -i.tmp \
        -e "s/cython = \"==.*\"/cython = \"==${new_version}\"/" \
        -e "s/\"Cython==.*\"/\"Cython==${new_version}\"/" \
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
