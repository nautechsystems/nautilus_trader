#!/usr/bin/env bash
set -euo pipefail

export PYTHONWARNDEFAULTENCODING=1
export PYTHONWARNINGS="${PYTHONWARNINGS:+$PYTHONWARNINGS,}error::EncodingWarning,ignore::EncodingWarning:plotly.validator_cache"

# shellcheck source=scripts/native-path.bash
# shellcheck disable=SC1091
source "$(dirname "${BASH_SOURCE[0]}")/../native-path.bash"

project_dir="${1:?Expected isolated Python project directory}"
pkg_dir="${2:?Expected source Python project directory}"
project_dir="$(cd "$project_dir" && pwd -P)"
pkg_dir="$(cd "$pkg_dir" && pwd -P)"
shift 2
project_dir_native="$(native_path "$project_dir")"
pkg_dir_native="$(native_path "$pkg_dir")"

neutral_dir="$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/nautilus-python-checks.XXXXXX")"
trap 'rm -rf "$neutral_dir"' EXIT
unset PYTHONPATH
unset VIRTUAL_ENV
unset UV_PROJECT_ENVIRONMENT
cd "$pkg_dir"
TEST_DATA_ROOT_PATH="$(
  uv run --project "$project_dir_native" --no-sync python -c \
    'from pathlib import Path; print(Path.cwd().resolve().parent)'
)"
export TEST_DATA_ROOT_PATH
cd "$neutral_dir"

uv run --project "$project_dir_native" --no-sync python -c '
import pathlib
import sys

import nautilus_trader

package_dir = pathlib.Path(nautilus_trader.__file__).resolve().parent
environment_dir = pathlib.Path(sys.prefix).resolve()
if not package_dir.is_relative_to(environment_dir):
    sys.exit(f"Expected the package installed in {environment_dir}, imported {package_dir}")
'

if [ "$#" -eq 0 ]; then
  set -- "$pkg_dir_native/tests/"
fi
uv run --project "$project_dir_native" --no-sync python -m pytest \
  --import-mode=importlib --rootdir="$pkg_dir_native" "$@" -v

bash "$pkg_dir/../scripts/ci/test-python-doctests.bash" "$project_dir"
cp -R "$pkg_dir/../examples" "$neutral_dir/examples"
bash "$pkg_dir/../scripts/ci/check-python-types.bash" \
  "$pkg_dir" "$neutral_dir/examples" "$project_dir"
