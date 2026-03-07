# nautilus-polymarket

[![build](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml/badge.svg?branch=master)](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml)
[![Documentation](https://img.shields.io/docsrs/nautilus-polymarket)](https://docs.rs/nautilus-polymarket/latest/nautilus-polymarket/)
[![crates.io version](https://img.shields.io/crates/v/nautilus-polymarket.svg)](https://crates.io/crates/nautilus-polymarket)
![license](https://img.shields.io/github/license/nautechsystems/nautilus_trader?color=blue)
[![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?logo=discord&logoColor=white)](https://discord.gg/NautilusTrader)

[NautilusTrader](https://nautilustrader.io) adapter for the [Polymarket](https://polymarket.com) prediction market.

The `nautilus-polymarket` crate provides client implementations (HTTP & WebSocket), data
models and parsing for the **Polymarket CLOB API** for trading binary option contracts.

## Platform

[NautilusTrader](https://nautilustrader.io) is an open-source, high-performance, production-grade
algorithmic trading platform, providing quantitative traders with the ability to backtest
portfolios of automated trading strategies on historical data with an event-driven engine,
and also deploy those same strategies live, with no code changes.

NautilusTrader's design, architecture, and implementation philosophy prioritizes software correctness and safety at the
highest level, with the aim of supporting mission-critical, trading system backtesting and live deployment workloads.

## Feature flags

This crate provides feature flags to control source code inclusion during compilation:

- `python`: Enables Python bindings from [PyO3](https://pyo3.rs).
- `extension-module`: Builds as a Python extension module.

[High-precision mode](https://nautilustrader.io/docs/nightly/getting_started/installation#precision-mode) (128-bit value types) is enabled by default.

## API endpoints

The adapter communicates with three Polymarket API surfaces:

| API            | Base URL                                        | Auth                   | Purpose                                     |
|----------------|-------------------------------------------------|------------------------|---------------------------------------------|
| CLOB REST      | `https://clob.polymarket.com`                   | L2 HMAC                | Orders, trades, balances.                   |
| CLOB WebSocket | `wss://ws-subscriptions-clob.polymarket.com/ws` | L2 HMAC (user channel) | Streaming orderbook, trades, order updates. |
| Gamma (Data)   | `https://data-api.polymarket.com`               | None                   | Market discovery, positions.                |

## Authentication

Polymarket uses two-tier authentication:

- **L1 (EIP-712)**: Wallet-level signing for API credential creation and order signing
  via the CTF Exchange contract. Uses `alloy` signer crates.
- **L2 (HMAC-SHA256)**: API key + secret + passphrase for authenticated REST and
  WebSocket requests. Signatures expire after 30 seconds.

## Documentation

See [the docs](https://docs.rs/nautilus-polymarket) for more detailed usage.

## License

The source code for NautilusTrader is available on GitHub under the [GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).

---

NautilusTrader™ is developed and maintained by Nautech Systems, a technology
company specializing in the development of high-performance trading systems.
For more information, visit <https://nautilustrader.io>.

Use of this software is subject to the [Disclaimer](https://nautilustrader.io/legal/disclaimer/).

<img src="https://github.com/nautechsystems/nautilus_trader/raw/develop/assets/nautilus-logo-white.png" alt="logo" width="300" height="auto"/>

© 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
