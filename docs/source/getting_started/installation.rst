Installation
============

The ``master`` branch will always reflect the code of the latest release version.

The package is tested against Python 3.7 - 3.9 on 64-bit Windows, macOS and Linux.
We recommend running the platform with the latest stable version of Python, and
in a virtual environment to isolate the dependencies.

For UNIX machines, [pyenv](https://github.com/pyenv/pyenv) is the recommended tool for handling system wide
Python installations and virtual environments.

Installation can be achieved through _one_ of the following options;

> https://github.com/pyenv/pyenv

Installation for UNIX-like systems can be achieved through `one` of the
following options;

From PyPI
---------

To install the latest binary wheel (or sdist package) from PyPI, run::

    pip install -U nautilus_trader

From GitHub Release
-------------------

To install a binary wheel from GitHub, first navigate to the latest release.

> https://github.com/nautechsystems/nautilus_trader/releases/latest/

Download the appropriate ``.whl`` for your operating system and Python version, then run::

    pip install <file-name>.whl

From Source
-----------

Installation from source requires Cython to compile the Python C extensions.

1. To install Cython, run::

        pip install -U Cython==3.0.0a8

2. Then to install NautilusTrader using ``pip``, run::

        pip install -U git+https://github.com/nautechsystems/nautilus_trader

**Or** clone the source with ``git``, and install from the projects root directory by running::

        git clone https://github.com/nautechsystems/nautilus_trader
        cd nautilus_trader
        pip install .

Extras
------

Also, the following optional dependency 'extras' are separately available for installation.
- `betfair` for the Betfair adapter.
- `ccxt` for the CCXT Pro adapter.
- `docs` for building the documentation.
- `ib` for the Interactive Brokers adapter.

For example, to install including the `ccxt` extra using pip:

    pip install nautilus_trader[ccxt]
