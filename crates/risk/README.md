# nautilus-risk

[![build](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml/badge.svg?branch=master)](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml)
[![Documentation](https://img.shields.io/docsrs/nautilus-risk)](https://docs.rs/nautilus-risk/latest/nautilus-risk/)
[![crates.io version](https://img.shields.io/crates/v/nautilus-risk.svg)](https://crates.io/crates/nautilus-risk)
![license](https://img.shields.io/github/license/nautechsystems/nautilus_trader?color=blue)
[![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?logo=discord&logoColor=white)](https://discord.gg/NautilusTrader)

Risk engine for [NautilusTrader](http://nautilustrader.io).

The `nautilus-risk` crate provides comprehensive risk management capabilities including pre-trade
order validation, position sizing calculations, and trading controls. This system ensures
trading operations remain within defined risk parameters and regulatory constraints:

- **Risk engine**: Central risk management orchestration with configurable trading states.
- **Order validation**: Pre-trade checks for price, quantity, notional limits, and market conditions.
- **Position sizing**: Fixed-risk position sizing calculations with commission and exchange rate support.
- **Trading controls**: Rate limiting, balance validation, and exposure management.
- **Account protection**: Multi-currency balance checks and margin requirement validation.

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

## Documentation

See [the docs](https://docs.rs/nautilus-risk) for more detailed usage.

## License

The source code for NautilusTrader is available on GitHub under the [GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).
Contributions to the project are welcome and require the completion of a standard [Contributor License Agreement (CLA)](https://github.com/nautechsystems/nautilus_trader/blob/develop/CLA.md).

---

NautilusTrader™ is developed and maintained by Nautech Systems, a technology
company specializing in the development of high-performance trading systems.
For more information, visit <https://nautilustrader.io>.

<img src="https://nautilustrader.io/nautilus-logo-white.png" alt="logo" width="400" height="auto"/>

<span style="font-size: 0.8em; color: #999;">© 2015-2025 Nautech Systems Pty Ltd. All rights reserved.</span>
