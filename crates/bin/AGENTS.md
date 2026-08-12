# AGENTS.md

NautilusTrader: Rust-native trading engine (v2) with a Python control plane. This is a fork of
nautechsystems/nautilus_trader on `develop`; active development is in `crates/bin` (`nautilus_bin`),
a private crate of live-trading binaries. Rust code lives in `crates/*`; Python v1 (legacy Cython)
lives at the repo root; Python v2 (PyO3) lives under `python/`.

## crates/bin (`nautilus_bin`) — the active work

- Two binaries: `strategy_runner` (dispatches on `[runner] strategy` in `config.toml`, no recompilation to switch) and `recorder`. `cargo check -p nautilus_bin` / `cargo run --bin strategy_runner` from the repo root or `crates/bin`. Don't use `make build-debug` for this crate.
- Strategies implement the Rust-native v2 `nautilus_trading::Strategy` trait (`src/strategy/*`). Registered names: `grid_mm`, `mmm`. Each strategy has its own TOML section with `execution_environment = "backtest" | "live" | "sandbox"`.
- Backtest mode reads parquet order-book/trade data from the catalog `path` (e.g. `/var/lib/nautilus-trader/`) and requires `[runner] start_date`/`end_date`/`run_id`. Sandbox is an unimplemented `todo!()` in `src/runner.rs`.
- Live mode needs exchange credentials from `.env` (dotenvy) / env vars; venues: bybit, dydx. `Environment::Sandbox` and `Environment::Live` both build a `nautilus_live::LiveNode` — see `src/exchange.rs`.
- RPM packaging via `cargo generate-rpm` (`[package.metadata.generate-rpm]`): binaries to `/usr/bin`, config to `/etc/nautilus-trader/`, systemd unit `nautilus-recorder`. Requires prebuilt release binaries (`target/release/{strategy_runner,recorder}`).
- No tests in this crate yet.

## Toolchain and setup

- Dev tools are version-pinned: `make install-tools` (one-off prerequisite: `cargo install cargo-binstall --locked`), then `prek install`. Commits are gated by `prek` (a pre-commit runner), not plain pre-commit.
- Rust toolchain is pinned to 1.97.1 via `rust-toolchain.toml`. rustfmt must run on **nightly**: `cargo +nightly fmt` (repo config sets `imports_granularity = "Crate"`, `group_imports = "StdExternalCrate"`). Plain `cargo fmt` will not match CI.
- `make` is the source of truth for dev commands; `make help` documents every target.
- uv is version-pinned (`required-version = "==0.11.29"` in both pyprojects); v2 targets abort on any other version.

## Checks and testing

- `make format` (rustfmt nightly + ruff format), `make check-code` (workspace clippy `-D warnings` on lib+tests with features `arrow,ffi,python,high-precision,streaming,defi` + `ruff check --fix`), `make check-all-targets` (clippy incl. bins/examples).
- Rust tests use **cargo-nextest**, not `cargo test`. Single crate: `make cargo-test-crate-<crate-name>`; fast local core run: `make cargo-test-core-local`. Custom hooks live in `.pre-commit-hooks/` (cargo conventions, copyright year, etc.).
- Default precision mode is `high-precision` (`PriceRaw`/`QuantityRaw` = u128); some targets run standard precision too.
- Python tests: `make pytest` (v1, root venv) vs `make pytest-v2` (v2, `python/` venv). Integration services (postgres/redis/pgadmin): `make init-services`, then `make init-db` (schema in `schema/sql/`).

## Python v1 vs v2 — do not mix them

- v1: Cython package at repo root, `uv.lock` at root; build with `make build-debug` (runs `build.py`, builds the whole package — slow). Env vars `PYO3_PYTHON`/`LD_LIBRARY_PATH`/`PYTHONHOME` must point at the root `.venv` (see `docs/developer_guide/environment_setup.md`).
- v2: PyO3 package in `python/` with its own `python/uv.lock`; build with `make build-debug-v2` (maturin, target dir `target-v2`). `make py-stubs-v2` regenerates committed type stubs — run and commit it whenever Rust/PyO3 bindings change. v2 make targets run with `VIRTUAL_ENV` unset internally; if you run uv inside `python/` manually, unset it yourself or you'll hit the v1 venv.

## Generated artifacts

- Cap'n Proto code in `crates/serialization/generated/capnp` is generated: `make regen-capnp`; verify with `make check-capnp-schemas`.
- Rust conventions (manifest layout, feature flags, adapter dep placement) are pre-commit-enforced and documented in `docs/developer_guide/rust.md`. Docs use sentence case for H2+ headings.

## Repo quirks

- `*.sh` is gitignored except an explicit whitelist — new shell scripts must be `git add -f`.
- Also gitignored: `*.bak`, `*.sqlite`, `logs/`, `*.env`, `python/nautilus_trader/_libnautilus.pyi`.
- Never update `RELEASES.md` (maintainers keep it current). Keep PRs small and reference the issue; CI runs the full suite.
