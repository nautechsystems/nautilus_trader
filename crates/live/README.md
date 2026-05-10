# nautilus-live

[![build](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml/badge.svg?branch=master)](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml)
[![Documentation](https://img.shields.io/docsrs/nautilus-live)](https://docs.rs/nautilus-live/latest/nautilus-live/)
[![crates.io version](https://img.shields.io/crates/v/nautilus-live.svg)](https://crates.io/crates/nautilus-live)
![license](https://img.shields.io/github/license/nautechsystems/nautilus_trader?color=blue)
[![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?logo=discord&logoColor=white)](https://discord.gg/NautilusTrader)

Live system node for [NautilusTrader](https://nautilustrader.io).

The `nautilus-live` crate provides high-level abstractions and infrastructure for running live trading
systems, including data streaming, execution management, and system lifecycle handling.
It builds on top of the system kernel to provide simplified interfaces for live deployment:

- `LiveNode` High-level abstraction for live system nodes.
- `LiveNodeConfig` Configuration for live node deployment.
- `AsyncRunner` for managing system real-time data flow.

## NautilusTrader

[NautilusTrader](https://nautilustrader.io) is an open-source, production-grade, Rust-native
engine for multi-asset, multi-venue trading systems.

The system spans research, deterministic simulation, and live execution within a single
event-driven architecture, providing research-to-live semantic parity.

## Feature flags

This crate provides feature flags to control source code inclusion during compilation:

- `ffi`: Enables the C foreign function interface (FFI) from [cbindgen](https://github.com/mozilla/cbindgen).
- `streaming`: Enables `persistence` dependency for streaming configuration.
- `python`: Enables Python bindings from [PyO3](https://pyo3.rs) (auto-enables `streaming`).
- `defi`: Enables DeFi (Decentralized Finance) support.
- `extension-module`: Builds as a Python extension module.

## Documentation

See [the docs](https://docs.rs/nautilus-live) for more detailed usage.

## License

The source code for NautilusTrader is available on GitHub under the [GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).

---

NautilusTrader™ is developed and maintained by Nautech Systems, a technology
company specializing in the development of high-performance trading systems.
For more information, visit <https://nautilustrader.io>.

Use of this software is subject to the [Disclaimer](https://nautilustrader.io/legal/disclaimer/).

<img src="https://github.com/nautechsystems/nautilus_trader/raw/develop/assets/nautilus-logo-white.png" alt="logo" width="300" height="auto"/>

© 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
