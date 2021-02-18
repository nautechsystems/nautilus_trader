Environment
===========

For development we recommend using the PyCharm `Professional` edition IDE, as it
interprets Cython syntax. Alternatively, you could use Visual Studio Code with
a Cython extension.

``poetry`` is the preferred tool for handling all Python package and dev dependencies.

> https://python-poetry.org/

``pre-commit`` is used to automatically run various checks, auto-formatters and linting tools
at commit.

> https://pre-commit.com/

Setup
-----
The following steps are for Unix-like systems, and only need to be completed once.

1. Install ``pre-commit`` by running::

        pip install pre-commit

2. Install the Cython package by running:

        pip install -U Cython==3.0a6

3. Install ``poetry`` by running::

        curl -sSL https://raw.githubusercontent.com/python-poetry/poetry/master/get-poetry.py | python -

4. Then install all Python package dependencies, and compile the C extensions by running::

        poetry install

5. Setup the ``pre-commit`` hook which will then run automatically at commit by running::

        pre-commit run --all-files

Builds
------

Following any changes to ``.pyx`` or ``.pxd`` files, you can re-compile by running::

    python build.py
