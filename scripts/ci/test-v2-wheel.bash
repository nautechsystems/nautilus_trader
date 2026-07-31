#!/usr/bin/env bash
set -euo pipefail

pkg_dir="$(pwd -P)"
temp_root="${RUNNER_TEMP:-${TMPDIR:-/tmp}}"
neutral_dir="$(mktemp -d "$temp_root/nautilus-v2-wheel.XXXXXX")"
trap 'rm -rf "$neutral_dir"' EXIT

uv sync --group test --no-install-package nautilus-trader

set -- ../dist/*.whl
if [ "$#" -ne 1 ] || [ ! -f "$1" ]; then
  echo "Expected exactly one wheel in ${pkg_dir}/../dist"
  exit 1
fi

uv pip install --reinstall "$1[visualization]"

# Pin pandas test dependencies until v2 runtime dependencies are settled
uv pip install --only-binary :all: \
  "numpy==2.4.6" \
  "pandas==3.0.3" \
  "python-dateutil==2.9.0.post0" \
  "six==1.17.0"

unset PYTHONPATH
unset VIRTUAL_ENV
unset MYPYPATH
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

bash "$pkg_dir/../scripts/ci/test-python-doctests.bash" v2 "$pkg_dir"

mypy_dir="$neutral_dir/mypy"
mkdir "$mypy_dir"
cp -R "$pkg_dir/examples" "$mypy_dir/examples"
cp "$pkg_dir/tests/type_checking/supported.py" "$mypy_dir/supported.py"

uv run --project "$pkg_dir" --no-sync mypy \
  --config-file "$pkg_dir/pyproject.toml" \
  "$mypy_dir/examples" \
  "$mypy_dir/supported.py"
