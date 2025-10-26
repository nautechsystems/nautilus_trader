# nautilus-network

[![build](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml/badge.svg?branch=master)](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml)
[![Documentation](https://img.shields.io/docsrs/nautilus-network)](https://docs.rs/nautilus-network/latest/nautilus-network/)
[![crates.io version](https://img.shields.io/crates/v/nautilus-network.svg)](https://crates.io/crates/nautilus-network)
![license](https://img.shields.io/github/license/nautechsystems/nautilus_trader?color=blue)
[![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?logo=discord&logoColor=white)](https://discord.gg/NautilusTrader)

Network functionality for [NautilusTrader](http://nautilustrader.io).

The `nautilus-network` crate provides networking components including HTTP, WebSocket, and raw TCP socket
clients, rate limiting, backoff strategies, and socket TLS utilities for connecting to
trading venues and data providers.

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
- `extension-module`: Builds the crate as a Python extension module.
- `turmoil`: Enables deterministic network simulation testing with [turmoil](https://github.com/tokio-rs/turmoil).

## Testing

The crate includes both standard integration tests and deterministic network simulation tests using turmoil.

To run standard tests:

```bash
cargo nextest run -p nautilus-network
```

To run turmoil network simulation tests:

```bash
cargo nextest run -p nautilus-network --features turmoil
```

The turmoil tests simulate various network conditions (reconnections, partitions, etc.) in a deterministic way,
allowing reliable testing of network failure scenarios without flakiness.

## Documentation

See [the docs](https://docs.rs/nautilus-network) for more detailed usage.

## License

The source code for NautilusTrader is available on GitHub under the [GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).
Contributions to the project are welcome and require the completion of a standard [Contributor License Agreement (CLA)](https://github.com/nautechsystems/nautilus_trader/blob/develop/CLA.md).

---

NautilusTrader™ is developed and maintained by Nautech Systems, a technology
company specializing in the development of high-performance trading systems.
For more information, visit <https://nautilustrader.io>.

<img src="https://nautilustrader.io/nautilus-logo-white.png" alt="logo" width="400" height="auto"/>

<span style="font-size: 0.8em; color: #999;">© 2015-2025 Nautech Systems Pty Ltd. All rights reserved.</span>
