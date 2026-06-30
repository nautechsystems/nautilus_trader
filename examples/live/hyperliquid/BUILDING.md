# Hyperliquid Build Guide

This guide is for building enough NautilusTrader to run the root-package
Hyperliquid live examples, including:

```bash
uv run --no-sync python examples/live/hyperliquid/hyperliquid_sweep_strategy.py \
  --config examples/live/hyperliquid/sweep_basket_config.example.json
```

## Mental Model

There are three different build paths that look similar but do different jobs.

1. Root Python package build

   This is the build path for `examples/live/hyperliquid/*.py`. It builds the
   legacy root package under `nautilus_trader/`, including Cython modules and
   the `nautilus_trader.core.nautilus_pyo3` extension.

2. PyO3-only root rebuild

   This rebuilds the Rust/PyO3 side and copies the updated
   `nautilus_pyo3` extension into `nautilus_trader/core/`, but skips Cython.
   Use it after a full build when you changed Rust only.

3. Direct Cargo build

   This is useful for checking Rust compile errors and running Rust tests. It
   does not copy Python extension files into `nautilus_trader/`, so it is not
   enough by itself for the root Python live examples.

The `python/` directory is the newer v2 package. Do not use the v2 build path
for these root examples unless the example has been moved under `python/`.

## One-Time Python Dependencies

Install the Python dependencies needed by `build.py` without installing the
local package through uv:

```bash
uv sync --group dev --no-install-package nautilus_trader
```

The broader Makefile equivalent is:

```bash
make install-deps
```

`make install-deps` installs all dependency groups and extras, so it is more
complete but less minimal.

## Full Build For Running Python

Run this when starting from a clean checkout, after deleting compiled `.so`
files, after changing Cython, or after seeing import errors such as
`No module named 'nautilus_trader.core.data'`.

Fast runtime, slowest compile:

```bash
BUILD_MODE=release uv run --no-sync python build.py
```

Faster compile, slower runtime:

```bash
BUILD_MODE=debug uv run --no-sync python build.py
```

Debug symbols for PyO3/Rust debugging:

```bash
BUILD_MODE=debug-pyo3 uv run --no-sync python build.py
```

Makefile equivalents:

```bash
make build
make build-debug
make build-debug-pyo3
```

## Rust Core Only Rebuild

Use this after a full build when the Cython modules already exist and you only
changed Rust code, such as Hyperliquid adapter execution, dispatch, HTTP, or
WebSocket logic.

Fast runtime:

```bash
BUILD_MODE=release PYO3_ONLY=true uv run --no-sync python build.py
```

Faster compile, slower runtime:

```bash
BUILD_MODE=debug PYO3_ONLY=true uv run --no-sync python build.py
```

PyO3 debug symbols:

```bash
BUILD_MODE=debug-pyo3 PYO3_ONLY=true uv run --no-sync python build.py
```

What this builds:

```text
nautilus-backtest
nautilus-common
nautilus-core
nautilus-model
nautilus-persistence
nautilus-pyo3
```

`nautilus-pyo3` depends on the Python-enabled adapter crates, including
`nautilus-hyperliquid`, so Hyperliquid Rust changes are included.

## Direct Cargo Build

Use this for compile checks. It is not enough to run the root Python examples
because it does not copy `nautilus_pyo3` into `nautilus_trader/core/` and does
not build Cython modules.

Debug compile check:

```bash
cargo build --lib \
  -p nautilus-backtest \
  -p nautilus-common \
  -p nautilus-core \
  -p nautilus-model \
  -p nautilus-persistence \
  -p nautilus-pyo3 \
  --no-default-features \
  --features arrow,cython-compat,extension-module,ffi,high-precision,postgres,python,tracing-bridge
```

Release compile check:

```bash
cargo build --lib \
  -p nautilus-backtest \
  -p nautilus-common \
  -p nautilus-core \
  -p nautilus-model \
  -p nautilus-persistence \
  -p nautilus-pyo3 \
  --release \
  --no-default-features \
  --features arrow,cython-compat,extension-module,ffi,high-precision,postgres,python,tracing-bridge
```

For isolated Hyperliquid Rust tests, prefer narrower commands that avoid
Python extension-module linking. For example:

```bash
cargo test -p nautilus-hyperliquid --test dispatch --no-default-features
```

Avoid using `extension-module` for Rust test binaries. That feature is for a
Python-loaded extension module and can cause runtime linker errors in ordinary
Rust test executables.

## Run After Build

Use `--no-sync` so uv does not decide to reinstall or rebuild while starting
the live process:

```bash
uv run --no-sync python examples/live/hyperliquid/hyperliquid_sweep_strategy.py \
  --config examples/live/hyperliquid/sweep_basket_config.example.json
```

If you intentionally want uv to refresh dependencies, do it as a separate
explicit step with `uv sync`.

## Smoke Checks

After a full build or PyO3-only rebuild:

```bash
uv run --no-sync python - <<'PY'
import nautilus_trader.core.data
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.adapters.hyperliquid import HYPERLIQUID

print("ok", HYPERLIQUID, nautilus_pyo3)
PY
```

If this fails on `nautilus_trader.core.data`, do a full build without
`PYO3_ONLY=true`.

If this fails on `nautilus_pyo3`, rebuild the Rust core through `build.py`
rather than direct Cargo.

## Build Mode Choice

Use `BUILD_MODE=debug` while iterating. It compiles faster and uses incremental
debug artifacts under `target/debug/`.

Use `BUILD_MODE=release` for realistic live performance. It uses the release
profile, which is intentionally slower to compile because it optimizes harder,
uses fat LTO, strips symbols, disables incremental compilation, and uses one
codegen unit.

Use `BUILD_MODE=debug-pyo3` when you need usable Rust debug symbols in the
Python extension.

## Precision

High precision is enabled by default. To build standard precision:

```bash
HIGH_PRECISION=false BUILD_MODE=debug uv run --no-sync python build.py
```

Changing precision changes Cargo features and can force a larger rebuild.

## Cache Hygiene

Stay consistent with:

```text
BUILD_MODE
HIGH_PRECISION
CARGO_TARGET_DIR
feature set
```

Changing any of those can invalidate a large part of the Cargo cache.

To keep this checkout's normal artifacts isolated from experiments:

```bash
CARGO_TARGET_DIR=target/sweep-debug BUILD_MODE=debug uv run --no-sync python build.py
```

For normal iteration, use the default `target/` so rebuilds stay warm.

## Common Failures

`ModuleNotFoundError: No module named 'nautilus_trader.core.data'`

The Cython modules have not been built or copied into the source tree. Run:

```bash
BUILD_MODE=debug uv run --no-sync python build.py
```

`symbol not found in flat namespace '_PyBaseObject_Type'`

You probably ran a Rust test binary with the `extension-module` feature. Use a
Rust-test feature set that excludes `extension-module`, such as:

```bash
cargo test -p nautilus-hyperliquid --test dispatch --no-default-features
```

`Blocking waiting for file lock on build directory`

Another Cargo build is already using the same `target/` directory. Either wait
for it, stop the other build if it is yours, or use a separate `CARGO_TARGET_DIR`
for the new build.

Plain `uv run python ...` rebuilds or changes the environment

Use:

```bash
uv run --no-sync python ...
```

and run `uv sync` explicitly only when you mean to change dependencies.

## Quick Decision Table

| Goal | Command |
| --- | --- |
| First usable local build | `BUILD_MODE=debug uv run --no-sync python build.py` |
| Fast runtime build | `BUILD_MODE=release uv run --no-sync python build.py` |
| Rust-only iteration after full build | `BUILD_MODE=debug PYO3_ONLY=true uv run --no-sync python build.py` |
| Rust-only optimized rebuild | `BUILD_MODE=release PYO3_ONLY=true uv run --no-sync python build.py` |
| Rust compile check only | direct `cargo build --lib ...` |
| Hyperliquid dispatch test | `cargo test -p nautilus-hyperliquid --test dispatch --no-default-features` |
