# nautilus-common

[![build](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml/badge.svg?branch=master)](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml)
[![Documentation](https://img.shields.io/docsrs/nautilus-common)](https://docs.rs/nautilus-common/latest/nautilus-common/)
[![crates.io version](https://img.shields.io/crates/v/nautilus-common.svg)](https://crates.io/crates/nautilus-common)
![license](https://img.shields.io/github/license/nautechsystems/nautilus_trader?color=blue)
[![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?logo=discord&logoColor=white)](https://discord.gg/NautilusTrader)

Common componentry for [NautilusTrader](http://nautilustrader.io).

The *common* crate provides shared components and utilities that form the system foundation for
NautilusTrader applications. This includes the actor system, message bus, caching layer, and other
essential services.

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
- `defi`: Enables DeFi (Decentralized Finance) support.

## Documentation

See [the docs](https://docs.rs/nautilus-common) for more detailed usage.

## License

The source code for NautilusTrader is available on GitHub under the [GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).
Contributions to the project are welcome and require the completion of a standard [Contributor License Agreement (CLA)](https://github.com/nautechsystems/nautilus_trader/blob/develop/CLA.md).

---

NautilusTrader™ is developed and maintained by Nautech Systems, a technology
company specializing in the development of high-performance trading systems.
For more information, visit <https://nautilustrader.io>.

<img src="https://nautilustrader.io/nautilus-logo-white.png" alt="logo" width="400" height="auto"/>

<span style="font-size: 0.8em; color: #999;">© 2015-2025 Nautech Systems Pty Ltd. All rights reserved.</span>
