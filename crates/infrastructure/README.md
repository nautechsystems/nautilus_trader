# nautilus-infrastructure

[![build](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml/badge.svg?branch=master)](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml)
[![Documentation](https://img.shields.io/docsrs/nautilus-infrastructure)](https://docs.rs/nautilus-infrastructure/latest/nautilus-infrastructure/)
[![crates.io version](https://img.shields.io/crates/v/nautilus-infrastructure.svg)](https://crates.io/crates/nautilus-infrastructure)
![license](https://img.shields.io/github/license/nautechsystems/nautilus_trader?color=blue)
[![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?logo=discord&logoColor=white)](https://discord.gg/NautilusTrader)

Database and messaging infrastructure for [NautilusTrader](https://nautilustrader.io).

The `nautilus-infrastructure` crate provides backend database implementations and message bus adapters
that enable NautilusTrader to scale from development to production deployments. This includes
enterprise-grade data persistence and messaging capabilities:

- **Redis integration**: Cache database and message bus implementations using Redis.
- **PostgreSQL integration**: SQL-based cache database with full data models.
- **Connection management**: Connection handling with retry logic and health monitoring.
- **Serialization options**: Support for JSON and MessagePack encoding formats.
- **Python bindings**: PyO3 integration for Python interoperability.

The crate supports multiple database backends through feature flags, allowing users to choose
the appropriate infrastructure components for their specific deployment requirements and scale.

## NautilusTrader

[NautilusTrader](https://nautilustrader.io) is an open-source, production-grade, Rust-native
engine for multi-asset, multi-venue trading systems.

The system spans research, deterministic simulation, and live execution within a single
event-driven architecture, providing research-to-live semantic parity.

## Feature flags

This crate provides feature flags to control source code inclusion during compilation:

- `python`: Enables Python bindings from [PyO3](https://pyo3.rs).
- `redis`: Enables the Redis cache database and message bus backing implementations.
- `postgres`: Enables the PostgreSQL SQLx models and cache database backend.
- `extension-module`: Builds as a Python extension module.

## Documentation

See [the docs](https://docs.rs/nautilus-infrastructure) for more detailed usage.

## License

The source code for NautilusTrader is available on GitHub under the [GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).

---

NautilusTrader™ is developed and maintained by Nautech Systems, a technology
company specializing in the development of high-performance trading systems.
For more information, visit <https://nautilustrader.io>.

Use of this software is subject to the [Disclaimer](https://nautilustrader.io/legal/disclaimer/).

<img src="https://github.com/nautechsystems/nautilus_trader/raw/develop/assets/nautilus-logo-white.png" alt="logo" width="300" height="auto"/>

© 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
