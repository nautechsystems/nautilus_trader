# nautilus-pyo3

A temporary crate to provide all Python bindings for the main `nautilus_trader` Python package.

## Platform

[NautilusTrader](http://nautilustrader.io) is an open-source, high-performance, production-grade
algorithmic trading platform, providing quantitative traders with the ability to backtest
portfolios of automated trading strategies on historical data with an event-driven engine,
and also deploy those same strategies live, with no code changes.

NautilusTrader's design, architecture, and implementation philosophy prioritizes software correctness and safety at the
highest level, with the aim of supporting mission-critical, trading system backtesting and live deployment workloads.

## Feature flags

This crate is primarily intended to be built for Python via
[maturin](https://github.com/PyO3/maturin) and therefore provides a broad set of feature flags
to toggle bindings and optional dependencies:

- `extension-module`: Builds the crate as a Python extension module (automatically enabled by `maturin`).
- `ffi`: Enables the C foreign function interface (FFI) support in dependent crates.
- `high-precision`: Uses 128-bit value types throughout the workspace.
- `cython-compat`: Adjusts the module name so it can be imported from Cython generated code.
- `postgres`: Enables PostgreSQL (sqlx) back-ends in dependent crates.
- `redis`: Enables Redis based infrastructure in dependent crates.
- `hypersync`: Enables hypersync support (fast parallel hash maps) where available.
