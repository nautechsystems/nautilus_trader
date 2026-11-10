# nautilus-bybit

[![build](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml/badge.svg?branch=master)](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml)
[![Documentation](https://img.shields.io/docsrs/nautilus-bybit)](https://docs.rs/nautilus-bybit/latest/nautilus_bybit/)
[![crates.io version](https://img.shields.io/crates/v/nautilus-bybit.svg)](https://crates.io/crates/nautilus-bybit)
![license](https://img.shields.io/github/license/nautechsystems/nautilus_trader?color=blue)
[![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?logo=discord&logoColor=white)](https://discord.gg/NautilusTrader)

[NautilusTrader](https://nautilustrader.io) adapter for the [Bybit](https://www.bybit.com) exchange.

The `nautilus-bybit` crate provides instrument loading, live market data, and order execution for
Bybit's spot, futures, perpetual, and options markets. It includes typed HTTP and WebSocket clients
for the **Bybit v5 API**, with live-node factories and optional Python bindings.

The official Bybit API reference can be found at <https://bybit-exchange.github.io/docs/v5/intro>.

## NautilusTrader

[NautilusTrader](https://nautilustrader.io) is an open-source, production-grade, Rust-native trading engine
with a deterministic, event-driven architecture for multi-asset, multi-venue systems.

The system spans research, simulation, and live execution, providing research-to-live semantic parity.

## Configuration

See the [integration guide](../../../docs/integrations/bybit.md) for client settings,
credentials, and connection setup.

## Feature flags

Feature flags control code inclusion for Rust builds and Python bindings:

- `arrow`: Enables Apache Arrow data support.
- `examples`: Enables the crate's example binaries.
- `extension-module`: Builds as a Python extension module.
- `high-precision` (default): Enables
  [high-precision mode](https://nautilustrader.io/docs/nightly/getting_started/installation/#precision-mode)
  to use 128-bit value types.
- `python`: Enables Python bindings from [PyO3](https://pyo3.rs).

## Integrator attribution

HTTP requests use `X-Referer`, and WebSocket orders use `Referer`, for NautilusTrader's integration ID.
Post-only orders omit attribution when background detection identifies the account as a market maker.

## Documentation

- [Rust API reference](https://docs.rs/nautilus-bybit)
- [Integration guide](../../../docs/integrations/bybit.md)
- [Rust examples](examples/)
- [Python examples](../../../examples/live/bybit/)

## License

The source code for NautilusTrader is available on GitHub under the [GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).

---

NautilusTrader™ is developed and maintained by Nautech Systems, a technology
company specializing in the development of high-performance trading systems.
For more information, visit <https://nautilustrader.io>.

Use of this software is subject to the [Disclaimer](https://nautilustrader.io/legal/disclaimer/).

<img src="https://github.com/nautechsystems/nautilus_trader/raw/develop/assets/nautilus-logo-white.png" alt="logo" width="300" height="auto"/>

© 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
