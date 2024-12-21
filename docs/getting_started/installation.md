# Installation

NautilusTrader is tested and officially supported for Python 3.11 and 3.12 for the following 64-bit platforms:

| Operating System       | Supported Versions    | CPU Architecture  |
|------------------------|-----------------------|-------------------|
| Linux (Ubuntu)         | 22.04 and later       | x86_64            |
| macOS                  | 14.7 and later        | ARM64             |
| Windows Server         | 2022 and later        | x86_64            |

:::note
NautilusTrader may work on other platforms, but only those listed above are regularly used by developers and tested in CI.
:::

:::tip
We recommend using the latest supported version of Python and setting up [nautilus_trader](https://pypi.org/project/nautilus_trader/) in a virtual environment to isolate dependencies.
:::

## From PyPI

To install the latest [nautilus_trader]([nautilus_trader](https://pypi.org/project/nautilus_trader/)) binary wheel (or sdist package) from PyPI using Pythons pip package manager:

    pip install -U nautilus_trader

## Extras

Install optional dependencies as 'extras' for specific integrations:

- `betfair`: Betfair adapter (integration) dependencies.
- `docker`: Needed for Docker when using the IB gateway (with the Interactive Brokers adapter).
- `dydx`: dYdX adapter (integration) dependencies.
- `ib`: Interactive Brokers adapter (integration) dependencies.
- `polymarket`: Polymarket adapter (integration) dependencies.

To install with specific extras using pip:

    pip install -U "nautilus_trader[docker,ib]"

## From Source

Installation from source requires the `Python.h` header file, which is included in development releases such as `python-dev`.
You'll also need the latest stable `rustc` and `cargo` to compile the Rust libraries.

For MacBook Pro M1/M2, make sure your Python installed using pyenv is configured with --enable-shared:

    PYTHON_CONFIGURE_OPTS="--enable-shared" pyenv install <python_version>

See the [PyO3 user guide](https://pyo3.rs/latest/getting-started#virtualenvs) for more details.

It's possible to install from source using pip if you first install the build dependencies
as specified in the `pyproject.toml`. We highly recommend installing using [poetry](https://python-poetry.org/) as below.

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

3. Install [clang](https://clang.llvm.org/) (a C language frontend for LLVM):
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
       from a terminal session run: `clang --version`

4. Install poetry (or follow the installation guide on their site):

       curl -sSL https://install.python-poetry.org | python3 -

5. Clone the source with `git`, and install from the projects root directory:

       git clone https://github.com/nautechsystems/nautilus_trader
       cd nautilus_trader
       poetry install --only main --all-extras

## From GitHub Release

To install a binary wheel from GitHub, first navigate to the [latest release](https://github.com/nautechsystems/nautilus_trader/releases/latest).
Download the appropriate `.whl` for your operating system and Python version, then run:

    pip install <file-name>.whl

## Nightly wheels

Nightly binary wheels for `nautilus_trader` are built and published daily from the `nightly` branch
using the version format `dev.{date}` (e.g., `1.208.0.dev20241212` for December 12, 2024).
These wheels allow testing of the latest features and fixes that have not yet been included in a stable PyPI release.

**To install the latest nightly build**:

    pip install -U --index-url=https://nautechsystems.github.io/nautilus_trader/simple/ nautilus_trader

**To install a specific nightly build** (e.g., `1.208.0.dev20241212` for December 12, 2024):

    pip install -U --index-url=https://nautechsystems.github.io/nautilus_trader/simple/ nautilus_trader==1.208.0.dev20241212

**Notes**:
- The `develop` branch is merged into `nightly` daily at **14:00 UTC** (if there are changes).
- A 3-day lookback window is maintained, retaining only the last 3 nightly builds.

## Versioning and releases

NautilusTrader is still under active development. Some features may be incomplete, and while
the API is becoming more stable, breaking changes can occur between releases.
We strive to document these changes in the release notes on a **best-effort basis**.

We aim to follow a **weekly release schedule**, though experimental or larger features may cause delays.

Use NautilusTrader only if you are prepared to adapt to these changes.

## Redis

Using Redis with NautilusTrader is **optional** and only required if configured as the backend for a cache database or [message bus](../concepts/message_bus.md).

:::info
The minimum supported Redis version is 6.2 or higher (required for [streams](https://redis.io/docs/latest/develop/data-types/streams/) functionality).
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
