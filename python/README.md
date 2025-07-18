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

## Project Structure

The v2 package is structured to provide a clean separation between the public Python API and the internal compiled core.
This enables first-class IDE/editor support through type stubs and allows for a mix of pure Python and high-performance Rust modules.

```
python/
├── pyproject.toml              # Maturin-based build configuration for the v2 package
├── README.md                   # This file
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

## Development Setup

All commands should be run from within this `python/` directory.

### Prerequisites

- A virtual environment activated at the project root (e.g., `.venv`).
- Python 3.11+
- The Rust toolchain (via `rustup`).

### Installation

To set up your development environment, run the following command. It will compile the Rust extension, install it in "editable" mode, and install all necessary development and test dependencies.

```bash
maturin develop --extras dev,test
```

This is the only command you need to get started. If you make changes to the Rust code, simply run it again to recompile.

## How It Works

The `nautilus_trader` Python package acts as a "facade" over the compiled Rust core.

1.  **The Build**: `maturin develop` compiles all the Rust code into a single native library, `nautilus_trader/_libnautilus.so`.
2.  **The Facade**: The `nautilus_trader/__init__.py` file imports all the functionality from the `_libnautilus.so` file.
3.  **The Submodules**: Each subdirectory (e.g., `nautilus_trader/model/`) uses its `__init__.py` to re-export the relevant components from `_libnautilus`, creating a clean, organized public API.
4.  **Type Hinting**: The `.pyi` stub files provide full type information to your IDE and tools like `mypy`, giving you autocompletion and static analysis, even for the compiled Rust code.

## Usage

Once the package is installed, you can import and use it like any other Python library. The underlying Rust implementation is completely transparent.

```python
from nautilus_trader.core import UUID4

# Use the bindings directly
UUID4()
# ...
```

## Python type stubs

Automatic generation of type stubs are a high-priority work in progress...
