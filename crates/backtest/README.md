# nautilus-backtest

[![build](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml/badge.svg?branch=master)](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml)
[![Documentation](https://img.shields.io/docsrs/nautilus-backtest)](https://docs.rs/nautilus-backtest/latest/nautilus-backtest/)
[![crates.io version](https://img.shields.io/crates/v/nautilus-backtest.svg)](https://crates.io/crates/nautilus-backtest)
![license](https://img.shields.io/github/license/nautechsystems/nautilus_trader?color=blue)
[![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?logo=discord&logoColor=white)](https://discord.gg/NautilusTrader)

Backtest engine for [NautilusTrader](https://nautilustrader.io).

The `nautilus-backtest` crate provides an event-driven backtesting framework that allows
quantitative traders to test and validate trading strategies on historical data with high
fidelity market simulation. The system replicates real market conditions including:

- Event-driven backtesting engine with simulated exchanges.
- Market data replay with configurable latency and fill models.
- Order matching engines with realistic execution simulation.
- Multi-venue and multi-asset backtesting capabilities.
- Configuration and state management.

## NautilusTrader

[NautilusTrader](https://nautilustrader.io) is an open-source, production-grade, Rust-native
engine for multi-asset, multi-venue trading systems.

The system spans research, deterministic simulation, and live execution within a single
event-driven architecture, providing research-to-live semantic parity.

## Feature flags

This crate provides feature flags to control source code inclusion during compilation:

- `defi`: Enables DeFi replay APIs and data-engine routing.
- `examples`: Enables example strategies and the EMA crossover backtest example.
- `extension-module`: Builds as a Python extension module.
- `high-precision`: Enables
  [high-precision mode](https://nautilustrader.io/docs/nightly/getting_started/installation#precision-mode)
  to use 128-bit value types.
- `mimalloc`: Uses [mimalloc](https://github.com/microsoft/mimalloc) as the global allocator for
  bundled Rust examples.
- `plugin`: Provides a compatibility flag without enabling additional code.
- `python`: Enables Python bindings from [PyO3](https://pyo3.rs).
- `streaming`: Enables the `nautilus-persistence` dependency for streaming configuration.

## Documentation

See [the docs](https://docs.rs/nautilus-backtest) for more detailed usage.

## License

The source code for NautilusTrader is available on GitHub under the [GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).

---

NautilusTrader™ is developed and maintained by Nautech Systems, a technology
company specializing in the development of high-performance trading systems.
For more information, visit <https://nautilustrader.io>.

Use of this software is subject to the [Disclaimer](https://nautilustrader.io/legal/disclaimer/).

<img src="https://github.com/nautechsystems/nautilus_trader/raw/develop/assets/nautilus-logo-white.png" alt="logo" width="300" height="auto"/>

© 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
