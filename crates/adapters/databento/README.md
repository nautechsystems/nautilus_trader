# nautilus-databento

[![build](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml/badge.svg?branch=master)](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml)
[![Documentation](https://img.shields.io/docsrs/nautilus-databento)](https://docs.rs/nautilus-databento/latest/nautilus_databento/)
[![crates.io version](https://img.shields.io/crates/v/nautilus-databento.svg)](https://crates.io/crates/nautilus-databento)
![license](https://img.shields.io/github/license/nautechsystems/nautilus_trader?color=blue)
[![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?logo=discord&logoColor=white)](https://discord.gg/NautilusTrader)

[NautilusTrader](https://nautilustrader.io) adapter for [Databento](https://databento.com).

The `nautilus-databento` crate provides live and historical market data across venues and asset classes
through Databento's official Rust client. It loads Databento Binary Encoding (DBN) files, decodes
market events and instrument definitions into Nautilus types, and supports live-node integration
and optional Python bindings. It provides no order execution.

The official Databento API reference can be found at <https://databento.com/docs>.

## NautilusTrader

[NautilusTrader](https://nautilustrader.io) is an open-source, production-grade, Rust-native trading engine
with a deterministic, event-driven architecture for multi-asset, multi-venue systems.

The system spans research, simulation, and live execution, providing research-to-live semantic parity.

## Configuration

See the [integration guide](../../../docs/integrations/databento.md) for client settings,
credentials, and connection setup.

## Feature flags

Feature flags control code inclusion for Rust builds and Python bindings:

- `arrow`: Enables Apache Arrow data support.
- `extension-module`: Builds as a Python extension module.
- `high-precision` (default): Enables
  [high-precision mode](https://nautilustrader.io/docs/nightly/getting_started/installation/#precision-mode)
  to use 128-bit value types.
- `live` (default): Enables live data functionality, including the `data`, `factories`, and `live` modules.
- `python`: Enables Python bindings from [PyO3](https://pyo3.rs).

## Documentation

- [Rust API reference](https://docs.rs/nautilus-databento)
- [Integration guide](../../../docs/integrations/databento.md)
- [Rust examples](examples/)
- [Python examples](../../../examples/live/databento/)

## License

The source code for NautilusTrader is available on GitHub under the [GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).

---

NautilusTrader™ is developed and maintained by Nautech Systems, a technology
company specializing in the development of high-performance trading systems.
For more information, visit <https://nautilustrader.io>.

Use of this software is subject to the [Disclaimer](https://nautilustrader.io/legal/disclaimer/).

<img src="https://github.com/nautechsystems/nautilus_trader/raw/develop/assets/nautilus-logo-white.png" alt="logo" width="300" height="auto"/>

© 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
