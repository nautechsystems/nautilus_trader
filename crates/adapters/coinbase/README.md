# nautilus-coinbase

[![build](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml/badge.svg?branch=master)](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml)
[![Documentation](https://img.shields.io/docsrs/nautilus-coinbase)](https://docs.rs/nautilus-coinbase/latest/nautilus_coinbase/)
[![crates.io version](https://img.shields.io/crates/v/nautilus-coinbase.svg)](https://crates.io/crates/nautilus-coinbase)
![license](https://img.shields.io/github/license/nautechsystems/nautilus_trader?color=blue)
[![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?logo=discord&logoColor=white)](https://discord.gg/NautilusTrader)

[NautilusTrader](https://nautilustrader.io) adapter for the Coinbase Advanced Trade API.

The `nautilus-coinbase` crate provides instrument loading, live market data, and order execution for
spot, futures, and perpetuals through the **Coinbase Advanced Trade API**. It includes authenticated
HTTP and WebSocket clients, live-node factories, and optional Python bindings.

The official Coinbase Advanced Trade API reference can be found at
<https://docs.cdp.coinbase.com/coinbase-app/advanced-trade-apis/overview>.

## NautilusTrader

[NautilusTrader](https://nautilustrader.io) is an open-source, production-grade, Rust-native trading engine
with a deterministic, event-driven architecture for multi-asset, multi-venue systems.

The system spans research, simulation, and live execution, providing research-to-live semantic parity.

## Configuration

See the [integration guide](../../../docs/integrations/coinbase.md) for client settings,
credentials, and connection setup.

## Feature flags

Feature flags control code inclusion for Rust builds and Python bindings:

- `examples`: Enables the crate's example binaries.
- `extension-module`: Builds as a Python extension module.
- `high-precision` (default): Enables
  [high-precision mode](https://nautilustrader.io/docs/nightly/getting_started/installation/#precision-mode)
  to use 128-bit value types.
- `python`: Enables Python bindings from [PyO3](https://pyo3.rs).

## Documentation

- [Rust API reference](https://docs.rs/nautilus-coinbase)
- [Integration guide](../../../docs/integrations/coinbase.md)
- [Rust examples](examples/)
- [Python examples](../../../examples/live/coinbase/)

## License

The source code for NautilusTrader is available on GitHub under the [GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).

---

NautilusTrader™ is developed and maintained by Nautech Systems, a technology
company specializing in the development of high-performance trading systems.
For more information, visit <https://nautilustrader.io>.

Use of this software is subject to the [Disclaimer](https://nautilustrader.io/legal/disclaimer/).

<img src="https://github.com/nautechsystems/nautilus_trader/raw/develop/assets/nautilus-logo-white.png" alt="logo" width="300" height="auto"/>

© 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
