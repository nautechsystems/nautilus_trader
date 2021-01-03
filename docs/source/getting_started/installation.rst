Installation
============

The package is tested against Python versions 3.7 - 3.9 on both Linux and
MacOS. Users are encouraged to use the latest stable version of Python.

It is a goal for the project to keep dependencies focused, however there are
still a large number of dependencies as found in the `pyproject.toml` file. Therefore we recommend you create a new
virtual environment for `NautilusTrader`.

There are various ways of achieving this - the easiest being to use the `Poetry`
tool. https://python-poetry.org/docs/

If you're not used to working with virtual environments, you will find a great
explanation in the `Poetry` documentation under the `Managing environments`
sub-menu.

The latest version of `NautilusTrader` can be downloaded
as a binary wheel from `PyPI`, just run::

   pip install -U nautilus_trader


Alternatively, you can install from source via pip by running::

    pip install .

The master branch will always reflect the code of the latest release version.
Also, the documentation found here on `readthedocs` is always current for the
latest version.
