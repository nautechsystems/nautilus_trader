# nautilus-persistence

[![build](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml/badge.svg?branch=master)](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml)
[![Documentation](https://img.shields.io/docsrs/nautilus-persistence)](https://docs.rs/nautilus-persistence/latest/nautilus-persistence/)
[![crates.io version](https://img.shields.io/crates/v/nautilus-persistence.svg)](https://crates.io/crates/nautilus-persistence)
![license](https://img.shields.io/github/license/nautechsystems/nautilus_trader?color=blue)
[![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?logo=discord&logoColor=white)](https://discord.gg/NautilusTrader)

Data persistence and storage for [NautilusTrader](https://nautilustrader.io).

The `nautilus-persistence` crate provides data persistence capabilities including reading and writing
trading data to various storage backends. This includes Apache Parquet file support, streaming data
pipelines, and cloud storage integration for historical data management.

## NautilusTrader

[NautilusTrader](https://nautilustrader.io) is an open-source, production-grade, Rust-native
engine for multi-asset, multi-venue trading systems.

The system spans research, deterministic simulation, and live execution within a single
event-driven architecture, providing research-to-live semantic parity.

## Feature flags

This crate provides feature flags to control source code inclusion during compilation:

- `cloud`: Enables cloud storage backends (S3, Azure, GCP, HTTP) via `object_store`.
- `ffi`: Enables the C foreign function interface (FFI) from [cbindgen](https://github.com/mozilla/cbindgen).
- `python`: Enables Python bindings from [PyO3](https://pyo3.rs) (auto-enables `cloud`).
- `high-precision`: Enables [high-precision mode](https://nautilustrader.io/docs/nightly/getting_started/installation#precision-mode) to use 128-bit value types.
- `extension-module`: Builds as a Python extension module.

## Documentation

See [the docs](https://docs.rs/nautilus-persistence) for more detailed usage.

## License

The source code for NautilusTrader is available on GitHub under the [GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).

---

NautilusTrader™ is developed and maintained by Nautech Systems, a technology
company specializing in the development of high-performance trading systems.
For more information, visit <https://nautilustrader.io>.

Use of this software is subject to the [Disclaimer](https://nautilustrader.io/legal/disclaimer/).

<img src="https://github.com/nautechsystems/nautilus_trader/raw/develop/assets/nautilus-logo-white.png" alt="logo" width="300" height="auto"/>

© 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
