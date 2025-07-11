# Environment Setup

For development we recommend using the PyCharm *Professional* edition IDE, as it interprets Cython syntax. Alternatively, you could use Visual Studio Code with a Cython extension.

[uv](https://docs.astral.sh/uv) is the preferred tool for handling all Python virtual environments and dependencies.

[pre-commit](https://pre-commit.com/) is used to automatically run various checks, auto-formatters and linting tools at commit.

NautilusTrader uses increasingly more [Rust](https://www.rust-lang.org), so Rust should be installed on your system as well
([installation guide](https://www.rust-lang.org/tools/install)).

:::info
NautilusTrader *must* compile and run on **Linux, macOS, and Windows**. Please keep portability in
mind (use `std::path::Path`, avoid Bash-isms in shell scripts, etc.).
:::

## Setup

The following steps are for UNIX-like systems, and only need to be completed once.

1. Follow the [installation guide](../getting_started/installation.md) to set up the project with a modification to the final command to install development and test dependencies:

```bash
uv sync --active --all-groups --all-extras
```

or

```bash
make install
```

If you're developing and iterating frequently, then compiling in debug mode is often sufficient and *significantly* faster than a fully optimized build.
To install in debug mode, use:

```bash
make install-debug
```

2. Set up the pre-commit hook which will then run automatically at commit:

```bash
pre-commit install
```

Before opening a pull-request run the formatting and lint suite locally so that CI passes on the
first attempt:

```bash
make format
make pre-commit
```

Make sure the Rust compiler reports **zero errors** – broken builds slow everyone down.

3. **Optional**: For frequent Rust development, configure the `PYO3_PYTHON` variable in `.cargo/config.toml` with the path to the Python interpreter. This helps reduce recompilation times for IDE/rust-analyzer based `cargo check`:

```bash
PYTHON_PATH=$(which python)
echo -e "\n[env]\nPYO3_PYTHON = \"$PYTHON_PATH\"" >> .cargo/config.toml
```

Since `.cargo/config.toml` is tracked, configure git to skip any local modifications:

```bash
git update-index --skip-worktree .cargo/config.toml
```

To restore tracking: `git update-index --no-skip-worktree .cargo/config.toml`

## Builds

Following any changes to `.rs`, `.pyx` or `.pxd` files, you can re-compile by running:

```bash
uv run --no-sync python build.py
```

or

```bash
make build
```

If you're developing and iterating frequently, then compiling in debug mode is often sufficient and *significantly* faster than a fully optimized build.
To compile in debug mode, use:

```bash
make build-debug
```

## Faster builds

The cranelift backends reduces build time significantly for dev, testing and IDE checks. However, cranelift is available on the nightly toolchain and needs extra configuration. Install the nightly toolchain

```
rustup install nightly
rustup override set nightly
rustup component add rust-analyzer # install nightly lsp
rustup override set stable # reset to stable
```

Activate the nightly feature and use "cranelift" backend for dev and testing profiles in workspace `Cargo.toml`. You can apply the below patch using `git apply <patch>`. You can remove it using `git apply -R <patch>` before pushing changes.

```
diff --git a/Cargo.toml b/Cargo.toml
index 62b78cd8d0..beb0800211 100644
--- a/Cargo.toml
+++ b/Cargo.toml
@@ -1,3 +1,6 @@
+# This line needs to come before anything else in Cargo.toml
+cargo-features = ["codegen-backend"]
+
 [workspace]
 resolver = "2"
 members = [
@@ -140,6 +143,7 @@ lto = false
 panic = "unwind"
 incremental = true
 codegen-units = 256
+codegen-backend = "cranelift"

 [profile.test]
 opt-level = 0
@@ -150,11 +154,13 @@ strip = false
 lto = false
 incremental = true
 codegen-units = 256
+codegen-backend = "cranelift"

 [profile.nextest]
 inherits = "test"
 debug = false # Improves compile times
 strip = "debuginfo" # Improves compile times
+codegen-backend = "cranelift"

 [profile.release]
 opt-level = 3
```

Pass `RUSTUP_TOOLCHAIN=nightly` when running `make build-debug` like commands and include it in in all [rust analyzer settings](#rust-analyzer-settings) for faster builds and IDE checks.

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

- `postgres`: Postgres database with root user `POSTRES_USER` which defaults to `postgres`, `POSTGRES_PASSWORD` which defaults to `pass` and `POSTGRES_DB` which defaults to `postgres`.
- `redis`: Redis server.
- `pgadmin`: PgAdmin4 for database management and administration.

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

## Nautilus CLI developer guide

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

1. `nautilus database init`: Will bootstrap schema, roles and all sql files located in `schema` root directory (like `tables.sql`).
2. `nautilus database drop`: Will drop all tables, roles and data in target Postgres database.

## Rust analyzer settings

Rust analyzer is a popular language server for Rust and has integrations for many IDEs. It is recommended to configure rust analyzer to have same environment variables as `make build-debug` for faster compile times. Below tested configurations for VSCode and Astro Nvim are provided. For more information see [PR](https://github.com/nautechsystems/nautilus_trader/pull/2524) or rust analyzer [config docs](https://rust-analyzer.github.io/book/configuration.html).

### VSCode

You can add the following settings to your VSCode `settings.json` file:

```
    "rust-analyzer.restartServerOnConfigChange": true,
    "rust-analyzer.linkedProjects": [
        "Cargo.toml"
    ],
    "rust-analyzer.cargo.features": "all",
    "rust-analyzer.check.workspace": false,
    "rust-analyzer.check.extraEnv": {
        "VIRTUAL_ENV": "<path-to-your-virtual-environment>/.venv",
        "CC": "clang",
        "CXX": "clang++"
    },
    "rust-analyzer.cargo.extraEnv": {
        "VIRTUAL_ENV": "<path-to-your-virtual-environment>/.venv",
        "CC": "clang",
        "CXX": "clang++"
    },
    "rust-analyzer.runnables.extraEnv": {
        "VIRTUAL_ENV": "<path-to-your-virtual-environment>/.venv",
        "CC": "clang",
        "CXX": "clang++"
    },
    "rust-analyzer.check.features": "all",
    "rust-analyzer.testExplorer": true
```

### Astro Nvim (Neovim + AstroLSP)

You can add the following to your astro lsp config file:

```
    config = {
      rust_analyzer = {
        settings = {
          ["rust-analyzer"] = {
            restartServerOnConfigChange = true,
            linkedProjects = { "Cargo.toml" },
            cargo = {
              features = "all",
              extraEnv = {
                VIRTUAL_ENV = "<path-to-your-virtual-environment>/.venv",
                CC = "clang",
                CXX = "clang++",
              },
            },
            check = {
              workspace = false,
              command = "check",
              features = "all",
              extraEnv = {
                VIRTUAL_ENV = "<path-to-your-virtual-environment>/.venv",
                CC = "clang",
                CXX = "clang++",
              },
            },
            runnables = {
              extraEnv = {
                VIRTUAL_ENV = "<path-to-your-virtual-environment>/.venv",
                CC = "clang",
                CXX = "clang++",
              },
            },
            testExplorer = true,
          },
        },
      },
```
