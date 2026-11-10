# nautilus-architect-ax

[![build](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml/badge.svg?branch=master)](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml)
[![Documentation](https://img.shields.io/docsrs/nautilus-architect-ax)](https://docs.rs/nautilus-architect-ax/latest/nautilus_architect_ax/)
[![crates.io version](https://img.shields.io/crates/v/nautilus-architect-ax.svg)](https://crates.io/crates/nautilus-architect-ax)
![license](https://img.shields.io/github/license/nautechsystems/nautilus_trader?color=blue)
[![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?logo=discord&logoColor=white)](https://discord.gg/NautilusTrader)

[NautilusTrader](https://nautilustrader.io) adapter for [AX Exchange](https://architect.exchange).

The `nautilus-architect-ax` crate connects AX Exchange market data and order execution to NautilusTrader.
It provides typed HTTP and WebSocket clients, instrument definitions, and live-node factories for
production and sandbox environments.

The official AX Exchange API reference can be found at <https://docs.architect.exchange/api-reference>.

## NautilusTrader

[NautilusTrader](https://nautilustrader.io) is an open-source, production-grade, Rust-native trading engine
with a deterministic, event-driven architecture for multi-asset, multi-venue systems.

The system spans research, simulation, and live execution, providing research-to-live semantic parity.

## Overview

[AX Exchange](https://architect.exchange) is a centralized and regulated derivatives exchange for
traditional underlying asset classes. Its production catalog lists perpetual contracts across FX,
equities, energy ETFs, metals, energy, treasuries, and compute. Its sandbox also exposes dated
futures. AX is licensed by the
[Bermuda Monetary Authority (BMA)](https://www.bma.bm).

## Configuration

See the [integration guide](../../../docs/integrations/architect_ax.md) for client settings,
credentials, and connection setup.

### Authentication

AX Exchange uses bearer token authentication via HTTP headers:

1. API key and secret obtain a session token via `/authenticate`.
2. The session token is used as a bearer token for subsequent REST and WebSocket requests.

### API endpoints

| Environment | HTTP API (market data)                           | HTTP API (orders)                                   | Market Data WS                                   | Orders WS                                            |
| ----------- | ------------------------------------------------ | --------------------------------------------------- | ------------------------------------------------ | ---------------------------------------------------- |
| Sandbox     | `https://gateway.sandbox.architect.exchange/api` | `https://gateway.sandbox.architect.exchange/orders` | `wss://gateway.sandbox.architect.exchange/md/ws` | `wss://gateway.sandbox.architect.exchange/orders/ws` |
| Production  | `https://gateway.architect.exchange/api`         | `https://gateway.architect.exchange/orders`         | `wss://gateway.architect.exchange/md/ws`         | `wss://gateway.architect.exchange/orders/ws`         |

## Feature flags

Feature flags control code inclusion for Rust builds and Python bindings:

- `examples`: Enables the crate's example binaries.
- `extension-module`: Builds as a Python extension module.
- `python`: Enables Python bindings from [PyO3](https://pyo3.rs).

## Integrator attribution

WebSocket order submissions set `tag` to `Nautilus` for attribution.
HTTP order submissions omit `tag` unless the caller supplies it.

## Examples

Run example binaries to test the adapter. `AX_IS_SANDBOX` defaults to `true`.
The WebSocket examples also require API credentials:

```bash
export AX_IS_SANDBOX=true
export AX_API_KEY="your-sandbox-api-key"
export AX_API_SECRET="your-sandbox-api-secret"

# HTTP client example
cargo run -p nautilus-architect-ax --bin ax-http-public

# WebSocket data client example
cargo run -p nautilus-architect-ax --bin ax-ws-data

# WebSocket orders client example
cargo run -p nautilus-architect-ax --bin ax-ws-orders
```

## Documentation

- [Rust API reference](https://docs.rs/nautilus-architect-ax)
- [Integration guide](../../../docs/integrations/architect_ax.md)
- [Rust examples](examples/)
- [Python examples](../../../examples/live/architect_ax/)

## License

The source code for NautilusTrader is available on GitHub under the [GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).

---

NautilusTrader™ is developed and maintained by Nautech Systems, a technology
company specializing in the development of high-performance trading systems.
For more information, visit <https://nautilustrader.io>.

Use of this software is subject to the [Disclaimer](https://nautilustrader.io/legal/disclaimer/).

<img src="https://github.com/nautechsystems/nautilus_trader/raw/develop/assets/nautilus-logo-white.png" alt="logo" width="300" height="auto"/>

© 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
