# nautilus-indicators

[![build](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml/badge.svg?branch=master)](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml)
[![Documentation](https://img.shields.io/docsrs/nautilus-indicators)](https://docs.rs/nautilus-indicators/latest/nautilus-indicators/)
[![crates.io version](https://img.shields.io/crates/v/nautilus-indicators.svg)](https://crates.io/crates/nautilus-indicators)
![license](https://img.shields.io/github/license/nautechsystems/nautilus_trader?color=blue)
[![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?logo=discord&logoColor=white)](https://discord.gg/NautilusTrader)

Technical analysis indicators for [NautilusTrader](http://nautilustrader.io).

The `nautilus-indicators` crate provides a comprehensive collection of technical analysis indicators
for quantitative trading and market research. This includes a wide variety of indicators
organized by category, with a unified trait-based architecture for consistent usage:

- **Moving averages**: SMA, EMA, DEMA, HMA, WMA, VWAP, adaptive averages, and linear regression.
- **Momentum indicators**: RSI, MACD, Aroon, Bollinger Bands, CCI, Stochastics, and rate of change.
- **Volatility indicators**: ATR, Donchian Channels, Keltner Channels, and volatility ratios.
- **Ratio analysis**: Efficiency ratios and spread analysis for relative performance.
- **Order book indicators**: Book imbalance ratio for analyzing market microstructure.
- **Common indicator trait**: Unified interface supporting bars, quotes, trades, and order book data.

All indicators are designed for high-performance real-time processing with bounded memory
usage and efficient circular buffer implementations. The crate supports both Rust-native
usage and Python integration for strategy development and backtesting.

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

## Documentation

See [the docs](https://docs.rs/nautilus-indicators) for more detailed usage.

## License

The source code for NautilusTrader is available on GitHub under the [GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).
Contributions to the project are welcome and require the completion of a standard [Contributor License Agreement (CLA)](https://github.com/nautechsystems/nautilus_trader/blob/develop/CLA.md).

---

NautilusTrader™ is developed and maintained by Nautech Systems, a technology
company specializing in the development of high-performance trading systems.
For more information, visit <https://nautilustrader.io>.

<img src="https://github.com/nautechsystems/nautilus_trader/raw/develop/assets/nautilus-logo-white.png" alt="logo" width="400" height="auto"/>

© 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
