# NautilusTrader v2

> [!WARNING]
>
> **Under active development and not yet considered usable.**

This directory contains the `nautilus_trader` v2 Python package, built entirely with PyO3 bindings.
This replaces the legacy Cython layer with Rust core bindings.

**Rules during the transition:**

- The `python/` directory is self-contained. Everything Python-related for v2 lives here.
- This directory will remain when v1 is removed (the top-level `nautilus_trader/` goes away).
- Nothing outside this directory should reference anything inside it for now.

## Project structure

```
python/
├── README.md                   # This file
├── generate_docstrings.py      # Copies Rust doc comments to PyO3 wrappers
├── generate_stubs.py           # Generates Python type stubs from Rust bindings
├── pyproject.toml              # Maturin build configuration
├── uv.lock                     # Dependency lock file
└── nautilus_trader/
    ├── __init__.py             # Re-exports from _libnautilus
    ├── _libnautilus.so         # Single compiled Rust extension (created by the build)
    ├── core/
    │   ├── __init__.py         # Re-exports from _libnautilus.core
    │   └── __init__.pyi        # Type stubs (auto-generated)
    ├── model/
    │   ├── __init__.py         # Re-exports from _libnautilus.model
    │   └── __init__.pyi        # Type stubs (auto-generated)
    └── ...                     # Other submodules follow the same pattern
```

## Build targets

> [!NOTE]
> The v2 build uses `target-v2/` for Cargo artifacts to avoid conflicts with
> the v1 build in `target/`. This separation is temporary until the full
> transition to v2.

From the repository root:

```bash
make build-debug-v2   # Compile and install into python/.venv (debug mode)
make py-stubs-v2      # Regenerate type stubs and docstrings
```

## Development setup

### Prerequisites

- Rust toolchain (via `rustup`)
- Python 3.12-3.14
- `patchelf` (Linux only) for setting rpath on the compiled extension

### Quick start

From within this `python/` directory:

```bash
uv run maturin develop --extras dev,test
```

This compiles the Rust extension and installs it into the project venv (`python/.venv`).
Run again after Rust changes to recompile.

## How it works

1. **Build**: `maturin develop` compiles all Rust code into `nautilus_trader/_libnautilus.so`.
2. **Re-exports**: Each submodule's `__init__.py` re-exports components from `_libnautilus`.
3. **Type stubs**: `.pyi` files provide type information for IDEs and `mypy`.
4. **Docstrings**: `generate_docstrings.py` copies `///` doc comments from the Rust source
   to PyO3 wrappers, so `__doc__` stays in sync without manual duplication.

## Usage

```python
from nautilus_trader.core import UUID4

UUID4()
```

## Installation

### From source

```bash
git clone https://github.com/nautechsystems/nautilus_trader.git
cd nautilus_trader/python
uv run maturin develop --extras dev,test
```

### Development wheels (pre-release)

Every successful build from the `develop` or `nightly` branches publishes a wheel to the
private v2 index.

```bash
pip install --index-url https://packages.nautechsystems.io/v2/simple/ --pre nautilus-trader
```

| Platform         | Python  | Develop | Nightly |
| :--------------- | :------ | :------ | :------ |
| `Linux (x86_64)` | 3.12-14 | ✓       | ✓       |
| `macOS (ARM64)`  | 3.12-14 | ✓       | ✓       |

The `--pre` flag is required because wheels are tagged as development releases.

## Python type stubs

Type stubs (`.pyi` files) are auto-generated using
[`pyo3-stub-gen`](https://github.com/Jij-Inc/pyo3-stub-gen). To regenerate after modifying
Rust bindings:

```bash
python generate_stubs.py
```

This first runs `generate_docstrings.py` to copy doc comments from the underlying Rust source
to PyO3 wrapper functions, then generates the `.pyi` stub files.

> [!NOTE]
> Stub generation is a work in progress and may not cover all exported types yet.
