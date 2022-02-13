# Installation

The package is tested against Python 3.8, 3.9 and 3.10 on 64-bit Windows, macOS and Linux. 
We recommend running the platform with the latest stable version of Python, and 
in a virtual environment to isolate the dependencies.

## From PyPI
To install the latest binary wheel (or sdist package) from PyPI:
    
    pip install -U nautilus_trader

## Extras

Also, the following optional dependency ‘extras’ are separately available for installation.

- `distributed` - packages for using Dask distributed in backtests.
- `ib`  - packages required for the Interactive Brokers adapter.

For example, to install including the `distributed` extras using pip:

    pip install -U nautilus_trader[distributed]

## From Source
It's possible to install from sourcing using `pip` if you first install the build dependencies
as specified in the `pyproject.toml`. However, we highly recommend installing using [poetry](https://python-poetry.org/).

1. First install poetry (or follow the installation guide on their site):

        curl -sSL https://raw.githubusercontent.com/python-poetry/poetry/master/get-poetry.py | python -

2. Clone the source with `git`, and install from the projects root directory:

       git clone https://github.com/nautechsystems/nautilus_trader
       cd nautilus_trader
       poetry install --no-dev

```{note}
Because of `jupyter-book`, the project requires a large number of development dependencies (which is the
reason for passing the `--no-dev` option above). If you'll be running tests, or developing with the codebase
in general then remove that option flag when installing. It's also possible to simply run `make` from the
top-level directory.
```

## From GitHub Release
To install a binary wheel from GitHub, first navigate to the [latest release](https://github.com/nautechsystems/nautilus_trader/releases/latest).
Download the appropriate `.whl` for your operating system and Python version, then run:

    pip install <file-name>.whl
