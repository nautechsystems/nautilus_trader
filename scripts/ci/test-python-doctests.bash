#!/usr/bin/env bash
set -euo pipefail

distribution="${1:?Expected distribution: v1 or v2}"
project_dir="${2:?Expected project directory}"
project_dir="$(cd "$project_dir" && pwd -P)"
temp_root="${RUNNER_TEMP:-${TMPDIR:-/tmp}}"
neutral_dir="$(mktemp -d "$temp_root/nautilus-python-doctests.XXXXXX")"
trap 'rm -rf "$neutral_dir"' EXIT

case "$distribution" in
  v1)
    distribution_probe='import importlib.util; assert importlib.util.find_spec("nautilus_trader.backtest.engine") is not None'
    set -- \
      nautilus_trader.adapters.betfair.parsing.common \
      nautilus_trader.analysis.tearsheet \
      nautilus_trader.analysis.themes \
      nautilus_trader.persistence.funcs
    ;;
  v2)
    distribution_probe='import importlib.util; assert importlib.util.find_spec("nautilus_trader.backtest.engine") is None'
    set -- \
      nautilus_trader.analysis.tearsheet \
      nautilus_trader.analysis.themes
    ;;
  *)
    echo "Unknown Python distribution: $distribution"
    exit 1
    ;;
esac

unset PYTHONPATH
unset VIRTUAL_ENV
unset MYPYPATH
cd "$neutral_dir"
uv run --project "$project_dir" --no-sync python -c "$distribution_probe"
uv run --project "$project_dir" --no-sync pytest \
  --rootdir="$project_dir" \
  --doctest-modules \
  --pyargs "$@"
