# nautilus-dydx

[![build](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml/badge.svg?branch=master)](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml)
[![Documentation](https://img.shields.io/docsrs/nautilus-dydx)](https://docs.rs/nautilus-dydx/latest/nautilus_dydx/)
[![crates.io version](https://img.shields.io/crates/v/nautilus-dydx.svg)](https://crates.io/crates/nautilus-dydx)
![license](https://img.shields.io/github/license/nautechsystems/nautilus_trader?color=blue)
[![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?logo=discord&logoColor=white)](https://discord.gg/NautilusTrader)

[NautilusTrader](https://nautilustrader.io) adapter for the [dYdX v4](https://dydx.trade) decentralized exchange.

The `nautilus-dydx` crate provides client bindings (HTTP, WebSocket & gRPC) and data models
for the official **dYdX v4 API**.

dYdX v4 is built as a standalone Cosmos SDK appchain using CometBFT consensus. The order book
and matching engine run on-chain as part of the validator process. Orders are submitted as
Cosmos transactions via gRPC and settled each block. An Indexer service exposes REST and
WebSocket APIs for market data and account state.

The official dYdX API reference can be found at <https://docs.dydx.xyz/>.

## NautilusTrader

[NautilusTrader](https://nautilustrader.io) is an open-source, production-grade, Rust-native trading engine
with a deterministic, event-driven architecture for multi-asset, multi-venue systems.

The system spans research, simulation, and live execution, providing research-to-live semantic parity.

## Configuration

See the [integration guide](../../../docs/integrations/dydx.md) for client settings,
credentials, and connection setup.

## Feature flags

Feature flags control code inclusion for Rust builds and Python bindings:

- `examples`: Enables the crate's example binaries.
- `extension-module`: Builds as a Python extension module.
- `high-precision` (default): Enables
  [high-precision mode](https://nautilustrader.io/docs/nightly/getting_started/installation/#precision-mode)
  to use 128-bit value types.
- `python`: Enables Python bindings from [PyO3](https://pyo3.rs).

## Integrator attribution

Orders built by the gRPC `OrderBuilder` set `order_router_address` to NautilusTrader's address.

## Documentation

- [Rust API reference](https://docs.rs/nautilus-dydx)
- [Integration guide](../../../docs/integrations/dydx.md)
- [Rust examples](examples/)
- [Python examples](../../../examples/live/dydx/)

## License

The source code for NautilusTrader is available on GitHub under the [GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).

---

NautilusTrader™ is developed and maintained by Nautech Systems, a technology
company specializing in the development of high-performance trading systems.
For more information, visit <https://nautilustrader.io>.

Use of this software is subject to the [Disclaimer](https://nautilustrader.io/legal/disclaimer/).

<img src="https://github.com/nautechsystems/nautilus_trader/raw/develop/assets/nautilus-logo-white.png" alt="logo" width="300" height="auto"/>

© 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
