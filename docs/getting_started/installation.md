# Installation

NautilusTrader is officially supported for Python 3.11-3.13* on the following 64-bit platforms:

| Operating System       | Supported Versions    | CPU Architecture  |
|------------------------|-----------------------|-------------------|
| Linux (Ubuntu)         | 22.04 and later       | x86_64            |
| Linux (Ubuntu)         | 22.04 and later       | ARM64             |
| macOS                  | 14.7 and later        | ARM64             |
| Windows Server         | 2022 and later        | x86_64            |

\* Windows builds are currently pinned to CPython 3.13.2 because later
3.13.x Windows binaries were produced with the per-interpreter GIL enabled
but without exporting a handful of private C-API symbols. These missing
exports break linking of our Cython extensions. The pin can be removed
if/when an upstream CPython release restores the exports.

:::note
NautilusTrader may work on other platforms, but only those listed above are regularly used by developers and tested in CI.
:::

We recommend using the latest supported version of Python and installing [nautilus_trader](https://pypi.org/project/nautilus_trader/) inside a virtual environment to isolate dependencies.

**There are two supported ways to install**:

1. Pre-built binary wheel from PyPI *or* the Nautech Systems package index.
2. Build from source.

:::tip
We highly recommend installing using the [uv](https://docs.astral.sh/uv) package manager with a "vanilla" CPython.

Conda and other Python distributions *may* work but aren’t officially supported.
:::

## From PyPI

To install the latest [nautilus_trader](https://pypi.org/project/nautilus_trader/) binary wheel (or sdist package) from PyPI using Pythons pip package manager:

```bash
pip install -U nautilus_trader
```

## Extras

Install optional dependencies as 'extras' for specific integrations:

- `betfair`: Betfair adapter (integration) dependencies.
- `docker`: Needed for Docker when using the IB gateway (with the Interactive Brokers adapter).
- `dydx`: dYdX adapter (integration) dependencies.
- `ib`: Interactive Brokers adapter (integration) dependencies.
- `polymarket`: Polymarket adapter (integration) dependencies.

To install with specific extras using pip:

```bash
pip install -U "nautilus_trader[docker,ib]"
```

## From the Nautech Systems package index

The Nautech Systems package index (`packages.nautechsystems.io`) complies with [PEP-503](https://peps.python.org/pep-0503/) and hosts both stable and development binary wheels for `nautilus_trader`.
This enables users to install either the latest stable release or pre-release versions for testing.

### Stable wheels

Stable wheels correspond to official releases of `nautilus_trader` on PyPI, and use standard versioning.

To install the latest stable release:

```bash
pip install -U nautilus_trader --index-url=https://packages.nautechsystems.io/simple
```

### Development wheels

Development wheels are published from both the `nightly` and `develop` branches,
allowing users to test features and fixes ahead of stable releases.

**Note**: Wheels from the `develop` branch are only built for the Linux x86_64 platform to save time
and compute resources, while `nightly` wheels support additional platforms as shown below.

| Platform           | Nightly | Develop |
| :----------------- | :------ | :------ |
| `Linux (x86_64)`   | ✓       | ✓       |
| `Linux (ARM64)`    | ✓       | -       |
| `macOS (ARM64)`    | ✓       | -       |
| `Windows (x86_64)` | ✓       | -       |

This process also helps preserve compute resources and ensures easy access to the exact binaries tested in CI pipelines,
while adhering to [PEP-440](https://peps.python.org/pep-0440/) versioning standards:

- `develop` wheels use the version format `dev{date}+{build_number}` (e.g., `1.208.0.dev20241212+7001`).
- `nightly` wheels use the version format `a{date}` (alpha) (e.g., `1.208.0a20241212`).

:::warning
We do not recommend using development wheels in production environments, such as live trading controlling real capital.
:::

### Installation commands

By default, pip will install the latest stable release. Adding the `--pre` flag ensures that pre-release versions, including development wheels, are considered.

To install the latest available pre-release (including development wheels):

```bash
pip install -U nautilus_trader --pre --index-url=https://packages.nautechsystems.io/simple
```

To install a specific development wheel (e.g., `1.208.0a20241212` for December 12, 2024):

```bash
pip install nautilus_trader==1.208.0a20241212 --index-url=https://packages.nautechsystems.io/simple
```

### Available versions

You can view all available versions of `nautilus_trader` on the [package index](https://packages.nautechsystems.io/simple/nautilus-trader/index.html).

To programmatically request and list available versions:

```bash
curl -s https://packages.nautechsystems.io/simple/nautilus-trader/index.html | grep -oP '(?<=<a href="))[^"]+(?=")' | awk -F'#' '{print $1}' | sort
```

### Branch updates

- `develop` branch wheels (`.dev`): Build and publish continuously with every merged commit.
- `nightly` branch wheels (`a`): Build and publish daily when we automatically merge the `develop` branch at **14:00 UTC** (if there are changes).

### Retention policies

- `develop` branch wheels (`.dev`): We retain only the most recent wheel build.
- `nightly` branch wheels (`a`): We retain only the 10 most recent wheel builds.

## From source

It's possible to install from source using pip if you first install the build dependencies as specified in the `pyproject.toml`.

1. Install [rustup](https://rustup.rs/) (the Rust toolchain installer):
   - Linux and macOS:

```bash
curl https://sh.rustup.rs -sSf | sh
```

- Windows:
  - Download and install [`rustup-init.exe`](https://win.rustup.rs/x86_64)
  - Install "Desktop development with C++" with [Build Tools for Visual Studio 2019](https://visualstudio.microsoft.com/thank-you-downloading-visual-studio/?sku=BuildTools&rel=16)
- Verify (any system):
    from a terminal session run: `rustc --version`

2. Enable `cargo` in the current shell:
   - Linux and macOS:

```bash
source $HOME/.cargo/env
```

- Windows:
  - Start a new PowerShell

     1. Install [clang](https://clang.llvm.org/) (a C language frontend for LLVM):
- Linux:

```bash
sudo apt-get install clang
```

- Windows:

1. Add Clang to your [Build Tools for Visual Studio 2019](https://visualstudio.microsoft.com/thank-you-downloading-visual-studio/?sku=BuildTools&rel=16):

- Start | Visual Studio Installer | Modify | C++ Clang tools for Windows (12.0.0 - x64…) = checked | Modify

2. Enable `clang` in the current shell:

```powershell
[System.Environment]::SetEnvironmentVariable('path', "C:\Program Files (x86)\Microsoft Visual Studio\2019\BuildTools\VC\Tools\Llvm\x64\bin\;" + $env:Path,"User")
```

- Verify (any system):
  from a terminal session run:

```bash
clang --version
```

3. Install uv (see the [uv installation guide](https://docs.astral.sh/uv/getting-started/installation) for more details):

```bash
curl -LsSf https://astral.sh/uv/install.sh | sh
```

4. Clone the source with `git`, and install from the project's root directory:

```bash
git clone --branch develop --depth 1 https://github.com/nautechsystems/nautilus_trader
cd nautilus_trader
uv sync --all-extras
```

:::note
The `--depth 1` flag fetches just the latest commit for a faster, lightweight clone.
:::

5. Set environment variables for PyO3 compilation (Linux and macOS only):

```bash
# Set the library path for the Python interpreter (in this case Python 3.13.4)
export LD_LIBRARY_PATH="$HOME/.local/share/uv/python/cpython-3.13.4-linux-x86_64-gnu/lib:$LD_LIBRARY_PATH"

# Set the Python executable path for PyO3
export PYO3_PYTHON=$(pwd)/.venv/bin/python
```

:::note
Adjust the Python version and architecture in the `LD_LIBRARY_PATH` to match your system.
Use `uv python list` to find the exact path for your Python installation.
:::

## From GitHub release

To install a binary wheel from GitHub, first navigate to the [latest release](https://github.com/nautechsystems/nautilus_trader/releases/latest).
Download the appropriate `.whl` for your operating system and Python version, then run:

```bash
pip install <file-name>.whl
```

## Versioning and releases

NautilusTrader is still under active development. Some features may be incomplete, and while
the API is becoming more stable, breaking changes can occur between releases.
We strive to document these changes in the release notes on a **best-effort basis**.

We aim to follow a **weekly release schedule**, though experimental or larger features may cause delays.

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
By default, the official Python wheels **ship** in high-precision (128-bit) mode on Linux and macOS.
On Windows, only standard-precision (64-bit) is available due to the lack of native 128-bit integer support.

For the Rust crates, the default is standard-precision unless you explicitly enable the `high-precision` feature flag.
:::

The performance tradeoff is that standard-precision is ~3–5% faster in typical backtests,
but has lower decimal precision and a smaller representable value range.

:::note
Performance benchmarks comparing the modes are pending.
:::

### Build configuration

The precision mode is determined by:

- Setting the `HIGH_PRECISION` environment variable during compilation, **and/or**
- Enabling the `high-precision` Rust feature flag explicitly.

#### High-precision mode (128-bit)

```bash
export HIGH_PRECISION=true
make install-debug
```

#### Standard-precision mode (64-bit)

```bash
export HIGH_PRECISION=false
make install-debug
```

### Rust feature flag

To enable high-precision (128-bit) mode in Rust, add the `high-precision` feature to your `Cargo.toml`:

```toml
[dependencies]
nautilus_core = { version = "*", features = ["high-precision"] }
```

:::info
See the [Value Types](../concepts/overview.md#value-types) specifications for more details.
:::
