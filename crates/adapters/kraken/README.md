# nautilus-kraken

[![build](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml/badge.svg?branch=master)](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml)
[![Documentation](https://img.shields.io/docsrs/nautilus-kraken)](https://docs.rs/nautilus-kraken/latest/nautilus-kraken/)
[![crates.io version](https://img.shields.io/crates/v/nautilus-kraken.svg)](https://crates.io/crates/nautilus-kraken)
![license](https://img.shields.io/github/license/nautechsystems/nautilus_trader?color=blue)
[![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?logo=discord&logoColor=white)](https://discord.gg/NautilusTrader)

[NautilusTrader](http://nautilustrader.io) adapter for the [Kraken](https://www.kraken.com/) exchange.

The `nautilus-kraken` crate provides client bindings (HTTP & WebSocket), data models,
and helper utilities that wrap the official **Kraken API v2**.

The official Kraken API reference can be found at <https://docs.kraken.com/api/>.

## Platform

[NautilusTrader](http://nautilustrader.io) is an open-source, high-performance, production-grade
algorithmic trading platform, providing quantitative traders with the ability to backtest
portfolios of automated trading strategies on historical data with an event-driven engine,
and also deploy those same strategies live, with no code changes.

NautilusTrader's design, architecture, and implementation philosophy prioritizes software correctness and safety at the
highest level, with the aim of supporting mission-critical, trading system backtesting and live deployment workloads.

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
|----------------|-----------------------|------------------------------|
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

## Examples

See the `bin/` directory for example usage:

```bash
cargo run --bin kraken-http-spot-raw
cargo run --bin kraken-http-spot-public
cargo run --bin kraken-ws-spot-data
```

## Feature flags

This crate provides feature flags to control source code inclusion during compilation:

- `python`: Enables Python bindings from [PyO3](https://pyo3.rs).
- `extension-module`: Builds as a Python extension module (used with `python`).

## Documentation

See [the docs](https://docs.rs/nautilus-kraken) for more detailed usage.

## License

The source code for NautilusTrader is available on GitHub under the [GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).
Contributions to the project are welcome and require the completion of a standard [Contributor License Agreement (CLA)](https://github.com/nautechsystems/nautilus_trader/blob/develop/CLA.md).

---

NautilusTrader is developed and maintained by Nautech Systems, a technology
company specializing in the development of high-performance trading systems.
For more information, visit <https://nautilustrader.io>.

<img src="https://github.com/nautechsystems/nautilus_trader/raw/develop/assets/nautilus-logo-white.png" alt="logo" width="400" height="auto"/>

© 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
