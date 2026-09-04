#!/usr/bin/env bash
set -euo pipefail

pkg_dir="$(pwd -P)"
temp_root="${RUNNER_TEMP:-${TMPDIR:-/tmp}}"
neutral_dir="$(mktemp -d "$temp_root/nautilus-wheel.XXXXXX")"
trap 'rm -rf "$neutral_dir"' EXIT

# Pin the wheel environment so the nested doctest and type-check scripts validate the installed
# wheel rather than defaulting to the repository-root development environment. Relative values
# resolve against the uv project root, which keeps this correct under Git Bash on Windows where an
# absolute POSIX path would reach native uv unconverted.
UV_PROJECT_ENVIRONMENT=.venv
export UV_PROJECT_ENVIRONMENT

uv sync --group test --no-install-package nautilus-trader

set -- ../dist/*.whl
if [ "$#" -ne 1 ] || [ ! -f "$1" ]; then
  echo "Expected exactly one wheel in ${pkg_dir}/../dist"
  exit 1
fi

uv pip install --reinstall "$1[visualization]"

# Pin pandas test dependencies until runtime dependencies are settled
platform="$(uname -s)"
pandas_version="3.0.3"
if [ "$platform" = "Darwin" ]; then
  pandas_version="2.3.3"
fi

uv pip install --only-binary :all: \
  "numpy==2.4.6" \
  "pandas==$pandas_version" \
  "python-dateutil==2.9.0.post0" \
  "six==1.17.0"

uv pip install --only-binary :all: "pyarrow==25.0.0" # Test-only pending runtime dependencies

unset PYTHONPATH
unset VIRTUAL_ENV
TEST_DATA_ROOT_PATH="$(
  uv run --project "$pkg_dir" --no-sync python -c \
    'from pathlib import Path; print(Path.cwd().resolve().parent)'
)"
export TEST_DATA_ROOT_PATH
cd "$neutral_dir"

uv run --project "$pkg_dir" --no-sync python -c '
import pathlib
import sys

import nautilus_trader

package_dir = pathlib.Path(nautilus_trader.__file__).resolve().parent
environment_dir = pathlib.Path(sys.prefix).resolve()

if not package_dir.is_relative_to(environment_dir):
    sys.exit(f"Expected the wheel installed in {environment_dir}, imported {package_dir}")
'

uv run --project "$pkg_dir" --no-sync pytest \
  --import-mode=importlib \
  --rootdir="$pkg_dir" \
  "$pkg_dir/tests/" -v

bash "$pkg_dir/../scripts/ci/test-python-doctests.bash" "$pkg_dir"

types_dir="$neutral_dir/types"
mkdir "$types_dir"
cp -R "$pkg_dir/../examples" "$types_dir/examples"

bash "$pkg_dir/../scripts/ci/check-python-types.bash" \
  "$pkg_dir" \
  "$types_dir/examples"
