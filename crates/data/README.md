# nautilus-data

[![build](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml/badge.svg?branch=master)](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml)
[![Documentation](https://img.shields.io/docsrs/nautilus-data)](https://docs.rs/nautilus-data/latest/nautilus-data/)
[![crates.io version](https://img.shields.io/crates/v/nautilus-data.svg)](https://crates.io/crates/nautilus-data)
![license](https://img.shields.io/github/license/nautechsystems/nautilus_trader?color=blue)
[![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?logo=discord&logoColor=white)](https://discord.gg/NautilusTrader)

Data engine and market data processing for [NautilusTrader](http://nautilustrader.io).

The `nautilus-data` crate provides a comprehensive framework for handling market data ingestion,
processing, and aggregation within the NautilusTrader ecosystem. This includes real-time
data streaming, historical data management, and various aggregation methodologies:

- High-performance data engine for orchestrating data operations.
- Data client infrastructure for connecting to market data providers.
- Bar aggregation machinery supporting tick, volume, value, and time-based aggregation.
- Order book management and delta processing capabilities.
- Subscription management and data request handling.
- Configurable data routing and processing pipelines.

## Platform

[NautilusTrader](http://nautilustrader.io) is an open-source, high-performance, production-grade
algorithmic trading platform, providing quantitative traders with the ability to backtest
portfolios of automated trading strategies on historical data with an event-driven engine,
and also deploy those same strategies live, with no code changes.

NautilusTrader's design, architecture, and implementation philosophy prioritizes software correctness and safety at the
highest level, with the aim of supporting mission-critical, trading system backtesting and live deployment workloads.

## Feature flags

This crate provides feature flags to control source code inclusion during compilation,
depending on the intended use case, i.e. whether to provide Python bindings
for the [nautilus_trader](https://pypi.org/project/nautilus_trader) Python package,
or as part of a Rust only build.

- `ffi`: Enables the C foreign function interface (FFI) from [cbindgen](https://github.com/mozilla/cbindgen).
- `python`: Enables Python bindings from [PyO3](https://pyo3.rs).
- `high-precision`: Enables [high-precision mode](https://nautilustrader.io/docs/nightly/getting_started/installation#precision-mode) to use 128-bit value types.
- `defi`: Enables DeFi (Decentralized Finance) support.
- `extension-module`: Builds as a Python extension module (used with `python`).

## Documentation

See [the docs](https://docs.rs/nautilus-data) for more detailed usage.

## License

The source code for NautilusTrader is available on GitHub under the [GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).
Contributions to the project are welcome and require the completion of a standard [Contributor License Agreement (CLA)](https://github.com/nautechsystems/nautilus_trader/blob/develop/CLA.md).

---

NautilusTrader™ is developed and maintained by Nautech Systems, a technology
company specializing in the development of high-performance trading systems.
For more information, visit <https://nautilustrader.io>.

<img src="https://github.com/nautechsystems/nautilus_trader/raw/develop/assets/nautilus-logo-white.png" alt="logo" width="400" height="auto"/>

© 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
