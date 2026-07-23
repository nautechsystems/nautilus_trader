#!/usr/bin/env bash
set -euo pipefail

pkg_dir="$(pwd)"
neutral_dir="${RUNNER_TEMP:-/tmp}"

uv sync --group test --no-install-package nautilus-trader

set -- ../dist/*.whl
if [ "$#" -ne 1 ] || [ ! -f "$1" ]; then
  echo "Expected exactly one wheel in ${pkg_dir}/../dist"
  exit 1
fi

uv pip install "$1[visualization]"

# Pin pandas test dependencies until v2 runtime dependencies are settled
uv pip install --only-binary :all: \
  "numpy==2.4.6" \
  "pandas==3.0.3" \
  "python-dateutil==2.9.0.post0" \
  "six==1.17.0"

unset PYTHONPATH
unset VIRTUAL_ENV
cd "$neutral_dir"
uv run --project "$pkg_dir" --no-sync pytest \
  --import-mode=importlib \
  --rootdir="$pkg_dir" \
  "$pkg_dir/tests/" -v
