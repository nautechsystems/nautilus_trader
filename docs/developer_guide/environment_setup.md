# Environment Setup

Use an editor with current Rust and Python language support, such as PyCharm or Visual Studio Code.

[uv](https://docs.astral.sh/uv) is the preferred tool for handling all Python virtual environments and dependencies.

[prek](https://github.com/j178/prek) is used to automatically run various pre-commit checks, auto-formatters, and linting tools at commit.

NautilusTrader uses increasingly more [Rust](https://www.rust-lang.org), so Rust should be installed on your system as well
([installation guide](https://www.rust-lang.org/tools/install)).

[Cap'n Proto](https://capnproto.org/) is required for serialization schema compilation. The required
version is specified in `tools.toml` in the repository root. Ubuntu's default package
is typically too old, so you may need to install from source (see below).

:::info
NautilusTrader *must* compile and run on **Linux, macOS, and Windows**. Please keep portability in
mind: use `std::path::Path` in code and follow the
[shell portability policy](shell.md#define-the-portability-target) for scripts.
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

make sync
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

Follow the [installation guide](../getting_started/installation.md), then sync the development and
test dependencies from the repository root:

```bash
make sync
```

For frequent development, install a debug build of the package into the root `.venv`:

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
  `cargo-deny`, `cargo-edit`, `cargo-fuzz`, `cargo-llvm-cov`, `cargo-machete`, `cargo-nextest`,
  `cargo-vet`, `flamegraph`, `lychee`.
- **Prebuilt binaries** pinned in `tools.toml`: `prek` (pre-commit runner) and `osv-scanner`
  (vulnerability scanner).
- **uv**, installed at the project version pinned in `tools.toml`. The supported local uv minor
  series is defined in `python/pyproject.toml`.

Cap'n Proto is also pinned in `tools.toml` but installs separately; see the [Cap'n Proto](#capn-proto)
section below.

Fuzz targets also require a Rust nightly toolchain at runtime because `cargo-fuzz` uses
`libfuzzer-sys` and unstable compiler flags:

```bash
rustup toolchain install nightly
```

#### One-off prerequisite: cargo-binstall

`make install-tools` uses [`cargo-binstall`](https://github.com/cargo-bins/cargo-binstall) to fetch
`prek` as a prebuilt binary instead of compiling it from source. Install `cargo-binstall` once per
machine:

```bash
cargo install cargo-binstall --locked
```

This is a one-time step. Subsequent runs of `make install-tools` reuse the installed `cargo-binstall`.

#### Single source of truth for versions

The repository manifests are the canonical source for dependency and tool versions. Do not copy
current version numbers into docs, runner images, or scripts unless there is no manifest-backed way
to read them.

| Source file or section                    | Defines                                                 |
| ----------------------------------------- | ------------------------------------------------------- |
| `rust-toolchain.toml`                     | Rust toolchain.                                         |
| `Cargo.toml` and `Cargo.lock`             | Rust workspace dependencies and exact resolution.       |
| `Cargo.toml` `[workspace.metadata.tools]` | Cargo‑installable development tools.                    |
| `python/pyproject.toml`                   | Python dependencies and supported Python and uv ranges. |
| `python/uv.lock`                          | Exact Python dependency resolution.                     |
| `tools.toml`                              | External CLIs and binaries without a native manifest.   |

The external tool pins in `tools.toml` include uv, `prek`, `pip-audit`, `pypi-attestations`,
`osv-scanner`, and `capnp`.

The Makefile reads these via `scripts/cargo-tool-version.sh`, `scripts/tool-version.sh`, and
`scripts/uv-version.sh`, so bumping a version in the source file is the only required version
change. To check the pinned cargo tool versions against crates.io, run:

```bash
make outdated
```

### 3. Set up Git hooks

Set up the file and commit‑message hooks, which run automatically when committing:

```bash
prek install
```

Rerun `prek install` after pulling a change to the configured hook types.

Before opening a pull-request run the formatting and lint suite locally so that CI passes on the
first attempt:

```bash
make format
make pre-commit
```

Make sure the Rust compiler reports **zero errors** -- broken builds slow everyone down.

### 4. Configure environment variables

**Required for Rust/PyO3 (Linux and macOS)**: When using Python installed via `uv` on Linux or
macOS, set the following environment variables from the repository root after `make sync`:

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
`python/pyproject.toml` enforces three supply chain safety settings:

- **`required-version`**: local uv commands accept any patch release in the supported minor series.
  If your local uv is outside that range, `uv lock` and `uv sync` fail with a version mismatch.
  `tools.toml` separately pins the exact version used by CI, Docker, pre‑commit, and
  `make update-uv`. The stub targets run through `make sync`, so they enforce the same supported
  range; see [Generated Python artifacts](rust.md#generated-python-artifacts).
- **`exclude-newer = "7 days"`**: `uv lock` ignores package versions published within the last
  7 days. This gives the community time to detect and quarantine compromised releases before they
  enter the lockfile. The value accepts an RFC 3339 timestamp (`"2026-03-30T00:00:00Z"`), a friendly
  duration (`"7 days"`, `"1 week"`, `"24 hours"`), or an ISO 8601 duration (`"P7D"`, `"P1W"`,
  `"PT24H"`). uv 0.11.8+ stores the friendly/ISO form as `exclude-newer-span` inside
  `python/uv.lock` and emits a sentinel `exclude-newer` timestamp alongside it for backwards
  compatibility. `python/uv.lock` uses that format.
- **`no-build-package`**: explicit list of every third-party package locked in `python/uv.lock`. `uv`
  refuses to build any of them from source. In normal operation uv prefers wheels, so the setting
  is a no-op; it triggers only if a listed package stops publishing wheels for the target platform,
  in which case `uv lock` fails rather than silently building from an sdist. The local workspace
  package is intentionally not in the list because it must be built by the workspace's own build
  backend. The list is kept in sync with `python/uv.lock` by
  `scripts/check-no-build-packages.sh`, which also runs as a pre-commit hook on changes to the
  lockfile or manifest.

### Bypassing the cooldown

When a security patch or critical bug fix must be pulled in immediately, review the release and
override `exclude-newer` for that lock operation. Prefer a package‑scoped override so unrelated
packages remain subject to the 7‑day default. Do not add persistent package overrides to
`python/pyproject.toml`. All forms accept a timestamp, friendly duration, or ISO duration; package
overrides additionally accept `false` to exempt a package from the cooldown entirely.

```bash
# Shorten the cooldown for a single package (friendly duration)
uv lock --project python --exclude-newer-package "somepackage=1 day"

# Pin a single package to an absolute cutoff
uv lock --project python --exclude-newer-package "somepackage=2026-03-30T00:00:00Z"

# Exempt a single package from the cooldown entirely
uv lock --project python --exclude-newer-package "somepackage=false"

# Disable the cooldown for the whole resolution after reviewing every newly eligible package
uv lock --project python --exclude-newer "0 seconds"
```

The CLI flag overrides the `python/pyproject.toml` value for that invocation only. The config
remains unchanged for subsequent runs.

### Updating uv

To support a new uv minor series, change `required-version` in `python/pyproject.toml`. To update the
exact project version within that range, change `[uv].version` in `tools.toml`, the `rev` in
`.pre-commit-config.yaml`, and each digest‑pinned uv Docker image. Run `make update-uv` to install the
project version locally.

## Builds

The Python package and the standalone Nautilus CLI are separate build artifacts. `make build-debug`
and `make build` install the Python package into the root `.venv`; neither command updates the
`nautilus` binary in Cargo's binary directory. See the
[Nautilus CLI developer guide](#nautilus-cli-developer-guide) when changing or using the CLI.

After changing Rust bindings or Python package code, use a debug build for normal development. It
skips release optimization and LTO, which reduces build time and peak memory use:

```bash
make build-debug
```

Use `make build` when you need an optimized build. The release profile uses fat LTO and one code
generation unit, which increases peak memory use. Fat LTO can complete on a 16 GB machine when the
build has access to the full memory allocation and sufficient swap. Check VM or container memory
limits when applicable.

If the linker runs out of memory, use ThinLTO for an optimized local build:

```bash
CARGO_PROFILE_RELEASE_LTO=thin make build
```

This override applies only to that command. Use the default fat LTO profile for performance
measurements.

### Refresh after pulling changes

Use the command that updates the affected artifact. The build targets call their prerequisites, so
`make build-debug` also syncs Python dependencies and regenerates Python type stubs.

| Changed input                                       | Command                      | Updated artifact                                  |
| --------------------------------------------------- | ---------------------------- | ------------------------------------------------- |
| `python/pyproject.toml` or `python/uv.lock`         | `make sync`                  | Dependencies in the root `.venv`.                 |
| Rust bindings, Python package code, or stub sources | `make build-debug`           | Debug Python package and generated type stubs.    |
| CLI code, SQL initialization code, or `schema/sql`  | `make install-cli`           | Standalone `nautilus` binary in Cargo's bin path. |
| Cargo, uv, `prek`, or OSV Scanner tool pins         | `make install-tools`         | Pinned development tools.                         |
| Cap'n Proto version in `tools.toml`                 | `./scripts/install-capnp.sh` | Cap'n Proto compiler.                             |

The environment variables in [Configure environment variables](#4-configure-environment-variables)
contain checkout‑specific paths. After switching checkouts, changing the selected Python version,
or recreating `.venv`, activate that checkout's environment and export the variables again. Verify
that the shell resolves Python from the expected checkout:

```bash
source .venv/bin/activate
command -v python
python --version
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

The Cranelift code generation backend can reduce local build time for development, tests, and IDE
checks. It requires the nightly Rust toolchain and local changes to `Cargo.toml`:

```bash
rustup toolchain install nightly --component rust-analyzer
```

Save the patch below, then apply it with `git apply <patch>`. Remove it with
`git apply -R <patch>` before pushing changes.

:::warning
Do not commit these changes. The cranelift patch is for local development only and will break CI if pushed.
:::

```diff
diff --git a/Cargo.toml b/Cargo.toml
--- a/Cargo.toml
+++ b/Cargo.toml
@@ -1,3 +1,5 @@
+cargo-features = ["codegen-backend"]
+
 [workspace]
 resolver = "2"
 members = [
@@ -424,6 +426,7 @@
 lto = false
 panic = "unwind"
 incremental = true
+codegen-backend = "cranelift"

 # Compile third-party deps at opt-level=1 in dev/test profiles. Workspace
 # members keep opt-level=0 (fast iteration); deps recompile rarely so the
@@ -444,6 +447,7 @@
 strip = false
 lto = false
 incremental = true
+codegen-backend = "cranelift"

 [profile.test.package."*"]
 opt-level = 1
@@ -452,6 +456,7 @@
 inherits = "test"
 debug = false # Improves compile times
 strip = "debuginfo" # Improves compile times
+codegen-backend = "cranelift"

 [profile.ci-pr]
 inherits = "test"
```

Run local build commands with `RUSTUP_TOOLCHAIN=nightly`, for example:

```bash
RUSTUP_TOOLCHAIN=nightly make build-debug
```

Set the same toolchain in your [rust-analyzer settings](#rust-analyzer-settings) when using this
local patch.

## Services

Initialize PostgreSQL, Redis, and pgAdmin from the repository root:

```bash
make init-services
```

This starts the containers and initializes the NautilusTrader database schema. To start the
containers without reinitializing the schema, run `make start-services`. To start one service, use
the Compose file directly:

```bash
docker compose -f .docker/docker-compose.yml up -d postgres
```

The development services are:

- `postgres`: PostgreSQL with `POSTGRES_USER=nautilus`, `POSTGRES_PASSWORD=pass`, and
  `POSTGRES_DB=nautilus` by default.
- `redis`: Redis server.
- `pgadmin`: pgAdmin 4 for database management and administration.

:::info
Please use this as development environment only. For production, use a proper and more secure setup.
:::

Use `make stop-services` to stop the containers without removing their data. Use
`make purge-services` only when you intend to delete the development volumes.

PostgreSQL‑backed tests can each maintain several connections. On a high‑core workstation, the
local nextest concurrency can exceed the development container's connection limit. Use the CI
profile to match CI's lower concurrency:

```bash
NEXTEST_PROFILE=ci make cargo-test-extras
```

To retain more local parallelism, set an explicit bounded worker count, for example:

```bash
NEXTEST_TEST_THREADS=8 make cargo-test-extras
```

## Nautilus CLI developer guide

The Nautilus CLI is a standalone Rust binary for PostgreSQL administration and other repository
operations. It is independent from the Python package installed by `make build-debug` or
`make build`.

### Build and select the CLI

Install the CLI from the current checkout with:

```bash
make install-cli
```

This target runs `cargo install --locked --force` and places `nautilus` in Cargo's binary directory,
normally `~/.cargo/bin`. Reinstall it after pulling changes to `crates/cli`, SQL initialization code,
or `schema/sql`. An installed CLI can otherwise remain older than the checkout while reading newer
schema files from it.

Before running repository‑dependent commands, check which binary the shell resolves and its version:

```bash
command -v nautilus
nautilus --version
```

To build and run the CLI directly from the checkout without replacing the installed binary, use:

```bash
cargo run --locked --package nautilus-cli --bin nautilus -- --help
```

:::warning
On Linux systems with GNOME, `/usr/bin/nautilus` is normally the GNOME file manager. Select the
NautilusTrader CLI with one of these methods:

- Put `~/.cargo/bin` before `/usr/bin` in `PATH`.
- Run `~/.cargo/bin/nautilus` explicitly.
- Add `alias nautilus="$HOME/.cargo/bin/nautilus"` to the shell configuration.

:::

Windows source installs require GNU Make through MSYS2 or WSL. The nightly workflow also publishes
a Windows x86‑64 CLI archive.

Run `nautilus --help` to view the available command groups.

### Database commands

The database commands accept connection settings as command‑line arguments or through a `.env` file
in the current working directory or one of its parents. The CLI also accepts the corresponding
environment variables.

| Flag         | Environment variable | Purpose                                                       |
| ------------ | -------------------- | ------------------------------------------------------------- |
| `--host`     | `POSTGRES_HOST`      | Database host.                                                |
| `--port`     | `POSTGRES_PORT`      | Database port.                                                |
| `--username` | `POSTGRES_USERNAME`  | Connecting administrator, normally the `postgres` role.       |
| `--password` | `POSTGRES_PASSWORD`  | Administrator password and password for the application role. |
| `--database` | `POSTGRES_DATABASE`  | Database name and application role created during `init`.     |
| `--schema`   | `SCHEMA_DIR`         | Directory containing the SQL schema files.                    |

For example:

```dotenv
POSTGRES_HOST=localhost
POSTGRES_PORT=5432
POSTGRES_USERNAME=postgres
POSTGRES_PASSWORD=pass
POSTGRES_DATABASE=nautilus
```

`nautilus database init` creates or updates the roles and schema from the SQL files. Pass the schema
directory explicitly so renamed clones and worktrees do not depend on checkout path detection:

```bash
nautilus database init --schema "$PWD/schema/sql"
```

Use a CLI built from the same checkout as these schema files. The initialization is designed to be
re‑run, including after an earlier run stopped partway through.

:::danger
`nautilus database drop` removes the target schema, privileges, role, and stored data. Use it only
for a disposable database or after confirming that the data can be deleted.
:::

Run `nautilus database --help` for the complete command syntax.

## Rust analyzer settings

Rust analyzer is a popular language server for Rust and integrates with many IDEs. Configure its
`VIRTUAL_ENV` to use the root `.venv`. If PyO3 analysis cannot locate Python, also provide the
`PYO3_PYTHON` and `PYTHONHOME` values from [Configure environment variables](#4-configure-environment-variables).
The examples below cover VS Code and AstroNvim. For other settings, see the
[rust-analyzer configuration](https://rust-analyzer.github.io/book/configuration.html).

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
