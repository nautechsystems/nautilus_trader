# nautilus-cryptography

[![build](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml/badge.svg?branch=master)](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml)
[![Documentation](https://img.shields.io/docsrs/nautilus-cryptography)](https://docs.rs/nautilus-cryptography/latest/nautilus-cryptography/)
[![crates.io version](https://img.shields.io/crates/v/nautilus-cryptography.svg)](https://crates.io/crates/nautilus-cryptography)
![license](https://img.shields.io/github/license/nautechsystems/nautilus_trader?color=blue)
[![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?logo=discord&logoColor=white)](https://discord.gg/NautilusTrader)

Cryptographic utilities and security functions for [NautilusTrader](http://nautilustrader.io).

The `nautilus-cryptography` crate provides essential cryptographic primitives and security utilities
required for secure communication with trading venues and data providers. This includes
digital signing, TLS configuration, and cryptographic provider management:

- HMAC-based message authentication and signing.
- Digital signatures using RSA and Ed25519 algorithms.
- TLS client configuration with platform certificate verification.
- Cryptographic provider management and initialization.
- Secure encoding and decoding utilities.

## Platform

[NautilusTrader](http://nautilustrader.io) is an open-source, high-performance, production-grade
algorithmic trading platform, providing quantitative traders with the ability to backtest
portfolios of automated trading strategies on historical data with an event-driven engine,
and also deploy those same strategies live, with no code changes.

NautilusTrader's design, architecture, and implementation philosophy prioritizes software correctness and safety at the
highest level, with the aim of supporting mission-critical, trading system backtesting and live deployment workloads.

## Feature flags

This crate provides feature flags to control source code inclusion during compilation:

- `python`: Enables Python bindings from [PyO3](https://pyo3.rs).

## Documentation

See [the docs](https://docs.rs/nautilus-cryptography) for more detailed usage.

## License

The source code for NautilusTrader is available on GitHub under the [GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).

---

NautilusTrader™ is developed and maintained by Nautech Systems, a technology
company specializing in the development of high-performance trading systems.
For more information, visit <https://nautilustrader.io>.

Use of this software is subject to the [Disclaimer](https://nautilustrader.io/legal/disclaimer/).

<img src="https://github.com/nautechsystems/nautilus_trader/raw/develop/assets/nautilus-logo-white.png" alt="logo" width="300" height="auto"/>

© 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
