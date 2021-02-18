Installation
============

The ``master`` branch will always reflect the code of the latest release version.
Also, the documentation is always current for the latest version.

The package is tested against Python versions 3.7 - 3.9 on both Linux and
MacOS. Users are encouraged to use the latest stable version of Python.

It is a goal for the project to keep dependencies focused, however there are
still a large number of dependencies as found in the ``pyproject.toml`` file.
Therefore we recommend you create a new virtual environment for NautilusTrader.

There are various ways of achieving this - the easiest being to use the ``poetry``
tool. https://python-poetry.org/docs/

If you're not used to working with virtual environments, you will find a great
explanation in the ``poetry`` documentation under the `Managing environments`
sub-menu.

Installation for Unix-like systems can be achieved through `one` of the following options;

From PyPI
---------

To install the latest binary wheel (or sdist package) from PyPI, run:

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

        pip install -U Cython==3.0a6

2. Then to install NautilusTrader using ``pip``, run::

        pip install -U git+https://github.com/nautechsystems/nautilus_trader

**Or** clone the source with ``git``, and install from the projects root directory by running::

        git clone https://github.com/nautechsystems/nautilus_trader
        cd nautilus_trader
        pip install .
