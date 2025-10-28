# nautilus-serialization

[![build](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml/badge.svg?branch=master)](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml)
[![Documentation](https://img.shields.io/docsrs/nautilus-serialization)](https://docs.rs/nautilus-serialization/latest/nautilus-serialization/)
[![crates.io version](https://img.shields.io/crates/v/nautilus-serialization.svg)](https://crates.io/crates/nautilus-serialization)
![license](https://img.shields.io/github/license/nautechsystems/nautilus_trader?color=blue)
[![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?logo=discord&logoColor=white)](https://discord.gg/NautilusTrader)

Data serialization and format conversion for [NautilusTrader](http://nautilustrader.io).

The `nautilus-serialization` crate provides comprehensive data serialization capabilities for converting
trading data between different formats including Apache Arrow, Parquet, and custom schemas.
This enables efficient data storage, retrieval, and interoperability across different systems:

- **Apache Arrow integration**: Schema definitions and encoding/decoding for market data types.
- **Parquet file operations**: High-performance columnar storage for historical data analysis.
- **Record batch processing**: Efficient batch operations for time-series data.
- **Schema management**: Type-safe schema definitions with metadata preservation.
- **Cross-format conversion**: Seamless data interchange between Arrow, Parquet, and native types.

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

- `python`: Enables Python bindings from [PyO3](https://pyo3.rs).
- `extension-module`: Builds as a Python extension module (used with `python`).
- `high-precision`: Enables [high-precision mode](https://nautilustrader.io/docs/nightly/getting_started/installation#precision-mode) to use 128-bit value types.

## Documentation

See [the docs](https://docs.rs/nautilus-serialization) for more detailed usage.

## License

The source code for NautilusTrader is available on GitHub under the [GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).
Contributions to the project are welcome and require the completion of a standard [Contributor License Agreement (CLA)](https://github.com/nautechsystems/nautilus_trader/blob/develop/CLA.md).

---

NautilusTrader™ is developed and maintained by Nautech Systems, a technology
company specializing in the development of high-performance trading systems.
For more information, visit <https://nautilustrader.io>.

<img src="https://github.com/nautechsystems/nautilus_trader/raw/develop/assets/nautilus-logo-white.png" alt="logo" width="400" height="auto"/>

© 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
