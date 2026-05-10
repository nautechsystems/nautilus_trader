# Environment Setup

For development we recommend using the PyCharm *Professional* edition IDE, as it interprets Cython syntax. Alternatively, you could use Visual Studio Code with a Cython extension.

[uv](https://docs.astral.sh/uv) is the preferred tool for handling all Python virtual environments and dependencies.

[prek](https://github.com/j178/prek) is used to automatically run various pre-commit checks, auto-formatters and linting tools at commit.

NautilusTrader uses increasingly more [Rust](https://www.rust-lang.org), so Rust should be installed on your system as well
([installation guide](https://www.rust-lang.org/tools/install)).

[Cap'n Proto](https://capnproto.org/) is required for serialization schema compilation. The required
version is specified in `tools.toml` in the repository root. Ubuntu's default package
is typically too old, so you may need to install from source (see below).

:::info
NautilusTrader *must* compile and run on **Linux, macOS, and Windows**. Please keep portability in
mind (use `std::path::Path`, avoid Bash-isms in shell scripts, etc.).
:::

## Setup

The following steps are for UNIX-like systems, and only need to be completed once.

### Quick setup

Use this as a compact setup path for a new Linux or macOS development machine. The detailed
sections below explain each step and cover alternatives.

Install platform tools first:

```bash tab="Ubuntu"
sudo apt-get update
sudo apt-get install -y build-essential clang lld curl git make pkg-config
```

```bash tab="macOS"
xcode-select --install
```

Then clone the repository and install the pinned project tools:

```bash
git clone --branch develop https://github.com/nautechsystems/nautilus_trader
cd nautilus_trader

curl https://sh.rustup.rs -sSf | sh
source "$HOME/.cargo/env"

curl -LsSf https://astral.sh/uv/install.sh | sh
export PATH="$HOME/.local/bin:$PATH"

cargo install cargo-binstall --locked
make install-tools
./scripts/install-capnp.sh

uv sync --all-groups --all-extras
source .venv/bin/activate

export PYO3_PYTHON="$PWD/.venv/bin/python"

if [ "$(uname -s)" = "Linux" ]; then
  PYTHON_LIB_DIR="$("$PYO3_PYTHON" -c 'import sysconfig; print(sysconfig.get_config_var("LIBDIR"))')"
  export LD_LIBRARY_PATH="$PYTHON_LIB_DIR${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
fi

export PYTHONHOME="$("$PYO3_PYTHON" -c 'import sys; print(sys.base_prefix)')"

prek install
make build-debug
```

Windows users should follow the source installation steps in the
[installation guide](../getting_started/installation.md#from-source), then use the relevant commands
from this guide.

### 1. Install dependencies

Follow the [installation guide](../getting_started/installation.md) to set up the project with a modification to the final command to install development and test dependencies:

```bash tab="uv"
uv sync --active --all-groups --all-extras
```

```bash tab="make"
make install
```

If you're developing and iterating frequently, then compiling in debug mode is often sufficient and *significantly* faster than a fully optimized build.
To install in debug mode, use:

```bash
make install-debug
```

### 2. Install development tools

NautilusTrader pins every development tool so that all contributors and CI run identical versions.
A single Makefile target installs the full set:

```bash
make install-tools
```

This installs:

- **Cargo CLIs** pinned in `Cargo.toml` under `[workspace.metadata.tools]`: `cargo-audit`,
  `cargo-deny`, `cargo-edit`, `cargo-llvm-cov`, `cargo-machete`, `cargo-nextest`, `cargo-vet`,
  `lychee`.
- **Prebuilt binaries** pinned in `tools.toml`: `prek` (pre-commit runner) and `osv-scanner`
  (vulnerability scanner).
- **uv**, synced to the version required by `pyproject.toml`.

Cap'n Proto is also pinned in `tools.toml` but installs separately; see the [Cap'n Proto](#capn-proto)
section below.

#### One-off prerequisite: cargo-binstall

`make install-tools` uses [`cargo-binstall`](https://github.com/cargo-bins/cargo-binstall) to fetch
`prek` as a prebuilt binary instead of compiling it from source. Install `cargo-binstall` once per
machine:

```bash
cargo install cargo-binstall --locked
```

This is a one-time step. Subsequent runs of `make install-tools` reuse the installed `cargo-binstall`.

#### Single source of truth for versions

Tool versions live in two files:

- `Cargo.toml` under `[workspace.metadata.tools]` for cargo-installable crates.
- `tools.toml` for everything else (`prek`, `osv-scanner`, `capnp`).

The Makefile reads these via `scripts/cargo-tool-version.sh` and `scripts/tool-version.sh`, so
bumping a version in the source file is the only change required. To check the pinned cargo tool
versions against crates.io, run:

```bash
make outdated
```

### 3. Set up pre-commit

Set up the pre-commit hook which will then run automatically at commit:

```bash
prek install
```

Before opening a pull-request run the formatting and lint suite locally so that CI passes on the
first attempt:

```bash
make format
make pre-commit
```

Make sure the Rust compiler reports **zero errors** -- broken builds slow everyone down.

### 4. Configure environment variables

**Required for Rust/PyO3 (Linux and macOS)**: When using Python installed via `uv` on Linux or
macOS, set the following environment variables from the repository root after `uv sync`:

```bash
# Set the Python executable path for PyO3
export PYO3_PYTHON="$PWD/.venv/bin/python"

# Linux only: Set the library path for the uv-managed Python runtime
PYTHON_LIB_DIR="$("$PYO3_PYTHON" -c 'import sysconfig; print(sysconfig.get_config_var("LIBDIR"))')"
export LD_LIBRARY_PATH="$PYTHON_LIB_DIR${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"

# Set the Python home path (required for Rust tests)
export PYTHONHOME="$("$PYO3_PYTHON" -c 'import sys; print(sys.base_prefix)')"
```

:::note
The `LD_LIBRARY_PATH` export is Linux-specific and not needed on macOS or Windows.

- `PYO3_PYTHON` tells PyO3 which Python interpreter to use, reducing unnecessary recompilation.
- `PYTHONHOME` is required when running `make cargo-test` with a `uv`-installed Python.
  Without it, tests that depend on PyO3 may fail to locate the Python runtime.

:::

To verify your environment is configured correctly:

```bash
python -c "import sys; print('Python:', sys.executable, sys.version)"
echo "PYO3_PYTHON: $PYO3_PYTHON"
echo "PYTHONHOME: $PYTHONHOME"
```

## Dependency management

Python dependencies are managed by [uv](https://docs.astral.sh/uv). The `[tool.uv]` section in
`pyproject.toml` enforces two supply chain safety settings:

- **`required-version = "==0.11.8"`**: all developers and CI use the same uv version. The version
  is extracted by `scripts/uv-version.sh` for Makefile, CI, and Docker builds. If your local uv
  drifts off the pin, `uv lock`/`uv sync` will fail with `Required uv version ... does not match the
  running version ...`. Run `make update-uv` to install the pinned version (or follow uv's own
  `uv self update <version>` hint).
- **`exclude-newer = "3 days"`**: `uv lock` ignores package versions published within the last
  3 days. This gives the community time to detect and quarantine compromised releases before they
  enter the lockfile. The value accepts an RFC 3339 timestamp (`"2026-03-30T00:00:00Z"`), a friendly
  duration (`"3 days"`, `"1 week"`, `"24 hours"`), or an ISO 8601 duration (`"P3D"`, `"P1W"`,
  `"PT24H"`). uv 0.11.8+ stores the friendly/ISO form as `exclude-newer-span` inside `uv.lock` and
  emits a sentinel `exclude-newer` timestamp alongside it for backwards compatibility; both
  lockfiles in this repo use that format.

### Bypassing the cooldown

When a security patch or critical bug fix must be pulled in immediately, override `exclude-newer`
on the command line. All forms accept a timestamp, friendly duration, or ISO duration; package
overrides additionally accept `false` to exempt a package from the cooldown entirely.

```bash
# Shorten the cooldown for a single package (friendly duration)
uv lock --exclude-newer-package "somepackage=1 day"

# Pin a single package to an absolute cutoff
uv lock --exclude-newer-package "somepackage=2026-03-30T00:00:00Z"

# Exempt a single package from the cooldown entirely
uv lock --exclude-newer-package "somepackage=false"

# Disable the cooldown for the whole resolution
uv lock --exclude-newer "0 seconds"
```

The CLI flag overrides the `pyproject.toml` value for that invocation only. The config remains
unchanged for subsequent runs.

### Updating uv

To update the pinned uv version, change `required-version` in both `pyproject.toml` and
`python/pyproject.toml`, then update the `rev` in `.pre-commit-config.yaml` to match. Run
`make update-uv` to install the new pinned version locally.

## Builds

Following any changes to `.rs`, `.pyx` or `.pxd` files, you can re-compile by running:

```bash tab="uv"
uv run --no-sync python build.py
```

```bash tab="make"
make build
```

If you're developing and iterating frequently, then compiling in debug mode is often sufficient and *significantly* faster than a fully optimized build.
To compile in debug mode, use:

```bash
make build-debug
```

## Cap'n Proto

[Cap'n Proto](https://capnproto.org/) is required for serialization schema compilation.
The required version is defined in `tools.toml` in the repository root.

Install the correct version for your platform:

```bash tab="Script (Linux/macOS)"
./scripts/install-capnp.sh
```

```bash tab="macOS (Homebrew)"
brew install capnp
```

```bash tab="Linux (source)"
CAPNP_VERSION=$(bash scripts/tool-version.sh capnp)
cd ~
wget https://capnproto.org/capnproto-c++-${CAPNP_VERSION}.tar.gz
tar xzf capnproto-c++-${CAPNP_VERSION}.tar.gz
cd capnproto-c++-${CAPNP_VERSION}
./configure
make -j$(nproc)
sudo make install
sudo ldconfig
```

```bash tab="Windows (Chocolatey)"
choco install capnproto
```

Verify the installed version matches `tools.toml`:

```bash
capnp --version
```

The install script ensures the pinned version is installed. If Homebrew or Chocolatey provides
an older version, install from source or see the
[Cap'n Proto installation guide](https://capnproto.org/install.html).

## Faster builds

The cranelift backends reduces build time significantly for dev, testing and IDE checks. However, cranelift is available on the nightly toolchain and needs extra configuration. Install the nightly toolchain

```
rustup install nightly
rustup override set nightly
rustup component add rust-analyzer # install nightly lsp
rustup override set stable # reset to stable
```

Activate the nightly feature and use "cranelift" backend for dev and testing profiles in workspace `Cargo.toml`. You can apply the below patch using `git apply <patch>`. You can remove it using `git apply -R <patch>` before pushing changes.

:::warning
Do not commit these changes. The cranelift patch is for local development only and will break CI if pushed.
:::

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

Pass `RUSTUP_TOOLCHAIN=nightly` when running `make build-debug` like commands and include it in all [rust analyzer settings](#rust-analyzer-settings) for faster builds and IDE checks.

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

- `postgres`: Postgres database with root user `POSTGRES_USER` which defaults to `postgres`, `POSTGRES_PASSWORD` which defaults to `pass` and `POSTGRES_DB` which defaults to `postgres`.
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

:::warning
On Linux systems with GNOME desktop, the `nautilus` command typically refers to the GNOME file manager (`/usr/bin/nautilus`).
After installing the NautilusTrader CLI, you may need to ensure the Cargo binary takes precedence by either:

- Adding an alias to your shell config: `alias nautilus="$HOME/.cargo/bin/nautilus"`
- Using the full path: `~/.cargo/bin/nautilus`
- Ensuring `~/.cargo/bin` appears before `/usr/bin` in your `PATH`

:::

:::note
The Nautilus CLI command is only supported on UNIX-like systems.
:::

## Install

You can install the Nautilus CLI using the below Makefile target, which uses `cargo install` under the hood.
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
- `--user` or `POSTGRES_USERNAME` for the root administrator (typically the postgres user)
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

```json tab="VSCode"
{
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
}
```

```lua tab="Neovim (AstroLSP)"
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
}
```
