# Installation

The package is tested against Python 3.9 and 3.10 on 64-bit Linux, macOS and Windows. 
We recommend running the platform with the latest stable version of Python, and 
in a virtual environment to isolate the dependencies.

To install `numpy` and `scipy` on ARM architectures such as MacBook Pro M1 / Apple Silicon, [this stackoverflow thread](https://stackoverflow.com/questions/65745683/how-to-install-scipy-on-apple-silicon-arm-m1)
is useful.

## From PyPI
To install the latest binary wheel (or sdist package) from PyPI:
    
    pip install -U nautilus_trader

## Extras

Also, the following optional dependency ‘extras’ are separately available for installation.

- `hyperopt` - package required for model hyperparameter optimization in backtests
- `ib`  - package required for the Interactive Brokers adapter
- `redis`  - packages required to use Redis as a cache database

For example, to install including the `ib` and `redis` extras using pip:

    pip install -U nautilus_trader[ib,redis]

## From Source
Installation from source requires the `Python.h` header file, which is included in development releases such as `python-dev`. 
You'll also need the latest stable `rustc` and `cargo` to compile the Rust libraries.

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
       poetry install --no-dev

## From GitHub Release
To install a binary wheel from GitHub, first navigate to the [latest release](https://github.com/nautechsystems/nautilus_trader/releases/latest).
Download the appropriate `.whl` for your operating system and Python version, then run:

    pip install <file-name>.whl
