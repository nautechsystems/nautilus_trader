# Installation

NautilusTrader is officially supported for Python 3.12-3.14 on the following 64-bit platforms:

| Operating System | Supported Versions | CPU Architecture |
| ---------------- | ------------------ | ---------------- |
| Linux (Ubuntu)   | 22.04 and later    | x86_64           |
| Linux (Ubuntu)   | 22.04 and later    | ARM64            |
| macOS            | 15.0 and later     | ARM64            |
| Windows Server   | 2022 and later     | x86_64           |

:::note
NautilusTrader may work on other platforms, but only those listed above are regularly used by developers and tested in CI.
:::

NautilusTrader follows the
[Python support window in Scientific Python SPEC 0](https://scientific-python.org/specs/spec-0000/).
Each Python minor version is supported for three years after its initial release. Support normally
ends in the first NautilusTrader release after that window and after the replacement Python version
passes compatibility checks.

Continuous CI coverage comes from the GitHub Actions runners we build on:

- `Linux (Ubuntu)` builds currently pin to `ubuntu-22.04` to keep glibc 2.35 compatibility even as `ubuntu-latest` moves ahead.
- `macOS (ARM64)` builds run on `macos-latest`, so support tracks that runner image as it moves ahead.
- `Windows (x86_64)` builds currently pin to `windows-2022` to keep the toolchain stable.

On Linux, confirm your glibc version with `ldd --version` and ensure it reports 2.35 or newer before proceeding.

We recommend using the latest supported version of Python and installing [nautilus_trader](https://pypi.org/project/nautilus_trader/) inside a virtual environment to isolate dependencies.

**There are two supported ways to install**:

1. Pre-built binary wheel from PyPI *or* the Nautech Systems package index.
2. Build from source.

:::tip
We highly recommend installing using the [uv](https://docs.astral.sh/uv) package manager with a "vanilla" CPython.

Conda and other Python distributions *may* work but aren't officially supported.
:::

## From PyPI

:::warning[Install the 2.x wheel for these docs]
This documentation covers NautilusTrader 2.x. PyPI still resolves a plain
`uv pip install nautilus_trader` to the 1.x line, whose Python API differs, so the code on these
pages fails with `ImportError` and `TypeError` against a 1.x install. Pass `--pre` until `2.0.0`
is released, and confirm that
`python -c "import nautilus_trader; print(nautilus_trader.__version__)"` reports a `2.` version.
:::

NautilusTrader publishes 2.x release-candidate wheels to PyPI using `2.0.0rcN` versions while final
validation is in progress. To install the latest
[nautilus_trader](https://pypi.org/project/nautilus_trader/) binary wheel (or sdist package):

```bash
uv pip install --pre nautilus_trader
```

The `--pre` flag is required because these wheels are pre-release builds. The installed import name
is still `nautilus_trader`.

:::warning
We do not recommend release candidates for production environments, such as live trading
controlling real capital.
:::

Run this command outside a NautilusTrader source checkout. The repository root uses an
`exclude-newer` uv policy for reproducible development, which can filter out newly published
wheels. Inside a source checkout, use [Build Python from source](#8-build-python-from-source)
instead.

Current wheels target Python 3.12-3.14. Build from source when you need local Rust changes,
a debug build, or a platform wheel that is not available.

### Stable 1.x wheels

Omitting `--pre` installs the latest stable 1.x release:

```bash
uv pip install nautilus_trader
```

A 1.x install cannot run the examples on these pages. See
[Migrate from v1 to v2](https://github.com/nautechsystems/nautilus_trader/blob/develop/MIGRATION_V2.md)
for the API differences.

## Extras

Install the optional dependencies for Plotly-based interactive tearsheets and charts with the
`visualization` extra:

```bash
uv pip install --pre "nautilus_trader[visualization]"
```

## From the Nautech Systems package index

The Nautech Systems package index (`packages.nautechsystems.io`) complies with
[PEP-503](https://peps.python.org/pep-0503/) and hosts both stable and development binary wheels
for `nautilus_trader`.
This enables users to install either the latest stable release or pre-release versions for testing.

### Stable wheels

Stable wheels correspond to official releases of `nautilus_trader` on PyPI, and use standard
versioning. As on PyPI, the latest stable release is still on the 1.x line, so add `--pre` for a
2.x wheel.

To install the latest stable release:

```bash
uv pip install nautilus_trader --index-url=https://packages.nautechsystems.io/simple
```

:::tip
Use `--extra-index-url` instead of `--index-url` if you want uv to fall back to PyPI automatically.
:::

### Development wheels

The main package index publishes development wheels from both the `nightly` and `develop`
branches, allowing users to test features and fixes ahead of stable releases.

This process also helps preserve compute resources and provides easy access to the exact binaries tested in CI pipelines,
while adhering to [PEP-440](https://peps.python.org/pep-0440/) versioning standards:

- `develop` wheels use the version suffix `.devYYYYMMDD+run`.
- `nightly` wheels use `.devYYYYMMDD` when the base version is already a pre-release, and
  `aYYYYMMDD` otherwise.

| Platform           | Develop | Nightly |
| :----------------- | :------ | :------ |
| `Linux (x86_64)`   | ✓       | ✓       |
| `Linux (ARM64)`    | -       | ✓       |
| `macOS (ARM64)`    | -       | ✓       |
| `Windows (x86_64)` | -       | ✓       |

:::warning
We do not recommend using development wheels in production environments, such as live trading
controlling real capital.
:::

By default, uv will install the latest stable release. Adding the `--pre` flag ensures that pre-release versions, including development wheels, are considered.

To install the latest available pre-release (including development wheels):

```bash
uv pip install nautilus_trader --pre --index-url=https://packages.nautechsystems.io/simple
```

The installed import name is still `nautilus_trader`. Run this command outside a NautilusTrader
source checkout so the repository's `exclude-newer` uv policy does not filter out newly published
wheels. Build from source when you need local Rust changes, a debug build, or a platform wheel
that is not available.

### Available versions

You can view all available versions of `nautilus_trader` on the [package index](https://packages.nautechsystems.io/simple/nautilus-trader/index.html).

To programmatically request and list available versions:

```bash
curl -s https://packages.nautechsystems.io/simple/nautilus-trader/index.html | grep -oP '(?<=<a href=")[^"]+(?=")' | awk -F'#' '{print $1}' | sort
```

### Branch updates

- `develop` branch wheels (`.devYYYYMMDD+run`): Build and publish continuously with every merged commit.
- `nightly` branch wheels (`.devYYYYMMDD` or `aYYYYMMDD`): Build and publish daily when we
  automatically merge the `develop` branch at **14:00 UTC** (if there are changes).

### Retention policies

- `develop` branch wheels: We retain only the most recent wheel build.
- `nightly` branch wheels: We retain only the 30 most recent publication dates per platform.

### Verifying build provenance

All release artifacts published by the project carry cryptographic attestations
generated by the CI/CD pipeline:

- Python wheels and source distribution (PyPI, GitHub Releases, Nautech Systems package index): [SLSA](https://slsa.dev/) build provenance.
- Docker images (`ghcr.io/nautechsystems/nautilus_trader`, `ghcr.io/nautechsystems/jupyterlab`): keyless [cosign](https://github.com/sigstore/cosign) signatures plus SPDX SBOM attestations.

Both are issued via [Sigstore](https://www.sigstore.dev/) and bound to a specific
commit SHA, so verification ensures the artifact was produced by the official
NautilusTrader GitHub Actions workflow and has not been tampered with since.

For step-by-step verification commands, see [Verifying releases](https://github.com/nautechsystems/nautilus_trader/blob/develop/SECURITY.md#verifying-releases) in `SECURITY.md`.

:::note
Verification requires the [GitHub CLI](https://cli.github.com/) (`gh`) for Python artifacts
and [cosign](https://github.com/sigstore/cosign) for Docker images.
Development wheels from `develop` and `nightly` branches are also attested.
:::

## From source

It's possible to install from source using pip if you first install the build dependencies as specified in the `pyproject.toml`.

### 1. Install rustup

Install [rustup](https://rustup.rs/) (the Rust toolchain installer):

```bash tab="Linux/macOS"
curl https://sh.rustup.rs -sSf | sh
```

```powershell tab="Windows"
# Download and install rustup-init.exe from https://win.rustup.rs/x86_64
# Also install "Desktop development with C++" via Build Tools for Visual Studio 2022
```

Verify: `rustc --version`

### 2. Enable cargo

Enable `cargo` in the current shell:

```bash tab="Linux/macOS"
source $HOME/.cargo/env
```

```powershell tab="Windows"
# Start a new PowerShell session
```

### 3. Install clang

Install [clang](https://clang.llvm.org/) (a C language frontend for LLVM). On Linux this also installs [lld](https://lld.llvm.org/), which is configured as the Rust linker for faster builds:

```bash tab="Linux"
sudo apt-get install clang lld
```

```powershell tab="Windows"
# 1. Add Clang via Visual Studio Installer:
#    Modify > C++ Clang tools for Windows (latest) > Modify
# 2. Add to PATH:
[System.Environment]::SetEnvironmentVariable('path', "C:\Program Files\Microsoft Visual Studio\2022\BuildTools\VC\Tools\Llvm\x64\bin\;" + $env:Path,"User")
```

Verify: `clang --version`

### 4. Install uv

Install [uv](https://docs.astral.sh/uv/getting-started/installation):

```bash tab="Linux/macOS"
curl -LsSf https://astral.sh/uv/install.sh | sh
```

```powershell tab="Windows"
irm https://astral.sh/uv/install.ps1 | iex
```

### 5. Clone and sync dependencies

Clone the source with `git`, then sync its dependencies from the project root:

```bash
git clone --branch develop --depth 1 https://github.com/nautechsystems/nautilus_trader
cd nautilus_trader
make sync
```

For development hosts and CI runner images, see the
[single source of truth for versions](../developer_guide/environment_setup.md#single-source-of-truth-for-versions)
before installing pinned tools.

:::note
The `--depth 1` flag fetches just the latest commit for a faster, lightweight clone.
:::

### 6. Install Cap'n Proto for development

Install [Cap'n Proto](https://capnproto.org) if you plan to enable the `capnp` Rust feature,
regenerate serialization schemas, or work on serialization code. Use the repository script on
Linux or macOS to install the pinned version from `.nautilus-engineering/tools.toml`:

```bash
./scripts/install-capnp.sh
```

Verify: `capnp --version`

:::note
Cap'n Proto is a development dependency. It is not required when installing pre-built wheels.
:::

### 7. Set environment variables

The uv project environment lives at `python/.venv`, beside `python/pyproject.toml`. Run direct uv
project commands from `python/` or pass `--project python` from the repository root.

Set environment variables for PyO3 compilation (Linux and macOS only). Run these commands from
the repository root after `make sync`:

```bash
# Set the Python executable path for PyO3
export PYO3_PYTHON="$PWD/python/.venv/bin/python"

# Linux only: Set the library path for the uv-managed Python runtime
PYTHON_LIB_DIR="$("$PYO3_PYTHON" -c 'import sysconfig; print(sysconfig.get_config_var("LIBDIR"))')"
export LD_LIBRARY_PATH="$PYTHON_LIB_DIR${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"

# Required for Rust tests when using uv-installed Python
export PYTHONHOME="$("$PYO3_PYTHON" -c 'import sys; print(sys.base_prefix)')"
```

:::note
The `LD_LIBRARY_PATH` export is Linux-specific and not needed on macOS.

The `PYTHONHOME` variable is required when running `make cargo-test` with a `uv`-installed Python.
Without it, tests that depend on PyO3 may fail to locate the Python runtime.
:::

### 8. Build Python from source

This path builds the PyO3 package from the `python/` directory and installs it into `python/.venv`.
Use it from a NautilusTrader source checkout when a development wheel is not available for your
platform or when you need local Rust changes.

From the repository root:

```bash
make build-debug
```

This target syncs `python/.venv`, builds the Rust extension with maturin, and regenerates Python type
stubs. It uses `target/` for Cargo artifacts.

Run a Python example with the project environment:

```bash
uv run --project python --no-sync python examples/live/lighter/data_tester.py
```

The script connects to Lighter Testnet and starts streaming market data; stop it with Ctrl+C.

For direct commands and test targets, see the [Python package README][python-readme].

[python-readme]: https://github.com/nautechsystems/nautilus_trader/blob/develop/python/README.md

## From GitHub release

To install a binary wheel from GitHub, first navigate to the [latest release](https://github.com/nautechsystems/nautilus_trader/releases/latest).
Download the appropriate `.whl` for your operating system and Python version, then run:

```bash
uv pip install <file-name>.whl
```

## Troubleshooting

### Documentation examples fail to import

```text
ImportError: cannot import name 'OrderSide' from 'nautilus_trader.model'
ImportError: cannot import name 'BacktestEngine' from 'nautilus_trader.backtest'
TypeError: Struct types cannot define __init__
```

These come from running 2.x documentation against a 1.x install. Check what you have:

```bash
python -c "import nautilus_trader; print(nautilus_trader.__version__)"
```

A `1.` version means the resolver picked the stable line. Reinstall with `--pre`:

```bash
uv pip install -U --pre nautilus_trader
```

The 1.x and 2.x Python APIs are not interchangeable. See
[Migrate from v1 to v2](https://github.com/nautechsystems/nautilus_trader/blob/develop/MIGRATION_V2.md)
when porting a 1.x application.

### uv resolves an older version inside the repository

The repository root sets an `exclude-newer` policy for reproducible development, which hides
recently published wheels. Run install commands from another directory, or
[build from source](#from-source).

### Wheel not found for your platform

Check your Python version is 3.12-3.14 and your platform is listed at the top of this page. On
Linux, `ldd --version` must report glibc 2.35 or newer. Otherwise
[build from source](#from-source).

## Versioning and releases

NautilusTrader is still under active development. Some features may be incomplete, and while
the API is becoming more stable, breaking changes can occur between releases.
We strive to document these changes in the release notes on a **best-effort basis**.

We aim to follow a **bi-weekly release schedule**, though experimental or larger features may cause delays.

Use NautilusTrader only if you are prepared to adapt to these changes.

## Redis

Using [Redis](https://redis.io) with NautilusTrader is **optional** and only required if configured as the backend for a cache database or [message bus](../concepts/message_bus.md).

:::info
The minimum supported Redis version is 6.2 (required for [streams](https://redis.io/docs/latest/develop/data-types/streams/) functionality).
:::

For a quick setup, we recommend using a [Redis Docker container](https://hub.docker.com/_/redis/). You can find an example setup in the `.docker` directory,
or run the following command to start a container:

```bash
docker run -d --name redis -p 6379:6379 redis:latest
```

This command will:

- Pull the latest version of Redis from Docker Hub if it's not already downloaded.
- Run the container in detached mode (`-d`).
- Name the container `redis` for easy reference.
- Expose Redis on the default port 6379, making it accessible to NautilusTrader on your machine.

To manage the Redis container:

- Start it with `docker start redis`
- Stop it with `docker stop redis`

:::tip
We recommend using [Redis Insight](https://redis.io/insight/) as a GUI to visualize and debug Redis data efficiently.
:::

## Precision mode

NautilusTrader supports two precision modes for its core value types (`Price`, `Quantity`, `Money`),
which differ in their internal bit-width and maximum decimal precision.

- **High-precision**: 128-bit integers with up to 16 decimals of precision, and a larger value range.
- **Standard-precision**: 64-bit integers with up to 9 decimals of precision, and a smaller value range.

:::note
By default, the official Python wheels ship in high-precision (128-bit) mode on all supported platforms.

For pure Rust crates, high-precision works on all platforms (including Windows) since Rust handles
`i128`/`u128` via software emulation. The default is standard-precision unless you explicitly enable
the `high-precision` feature flag.
:::

The performance tradeoff is that standard-precision is ~3-5% faster in typical backtests,
but has lower decimal precision and a smaller representable value range.

:::note
Performance benchmarks comparing the modes are pending.
:::

### Build configuration

The precision mode is selected at compile time through the `high-precision` Rust feature flag.
The Python package enables this flag in the maturin build features (see `python/pyproject.toml`),
so source builds default to high-precision. For a standard-precision (64-bit) Python build,
remove `high-precision` from the maturin feature list, then build as usual:

```bash
make build-debug
```

### Rust feature flag

To enable high-precision (128-bit) mode in Rust, add the `high-precision` feature to your `Cargo.toml`:

```toml
[dependencies]
nautilus-core = { version = "*", features = ["high-precision"] }
```

:::info
See the [Value Types](../concepts/overview.md#value-types) specifications for more details.
:::
