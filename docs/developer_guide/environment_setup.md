# Environment Setup

For development we recommend using the PyCharm *Professional* edition IDE, as it interprets Cython syntax. Alternatively, you could use Visual Studio Code with a Cython extension.

[pyenv](https://github.com/pyenv/pyenv) is the recommended tool for handling Python installations and virtual environments.

[poetry](https://python-poetry.org/) is the preferred tool for handling all Python package and dev dependencies.

[pre-commit](https://pre-commit.com/) is used to automatically run various checks, auto-formatters and linting tools at commit.

NautilusTrader uses increasingly more [Rust](https://www.rust-lang.org), so Rust should be installed on your system as well
([installation guide](https://www.rust-lang.org/tools/install)).

## Setup

The following steps are for UNIX-like systems, and only need to be completed once.

1. Follow the [installation guide](../getting_started/installation.md) to set up the project with a modification to the final poetry command:

       poetry install

2. Set up the pre-commit hook which will then run automatically at commit:

       pre-commit install

3. In case of large recompiles for small changes, configure the `PYO3_PYTHON` variable in `nautilus_trader/.cargo/config.toml` with the path to the Python interpreter in the poetry managed environment. This is primarily useful for Rust developers working on core and experience frequent recompiles from IDE/rust analyzer based `cargo check`.

    ```
    poetry shell
    PYTHON_PATH=$(which python)
    echo -e "\n[env]\nPYO3_PYTHON = \"$PYTHON_PATH\"" >> .cargo/config.toml
    ```

    Since `.cargo/config.toml` is a tracked file, configure git to skip local modifications to it with `git update-index --skip-worktree .cargo/config.toml`. Git will still pull remote modifications. To push modifications track local modifications using `git update-index --no-skip-worktree .cargo/config.toml`.
    
    The git hack is needed till [local cargo config](https://github.com/rust-lang/cargo/issues/7723) feature is merged.

## Builds

Following any changes to `.pyx` or `.pxd` files, you can re-compile by running:

    poetry run python build.py

or

    make build

## Services

You can use `docker-compose.yml` file located in `.docker` directory 
to bootstrap the Nautilus working environment. This will start the following services:

```bash
docker-compose up -d
```

If you only want specific services running (like `postgres` for example), you can start them with command:

```bash
docker-compose up -d postgres
```

Used services are:

- `postgres`: Postgres database with root user `POSTRES_USER` which defaults to `postgres`, `POSTGRES_PASSWORD` which defaults to `pass` and `POSTGRES_DB` which defaults to `postgres`
- `redis`: Redis server
- `pgadmin`: PgAdmin4 for database management and administration

> **Note:** Please use this as development environment only. For production, use a proper and  more secure setup.

After the services has been started, you must log in with `psql` cli to create `nautilus` Postgres database.
To do that you can run, and type `POSTGRES_PASSWORD` from docker service setup

```bash
psql -h localhost -p 5432 -U postgres
```

After you have logged in as `postgres` administrator, run `CREATE DATABASE` command with target db name (we use `nautilus`): 

```
psql (16.2, server 15.2 (Debian 15.2-1.pgdg110+1))
Type "help" for help.

postgres=# CREATE DATABASE nautilus;
CREATE DATABASE

```

## Nautilus CLI Developer Guide

## Introduction

The Nautilus CLI is a command-line interface tool designed to interact
with the Nautilus Trader ecosystem. It provides commands for managing the Postgres database and other trading operations.

> **Note:** The Nautilus CLI command is only supported on UNIX-like systems.


## Install 

You can install nautilus cli command with from Make file target, which will use `cargo install` under the hood.
And this command will install `nautilus` bin executable in your path if Rust `cargo` is properly configured.

```bash
make install-cli
```

## Commands

You can run `nautilus --help` to inspect structure of CLI and groups of commands:

### Database

These are commands related to the bootstrapping the Postgres database.
For that you work, you need to supply right connection configuration. You can do that through 
command line arguments or `.env` file in the root directory or where the commands is being run.

- `--host` arg or `POSTGRES_HOST` for database host
- `--port` arg or `POSTGRES_PORT` for database port
- `--user` arg or `POSTGRES_USER` for root administrator user to run command with (namely `postgres` root user here)
- `--password` arg or `POSTGRES_PASSWORD` for root administrator password
- `--database` arg or `POSTGRES_DATABASE` for both database **name and new user** that will have privileges of this database
  ( if you provided `nautilus` as value, then new user will be created with name `nautilus` that will inherit the password from `POSTGRES_PASSWORD`
 and `nautilus` database with be bootstrapped with this user as owner)

Example of `.env` file

```
POSTGRES_HOST=localhost
POSTGRES_PORT=5432
POSTGRES_USERNAME=postgres
POSTGRES_DATABASE=nautilus
POSTGRES_PASSWORD=pass
```

List of commands are:

1. `nautilus database init`: Will bootstrap schema, roles and all sql files located in `schema` root directory (like `tables.sql`)
2. `nautilus database drop`: Will drop all tables, role and data in target Postgres database
