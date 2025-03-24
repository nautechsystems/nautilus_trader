# Environment Setup

For development we recommend using the PyCharm *Professional* edition IDE, as it interprets Cython syntax. Alternatively, you could use Visual Studio Code with a Cython extension.

[uv](https://docs.astral.sh/uv) is the preferred tool for handling all Python virtual environments and dependencies.

[pre-commit](https://pre-commit.com/) is used to automatically run various checks, auto-formatters and linting tools at commit.

NautilusTrader uses increasingly more [Rust](https://www.rust-lang.org), so Rust should be installed on your system as well
([installation guide](https://www.rust-lang.org/tools/install)).

## Setup

The following steps are for UNIX-like systems, and only need to be completed once.

1. Follow the [installation guide](../getting_started/installation.md) to set up the project with a modification to the final command to install development and test dependencies:

       uv sync --active --all-groups --all-extras

   or

       make install

   If you're developing and iterating frequently, then compiling in debug mode is often sufficient and *significantly* faster than a fully optimized build.
   To install in debug mode, use:

       make install-debug

2. Set up the pre-commit hook which will then run automatically at commit:

       pre-commit install

3. In case of large recompiles for small changes, configure the `PYO3_PYTHON` variable in `nautilus_trader/.cargo/config.toml` with the path to the Python interpreter in the virtual managed environment. This is primarily useful for Rust developers working on core and experience frequent recompiles from IDE/rust analyzer based `cargo check`.

    ```
    PYTHON_PATH=$(which python)
    echo -e "\n[env]\nPYO3_PYTHON = \"$PYTHON_PATH\"" >> .cargo/config.toml
    ```

    Since `.cargo/config.toml` is a tracked file, configure git to skip local modifications to it with `git update-index --skip-worktree .cargo/config.toml`. Git will still pull remote modifications. To push modifications track local modifications using `git update-index --no-skip-worktree .cargo/config.toml`.

    The git hack is needed till [local cargo config](https://github.com/rust-lang/cargo/issues/7723) feature is merged.

## Builds

Following any changes to `.rs`, `.pyx` or `.pxd` files, you can re-compile by running:

    uv run --no-sync python build.py

or

    make build

If you're developing and iterating frequently, then compiling in debug mode is often sufficient and *significantly* faster than a fully optimized build.
To compile in debug mode, use:

    make build-debug

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

:::info
Please use this as development environment only. For production, use a proper and more secure setup.
:::

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

The Nautilus CLI is a command-line interface tool for interacting with the NautilusTrader ecosystem.
It offers commands for managing the PostgreSQL database and handling various trading operations.

:::note
The Nautilus CLI command is only supported on UNIX-like systems.
:::

## Install

You can install the Nautilus CLI using the below Makefile target, which leverages `cargo install` under the hood.
This will place the nautilus binary in your system's PATH, assuming Rust's `cargo` is properly configured.

```bash
make install-cli
```

## Commands

You can run `nautilus --help` to view the CLI structure and available command groups:

### Database

These commands handle bootstrapping the PostgreSQL database.
To use them, you need to provide the correct connection configuration,
either through command-line arguments or a `.env` file located in the root directory or the current working directory.

- `--host` or `POSTGRES_HOST` for the database host
- `--port` or `POSTGRES_PORT` for the database port
- `--user` or `POSTGRES_USER` for the root administrator (typically the postgres user)
- `--password` or `POSTGRES_PASSWORD` for the root administrator's password
- `--database` or `POSTGRES_DATABASE` for both the database **name and the new user** with privileges to that database
    (e.g., if you provide `nautilus` as the value, a new user named nautilus will be created with the password from `POSTGRES_PASSWORD`, and the `nautilus` database will be bootstrapped with this user as the owner).

Example of `.env` file

```
POSTGRES_HOST=localhost
POSTGRES_PORT=5432
POSTGRES_USERNAME=postgres
POSTGRES_PASSWORD=pass
POSTGRES_DATABASE=nautilus
```

List of commands are:

1. `nautilus database init`: Will bootstrap schema, roles and all sql files located in `schema` root directory (like `tables.sql`)
2. `nautilus database drop`: Will drop all tables, roles and data in target Postgres database
