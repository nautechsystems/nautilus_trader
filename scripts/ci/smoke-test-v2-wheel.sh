#!/usr/bin/env bash
# Smoke test a freshly built v2 wheel across platforms: install it into the synced venv,
# then import the package and key submodules from a neutral directory so the installed
# wheel is exercised rather than the in-tree source. Run from the python/ package directory.
set -euo pipefail

pkg_dir="$(pwd)"
neutral_dir="${RUNNER_TEMP:-/tmp}"

uv sync --no-install-package nautilus-trader
uv pip install "${pkg_dir}/../dist/"*.whl

cd "$neutral_dir"
uv run --project "$pkg_dir" --no-sync python - << 'PY'
import importlib

import nautilus_trader

submodules = [
    "model",
    "common",
    "core",
    "live",
    "backtest",
    "testkit",
    "adapters.lighter",
]
for name in submodules:
    importlib.import_module(f"nautilus_trader.{name}")

print(f"nautilus_trader {nautilus_trader.__version__} imported OK")
PY
