#!/usr/bin/env bash
set -euo pipefail

project_dir="${1:?Expected project directory}"
project_dir="$(cd "$project_dir" && pwd -P)"
temp_root="${RUNNER_TEMP:-${TMPDIR:-/tmp}}"
neutral_dir="$(mktemp -d "$temp_root/nautilus-python-doctests.XXXXXX")"
trap 'rm -rf "$neutral_dir"' EXIT

distribution_probe='import importlib.util; assert importlib.util.find_spec("nautilus_trader.backtest.engine") is None'
set -- \
  nautilus_trader.analysis.tearsheet \
  nautilus_trader.analysis.themes

unset PYTHONPATH
unset VIRTUAL_ENV
cd "$neutral_dir"
uv run --project "$project_dir" --no-sync python -c "$distribution_probe"
uv run --project "$project_dir" --no-sync pytest \
  --rootdir="$project_dir" \
  --doctest-modules \
  --pyargs "$@"
