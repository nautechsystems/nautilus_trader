#!/usr/bin/env bash

set -euo pipefail

REPO_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
PYTEST_VERSION=$(
  python3 - "$REPO_ROOT/python/uv.lock" << 'PY'
import sys
import tomllib

with open(sys.argv[1], "rb") as lock:
    packages = tomllib.load(lock)["package"]
print(next(package["version"] for package in packages if package["name"] == "pytest"))
PY
)
exec env PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 uv run --project "$REPO_ROOT/python" \
  --locked --only-group dev --with "pytest==$PYTEST_VERSION" \
  python -B -m pytest --noconftest -o addopts= -q \
  "$REPO_ROOT/.pre-commit-hooks/test_check_formatting_py.py"
