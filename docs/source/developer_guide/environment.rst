Environment
===========

For development we recommend using the PyCharm `Professional` edition IDE, as it
interprets Cython syntax. Alternatively, you could use Visual Studio Code with
a Cython extension.

``pyenv`` is the recommended tool for handling Python installations and virtual environments.

> https://github.com/pyenv/pyenv

``poetry`` is the preferred tool for handling all Python package and dev dependencies.

> https://python-poetry.org/

``pre-commit`` is used to automatically run various checks, auto-formatters and linting tools
at commit.

> https://pre-commit.com/

Setup
-----
The following steps are for UNIX-like systems, and only need to be completed once.

1. Install ``poetry`` by running::

        curl -sSL https://raw.githubusercontent.com/python-poetry/poetry/master/get-poetry.py | python -

2. Then install all Python package dependencies, and compile the C extensions by running::

        poetry install

3. Install the ``pre-commit`` package by running::

        pip install pre-commit

4. Setup the ``pre-commit`` hook which will then run automatically at commit by running::

        pre-commit install

Builds
------

Following any changes to ``.pyx`` or ``.pxd`` files, you can re-compile by running::

    poetry run python build.py
