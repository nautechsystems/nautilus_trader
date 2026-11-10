# nautilus-kraken

[![build](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml/badge.svg?branch=master)](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml)
[![Documentation](https://img.shields.io/docsrs/nautilus-kraken)](https://docs.rs/nautilus-kraken/latest/nautilus_kraken/)
[![crates.io version](https://img.shields.io/crates/v/nautilus-kraken.svg)](https://crates.io/crates/nautilus-kraken)
![license](https://img.shields.io/github/license/nautechsystems/nautilus_trader?color=blue)
[![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?logo=discord&logoColor=white)](https://discord.gg/NautilusTrader)

[NautilusTrader](https://nautilustrader.io) adapter for the [Kraken](https://www.kraken.com) exchange.

The `nautilus-kraken` crate provides separate Spot and Futures data and execution clients over HTTP
and WebSocket. It includes typed API models, instrument loading, order and account updates, and
live-node factories for both markets.

The official Kraken API reference can be found at <https://docs.kraken.com/>.

## NautilusTrader

[NautilusTrader](https://nautilustrader.io) is an open-source, production-grade, Rust-native trading engine
with a deterministic, event-driven architecture for multi-asset, multi-venue systems.

The system spans research, simulation, and live execution, providing research-to-live semantic parity.

## Features

- HTTP REST API clients for market data (Spot v2, Futures v3).
- WebSocket clients for real-time data feeds (Spot v2, Futures).
- Support for both Spot and Futures markets.
- Instrument, ticker, trade, orderbook, and OHLC data.
- Prepared for execution support (orders, positions, balances) - WIP.

## Architecture

This crate provides **separate HTTP and WebSocket clients for Spot and Futures markets**.
This design reflects fundamental differences between the two APIs:

| Aspect         | Spot                  | Futures                      |
| -------------- | --------------------- | ---------------------------- |
| API Version    | REST API v2           | Derivatives API v3           |
| Base URL       | `api.kraken.com`      | `futures.kraken.com`         |
| Auth Headers   | `API-Key`, `API-Sign` | `APIKey`, `Authent`, `Nonce` |
| Request Format | URL-encoded form      | JSON body                    |
| WebSocket      | v2 protocol           | Futures-specific protocol    |

Kraken Futures was originally a separate platform (Crypto Facilities) acquired by Kraken,
which explains why the APIs remain distinct rather than unified.

### Client Types

- **`KrakenSpotHttpClient`** / **`KrakenSpotWebSocketClient`**: For spot trading pairs (e.g., `BTC/USD`, `ETH/EUR`).
- **`KrakenFuturesHttpClient`** / **`KrakenFuturesWebSocketClient`**: For perpetual and fixed-maturity futures (e.g., `PF_XBTUSD`, `PI_ETHUSD`).

### Bitcoin symbol format

| Market  | Format | Example     | Notes                                           |
| ------- | ------ | ----------- | ----------------------------------------------- |
| Spot    | `BTC`  | `BTC/USD`   | XBT normalized to BTC (base or quote position). |
| Futures | `XBT`  | `PI_XBTUSD` | Uses Kraken's native XBT format.                |

## Configuration

See the [integration guide](../../../docs/integrations/kraken.md) for client settings,
credentials, and connection setup.

## Feature flags

Feature flags control code inclusion for Rust builds and Python bindings:

- `examples`: Enables the crate's example binaries.
- `extension-module`: Builds as a Python extension module.
- `high-precision` (default): Enables
  [high-precision mode](https://nautilustrader.io/docs/nightly/getting_started/installation/#precision-mode)
  to use 128-bit value types. Keep this default for Futures because Kraken may return instrument
  precision above standard-precision mode's nine-decimal limit.
- `python`: Enables Python bindings from [PyO3](https://pyo3.rs).

## Integrator attribution

Spot and Futures HTTP order builders set `broker` to NautilusTrader's integration ID.
Raw HTTP clients preserve caller-provided parameters, and Spot WebSocket orders omit attribution.

## Examples

See the `bin/` directory for example usage:

```bash
cargo run --bin kraken-http-spot-raw
cargo run --bin kraken-http-spot-public
cargo run --bin kraken-ws-spot-data
```

## Documentation

- [Rust API reference](https://docs.rs/nautilus-kraken)
- [Integration guide](../../../docs/integrations/kraken.md)
- [Rust examples](examples/)
- [Python examples](../../../examples/live/kraken/)

## License

The source code for NautilusTrader is available on GitHub under the [GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).

---

NautilusTrader™ is developed and maintained by Nautech Systems, a technology
company specializing in the development of high-performance trading systems.
For more information, visit <https://nautilustrader.io>.

Use of this software is subject to the [Disclaimer](https://nautilustrader.io/legal/disclaimer/).

<img src="https://github.com/nautechsystems/nautilus_trader/raw/develop/assets/nautilus-logo-white.png" alt="logo" width="300" height="auto"/>

© 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
