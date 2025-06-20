#!/bin/bash
# NautilusTrader environment setup
#
# Use uv to install all dependencies and configure environment variables
# required for building Python extensions.
#
# Usage:
#   source scripts/setup-env.sh
# or
#   bash scripts/setup-env.sh
# (the environment variables will only persist in the current shell if the script
#  is sourced)

set -e

# Install all dependencies using uv
uv sync --active --all-groups --all-extras

# Path to the Python interpreter inside the virtual environment
PYTHON_BIN="$(pwd)/.venv/bin/python"

# Configure PyO3 variables for Rust builds
LIB_DIR="$($PYTHON_BIN - <<'PY'
import sysconfig
print(sysconfig.get_config_var('LIBDIR'))
PY
)"
export LD_LIBRARY_PATH="${LIB_DIR}:${LD_LIBRARY_PATH}"
export PYO3_PYTHON="${PYTHON_BIN}"

# Install the pre-commit hook
pre-commit install

echo "Environment setup complete." 
