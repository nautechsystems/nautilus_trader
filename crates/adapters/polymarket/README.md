# nautilus-polymarket

[![build](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml/badge.svg?branch=master)](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml)
[![Documentation](https://img.shields.io/docsrs/nautilus-polymarket)](https://docs.rs/nautilus-polymarket/latest/nautilus_polymarket/)
[![crates.io version](https://img.shields.io/crates/v/nautilus-polymarket.svg)](https://crates.io/crates/nautilus-polymarket)
![license](https://img.shields.io/github/license/nautechsystems/nautilus_trader?color=blue)
[![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?logo=discord&logoColor=white)](https://discord.gg/NautilusTrader)

[NautilusTrader](https://nautilustrader.io) adapter for the [Polymarket](https://polymarket.com) prediction market.

The `nautilus-polymarket` crate provides market data and order execution for binary outcome contracts
through the Polymarket CLOB APIs. It includes market discovery, authenticated HTTP and WebSocket
clients, order signing, and execution reconciliation, plus Deposit Wallet position operations
through the Relayer API.

The official Polymarket API reference can be found at <https://docs.polymarket.com/>.

## NautilusTrader

[NautilusTrader](https://nautilustrader.io) is an open-source, production-grade, Rust-native trading engine
with a deterministic, event-driven architecture for multi-asset, multi-venue systems.

The system spans research, simulation, and live execution, providing research-to-live semantic parity.

## Configuration

See the [integration guide](../../../docs/integrations/polymarket.md) for client settings,
credentials, and connection setup.

### Authentication

The CLOB uses two-tier authentication:

- **L1 (EIP-712)**: Wallet-level signing for API credential creation and order signing
  via the CTF Exchange contract. Uses `alloy` signer crates.
- **L2 (HMAC-SHA256)**: API key + secret + passphrase for authenticated REST and
  WebSocket requests. Signatures expire after 30 seconds.

Deposit Wallet position operations use a separate Relayer API key, sent in the `RELAYER_API_KEY`
header with its signer address in `RELAYER_API_KEY_ADDRESS`.

Session authorization and revocation use owner signatures and Builder HMAC credentials.
`PolymarketSessionKeyClient` manages access outside the trading runtime; session trading uses
explicit session CLOB credentials and `PolymarketSignerType::Session`.

### API endpoints

The adapter communicates with these Polymarket API surfaces:

| API            | Base URL                                        | Auth                            | Purpose                                     |
| -------------- | ----------------------------------------------- | ------------------------------- | ------------------------------------------- |
| CLOB REST      | `https://clob.polymarket.com`                   | L2 HMAC                         | Orders, trades, balances.                   |
| CLOB WebSocket | `wss://ws-subscriptions-clob.polymarket.com/ws` | L2 HMAC (user channel)          | Streaming orderbook, trades, order updates. |
| PolyBolt       | `wss://ws-live-v2.polymarket.com/ws`            | L2 credentials (auth frame)     | Crypto, equity, and TWAP reference prices.  |
| Gamma          | `https://gamma-api.polymarket.com`              | None                            | Market and event discovery, tags, search.   |
| Data           | `https://data-api.polymarket.com`               | None                            | Trade history and user positions.           |
| Relayer        | `https://relayer-v2.polymarket.com`             | Relayer API key or Builder HMAC | Deposit Wallet positions and session keys.  |

## Feature flags

Feature flags control code inclusion for Rust builds and Python bindings:

- `allowances`: Enables the `polymarket-set-allowances` binary.
- `examples`: Enables the crate's example binaries.
- `extension-module`: Builds as a Python extension module.
- `high-precision` (default): Enables
  [high-precision mode](https://nautilustrader.io/docs/nightly/getting_started/installation/#precision-mode)
  to use 128-bit value types.
- `python`: Enables Python bindings from [PyO3](https://pyo3.rs).

## Integrator attribution

Orders signed by the adapter carry the fixed NautilusTrader builder code with zero builder fees.

See [attribution details](../../../docs/integrations/polymarket.md#builder-code-attribution).

## Documentation

- [Rust API reference](https://docs.rs/nautilus-polymarket)
- [Integration guide](../../../docs/integrations/polymarket.md)
- [Rust examples](examples/)
- [Python examples](../../../examples/live/polymarket/)

## License

The source code for NautilusTrader is available on GitHub under the [GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).

---

NautilusTrader™ is developed and maintained by Nautech Systems, a technology
company specializing in the development of high-performance trading systems.
For more information, visit <https://nautilustrader.io>.

Use of this software is subject to the [Disclaimer](https://nautilustrader.io/legal/disclaimer/).

<img src="https://github.com/nautechsystems/nautilus_trader/raw/develop/assets/nautilus-logo-white.png" alt="logo" width="300" height="auto"/>

© 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
