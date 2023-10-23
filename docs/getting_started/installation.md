# Installation

NautilusTrader is tested and supported for Python 3.9-3.11 on the following 64-bit platforms:

| Operating System       | Supported Versions    | CPU Architecture  |
|------------------------|-----------------------|-------------------|
| Linux (Ubuntu)         | 20.04 or later        | x86_64            |
| macOS                  | 12 or later           | x86_64, ARM64     |
| Windows Server         | 2022 or later         | x86_64            |

```{tip}
We recommend running the platform with the latest supported stable version of Python, and in a virtual environment to isolate the dependencies.
```

## From PyPI
To install the latest binary wheel (or sdist package) from PyPI using Pythons _pip_ package manager:
    
    pip install -U nautilus_trader

## Extras

Install optional dependencies as 'extras' for specific integrations:

- `betfair`: Betfair adapter
- `docker`: Needed for Docker when using the IB gateway
- `ib`: Interactive Brokers adapter
- `redis`: Use Redis as a cache database

To install with specific extras using _pip_:

    pip install -U "nautilus_trader[docker,ib,redis]"

## From Source
Installation from source requires the `Python.h` header file, which is included in development releases such as `python-dev`. 
You'll also need the latest stable `rustc` and `cargo` to compile the Rust libraries.

For MacBook Pro M1/M2, make sure your Python installed using pyenv is configured with --enable-shared:

    PYTHON_CONFIGURE_OPTS="--enable-shared" pyenv install <python_version>

See https://pyo3.rs/latest/getting_started#virtualenvs.

It's possible to install from source using `pip` if you first install the build dependencies
as specified in the `pyproject.toml`. However, we highly recommend installing using [poetry](https://python-poetry.org/) as below.

1. Install [rustup](https://rustup.rs/) (the Rust toolchain installer):
   - Linux and macOS:
       ```bash
       curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
       ```
   - Windows:
       - Download and install [`rustup-init.exe`](https://win.rustup.rs/x86_64)
       - Install "Desktop development with C++" with [Build Tools for Visual Studio 2019](https://visualstudio.microsoft.com/thank-you-downloading-visual-studio/?sku=BuildTools&rel=16)

2. Enable `cargo` in the current shell:
   - Linux and macOS:
       ```bash
       source $HOME/.cargo/env
       ```
   - Windows:
     - Start a new PowerShell

3. Install poetry (or follow the installation guide on their site):

       curl -sSL https://install.python-poetry.org | python3 -

4. Clone the source with `git`, and install from the projects root directory:

       git clone https://github.com/nautechsystems/nautilus_trader
       cd nautilus_trader
       poetry install --only main --all-extras

## From GitHub Release
To install a binary wheel from GitHub, first navigate to the [latest release](https://github.com/nautechsystems/nautilus_trader/releases/latest).
Download the appropriate `.whl` for your operating system and Python version, then run:

    pip install <file-name>.whl
    