# NautilusTrader v2

Welcome to the next generation of NautilusTrader!

> [!WARNING]
>
> **Under active development and not yet considered usable.**

This directory contains the pure Python package for `nautilus_trader` v2, which is built entirely
with PyO3 bindings.

This approach removes the legacy Cython layer, providing a cleaner architecture, direct integration
with the Rust core, and a more streamlined development experience.

**To reduce confusion we are following some simple rules:**

- The `python/` directory is self contained, everything Python related for v2 will be under here.
- This directory will remain when we transition completely away from v1 (the top level `nautilus_trader/` will be removed).
- Nothing outside of this directory should refer to anything within this directory for now.

## Project structure

The v2 package is structured to provide a clean separation between the public Python API and the internal compiled core.
This enables first-class IDE/editor support through type stubs and allows for a mix of pure Python and high-performance Rust modules.

```
python/
├── README.md                   # This file
├── generate_stubs.py           # Automatic generation of Python type stubs
├── pyproject.toml              # Maturin-based build configuration for the v2 package
├── uv.lock                     # UV lock file for the v2 package
└── nautilus_trader/
    ├── __init__.py             # Main package entry point, re-exports from _libnautilus
    ├── _libnautilus.so         # The *single* compiled Rust extension (created by the build)
    ├── core/
    │   ├── __init__.py         # Re-exports from `_libnautilus.core`
    │   └── __init__.pyi        # Type stubs for the core module (WIP)
    ├── model/
    │   ├── __init__.py         # Re-exports from `_libnautilus.model`
    │   └── __init__.pyi        # Type stubs for the model module (WIP)
    └── ...                     # Other submodules follow the same pattern
```

## Development setup

All commands should be run from within this `python/` directory.

### Prerequisites

- Rust toolchain (via `rustup`).
- Python 3.11-3.13
- A virtual environment activated at the project root (e.g., `.venv`).
- `patchelf` (Linux only) - required for setting rpath on the compiled extension. Install with `uv pip install patchelf`.

### Quick start

To set up your development environment, run the following command. It will compile the Rust extension, install it in "editable" mode, and install all necessary development and test dependencies.

```bash
uv run --active maturin develop --extras dev,test
```

This is the only command you need to get started. If you make changes to the Rust code, simply run it again to recompile.

## How it works

The `nautilus_trader` Python package acts as a "facade" over the compiled Rust core.

1. **The build**: `maturin develop` compiles all the Rust code into a single native library, `nautilus_trader/_libnautilus.so`.
2. **The facade**: The `nautilus_trader/__init__.py` file imports all the functionality from the `_libnautilus.so` file.
3. **The submodules**: Each subdirectory (e.g., `nautilus_trader/model/`) uses its `__init__.py` to re-export the relevant components from `_libnautilus`, creating a clean, organized public API.
4. **Type hints**: The `.pyi` stub files provide full type information to your IDE and tools like `mypy`, giving you autocompletion and static analysis, even for the compiled Rust code.

## Usage

Once the package is installed, you can import and use it like any other Python library. The underlying Rust implementation is completely transparent.

```python
from nautilus_trader.core import UUID4

# Use the bindings directly
UUID4()
# ...
```

## Installation

### From source

To build and install from source, you need Rust and Python 3.11+ installed. You can use either uv or Poetry:

**Using uv:**

```bash
# Clone the repository
git clone https://github.com/nautechsystems/nautilus_trader.git
cd nautilus_trader/python

# Install dependencies and build
uv run --active maturin develop --extras dev,test
```

### Development wheels (pre-release)

While v2 is still under heavy development, every successful build from the `develop` or `nightly` branches publishes a wheel to our private package v2 index.

```bash
pip install --index-url https://packages.nautechsystems.io/v2/simple/ --pre nautilus-trader
```

| Platform         | Python  | Develop | Nightly |
| :--------------- | :------ | :------ | :------ |
| `Linux (x86_64)` | 3.12-13 | ✓       | ✓       |
| `macOS (ARM64)`  | 3.12-13 | ✓       | ✓       |

The `--pre` flag is required because wheels are tagged as development releases (e.g., `0.2.0.dev20250601`).

## Python type stubs

Type stubs (`.pyi` files) provide full type information for the compiled Rust extension, enabling IDE autocompletion and static type checking with tools like `mypy`.

Stubs are automatically generated using [`pyo3-stub-gen`](https://github.com/PyO3/pyo3-stub-gen). To regenerate them after modifying Rust bindings:

```bash
python generate_stubs.py
```

> [!NOTE]
> Automatic stub generation is a work in progress and may not cover all exported types yet.
