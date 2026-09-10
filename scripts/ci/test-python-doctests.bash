#!/usr/bin/env bash
set -euo pipefail

export PYTHONWARNDEFAULTENCODING=1
export PYTHONWARNINGS="${PYTHONWARNINGS:+$PYTHONWARNINGS,}error::EncodingWarning,ignore::EncodingWarning:plotly.validator_cache"

# shellcheck source=scripts/native-path.bash
# shellcheck disable=SC1091
source "$(dirname "${BASH_SOURCE[0]}")/../native-path.bash"

project_dir="${1:?Expected project directory}"
project_dir="$(cd "$project_dir" && pwd -P)"
project_dir="$(native_path "$project_dir")"

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
uv run --project "$project_dir" --no-sync python -m pytest \
  --rootdir="$project_dir" \
  --doctest-modules \
  --pyargs "$@"
