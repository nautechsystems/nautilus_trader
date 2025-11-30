# nautilus-infrastructure

[![build](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml/badge.svg?branch=master)](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml)
[![Documentation](https://img.shields.io/docsrs/nautilus-infrastructure)](https://docs.rs/nautilus-infrastructure/latest/nautilus-infrastructure/)
[![crates.io version](https://img.shields.io/crates/v/nautilus-infrastructure.svg)](https://crates.io/crates/nautilus-infrastructure)
![license](https://img.shields.io/github/license/nautechsystems/nautilus_trader?color=blue)
[![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?logo=discord&logoColor=white)](https://discord.gg/NautilusTrader)

Database and messaging infrastructure for [NautilusTrader](http://nautilustrader.io).

The `nautilus-infrastructure` crate provides backend database implementations and message bus adapters
that enable NautilusTrader to scale from development to production deployments. This includes
enterprise-grade data persistence and messaging capabilities:

- **Redis integration**: Cache database and message bus implementations using Redis.
- **PostgreSQL integration**: SQL-based cache database with comprehensive data models.
- **Connection management**: Robust connection handling with retry logic and health monitoring.
- **Serialization options**: Support for JSON and MessagePack encoding formats.
- **Python bindings**: PyO3 integration for seamless Python interoperability.

The crate supports multiple database backends through feature flags, allowing users to choose
the appropriate infrastructure components for their specific deployment requirements and scale.

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
- `redis`: Enables the Redis cache database and message bus backing implementations.
- `sql`: Enables the SQL models and cache database.
- `extension-module`: Builds as a Python extension module (used with `python`).

## Documentation

See [the docs](https://docs.rs/nautilus-infrastructure) for more detailed usage.

## License

The source code for NautilusTrader is available on GitHub under the [GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).
Contributions to the project are welcome and require the completion of a standard [Contributor License Agreement (CLA)](https://github.com/nautechsystems/nautilus_trader/blob/develop/CLA.md).

---

NautilusTrader™ is developed and maintained by Nautech Systems, a technology
company specializing in the development of high-performance trading systems.
For more information, visit <https://nautilustrader.io>.

<img src="https://github.com/nautechsystems/nautilus_trader/raw/develop/assets/nautilus-logo-white.png" alt="logo" width="400" height="auto"/>

© 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
