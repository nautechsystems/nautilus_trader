# nautilus-pyo3

Python bindings for [NautilusTrader](http://nautilustrader.io).

The `nautilus-pyo3` crate provides all [PyO3](https://pyo3.rs) Python bindings for the
main `nautilus_trader` Python package, built via [maturin](https://github.com/PyO3/maturin).

## Platform

[NautilusTrader](http://nautilustrader.io) is an open-source, high-performance, production-grade
algorithmic trading platform, providing quantitative traders with the ability to backtest
portfolios of automated trading strategies on historical data with an event-driven engine,
and also deploy those same strategies live, with no code changes.

NautilusTrader's design, architecture, and implementation philosophy prioritizes software correctness and safety at the
highest level, with the aim of supporting mission-critical, trading system backtesting and live deployment workloads.

## Feature flags

This crate provides feature flags to control source code inclusion during compilation:

- `extension-module`: Builds as a Python extension module (automatically enabled by `maturin`).
- `ffi`: Enables the C foreign function interface (FFI) support in dependent crates.
- `high-precision`: Uses 128-bit value types throughout the workspace.
- `cython-compat`: Adjusts the module name so it can be imported from Cython generated code.
- `postgres`: Enables PostgreSQL (sqlx) back-ends in dependent crates.
- `redis`: Enables Redis based infrastructure in dependent crates.
- `hypersync`: Enables hypersync support (fast parallel hash maps) where available.
- `tracing-bridge`: Enables the `tracing` subscriber bridge for log integration.
- `defi`: Enables DeFi (Decentralized Finance) support including blockchain adapters.

## License

The source code for NautilusTrader is available on GitHub under the [GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).

---

NautilusTrader™ is developed and maintained by Nautech Systems, a technology
company specializing in the development of high-performance trading systems.
For more information, visit <https://nautilustrader.io>.

Use of this software is subject to the [Disclaimer](https://nautilustrader.io/legal/disclaimer/).

<img src="https://github.com/nautechsystems/nautilus_trader/raw/develop/assets/nautilus-logo-white.png" alt="logo" width="300" height="auto"/>

© 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
