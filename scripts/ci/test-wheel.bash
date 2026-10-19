#!/usr/bin/env bash
set -euo pipefail

pkg_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../python" && pwd -P)"
# shellcheck source=scripts/native-path.bash
source "$pkg_dir/../scripts/native-path.bash"
wheel_dir="${1:-$pkg_dir/../dist}"
wheel_dir="$(cd "$wheel_dir" && pwd -P)"
temp_root="${RUNNER_TEMP:-${TMPDIR:-/tmp}}"
neutral_dir="$(mktemp -d "$temp_root/nautilus-wheel.XXXXXX")"
trap 'rm -rf "$neutral_dir"' EXIT

set -- "$wheel_dir"/*.whl
if [ "$#" -ne 1 ] || [ ! -f "$1" ]; then
  echo "Expected exactly one wheel in $wheel_dir"
  exit 1
fi

unset PYTHONPATH
unset VIRTUAL_ENV
unset UV_PROJECT_ENVIRONMENT
cd "$pkg_dir"
project_python="$(uv python find)"

# A separate project preserves uv's default .venv layout and the development installation
wheel_project="$neutral_dir/python"
mkdir "$wheel_project"
cp "$pkg_dir/pyproject.toml" "$pkg_dir/uv.lock" "$wheel_project/"

wheel_project_native="$(native_path "$wheel_project")"
wheel_path="$(native_path "$1")"
uv sync --project "$wheel_project_native" --python "$project_python" --frozen --group test --no-install-package nautilus-trader
wheel_python="$(uv run --project "$wheel_project_native" --no-sync python -c 'import sys; print(sys.executable)')"
uv pip install --python "$wheel_python" --reinstall "${wheel_path}[visualization]"

# Pin pandas test dependencies until runtime dependencies are settled
platform="$(uname -s)"
pandas_version="3.0.3"
if [ "$platform" = "Darwin" ]; then
  pandas_version="2.3.3"
fi

uv pip install --python "$wheel_python" --only-binary :all: \
  "numpy==2.4.6" \
  "pandas==$pandas_version" \
  "python-dateutil==2.9.0.post0" \
  "six==1.17.0"

uv pip install --python "$wheel_python" --only-binary :all: "pyarrow==25.0.0" # Test-only pending runtime dependencies

bash "$pkg_dir/../scripts/ci/check-python-isolation.bash" "$wheel_project" "$pkg_dir"
