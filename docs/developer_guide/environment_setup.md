# Environment Setup

For development we recommend using the PyCharm *Professional* edition IDE, as it interprets Cython syntax. Alternatively, you could use Visual Studio Code with a Cython extension.

[pyenv](https://github.com/pyenv/pyenv) is the recommended tool for handling Python installations and virtual environments.

[poetry](https://python-poetry.org/) is the preferred tool for handling all Python package and dev dependencies.

[pre-commit](https://pre-commit.com/) is used to automatically run various checks, auto-formatters and linting tools at commit.

## Setup

The following steps are for UNIX-like systems, and only need to be completed once.

1. Install `poetry`:

       curl -sSL https://raw.githubusercontent.com/python-poetry/poetry/master/get-poetry.py | python -

2. Then install all Python package dependencies, and compile the C extensions:

        poetry install
   or

        make

4. Setup the pre-commit hook which will then run automatically at commit:

        pre-commit install

## Builds

Following any changes to `.pyx` or `.pxd` files, you can re-compile by running:

    poetry run python build.py

or

    make build
