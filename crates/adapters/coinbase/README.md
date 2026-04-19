# nautilus-coinbase

[![build](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml/badge.svg?branch=master)](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml)
![license](https://img.shields.io/github/license/nautechsystems/nautilus_trader?color=blue)
[![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?logo=discord&logoColor=white)](https://discord.gg/NautilusTrader)

[NautilusTrader](https://nautilustrader.io) adapter for the [Coinbase Advanced Trade](https://docs.cdp.coinbase.com/coinbase-app/docs/advanced-trade-apis) API.

The `nautilus-coinbase` crate provides client bindings (HTTP & WebSocket), data
models and helper utilities that wrap the official **Coinbase Advanced Trade API**
for spot, futures, and perpetual markets.

Components:

- `CoinbaseHttpClient`: Low-level HTTP API connectivity with ES256 JWT signing.
- `CoinbaseWebSocketClient`: Low-level WebSocket connectivity (market data and
  authenticated user channels).
- `CoinbaseInstrumentProvider`: Instrument loading and parsing.
- `CoinbaseDataClient`: Market data feed manager.
- `CoinbaseDataClientFactory`: Data client factory.

Pending (tracked for follow-up work):

- `CoinbaseExecutionClient` and `CoinbaseExecutionClientFactory`.

The adapter is consumed by the v2 system. Configurations and enums are exported
through PyO3 (`nautilus_pyo3.coinbase`); there is no legacy Python `TradingNode`
integration.

See the
[integration guide](https://nautilustrader.io/docs/nightly/integrations/coinbase)
for capabilities, symbology, configuration tables, and the current adapter
status.

## NautilusTrader

[NautilusTrader](https://nautilustrader.io) is an open-source, production-grade, Rust-native
engine for multi-asset, multi-venue trading systems.

The system spans research, deterministic simulation, and live execution within a single
event-driven architecture, providing research-to-live semantic parity.

## Feature flags

This crate provides feature flags to control source code inclusion during compilation:

- `python`: Enables Python bindings from [PyO3](https://pyo3.rs).
- `extension-module`: Builds as a Python extension module.

[High-precision mode](https://nautilustrader.io/docs/nightly/getting_started/installation#precision-mode) (128-bit value types) is enabled by default.

## License

The source code for NautilusTrader is available on GitHub under the [GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).

---

NautilusTrader™ is developed and maintained by Nautech Systems, a technology
company specializing in the development of high-performance trading systems.
For more information, visit <https://nautilustrader.io>.

Use of this software is subject to the [Disclaimer](https://nautilustrader.io/legal/disclaimer/).

<img src="https://github.com/nautechsystems/nautilus_trader/raw/develop/assets/nautilus-logo-white.png" alt="logo" width="300" height="auto"/>

© 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
