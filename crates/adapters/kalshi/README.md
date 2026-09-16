# nautilus-kalshi

[![build](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml/badge.svg?branch=master)](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml)
[![Documentation](https://img.shields.io/docsrs/nautilus-kalshi)](https://docs.rs/nautilus-kalshi/latest/nautilus_kalshi/)
[![crates.io version](https://img.shields.io/crates/v/nautilus-kalshi.svg)](https://crates.io/crates/nautilus-kalshi)
![license](https://img.shields.io/github/license/nautechsystems/nautilus_trader?color=blue)
[![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?logo=discord&logoColor=white)](https://discord.gg/NautilusTrader)

[NautilusTrader](https://nautilustrader.io) adapter for the [Kalshi](https://kalshi.com) prediction market.

The `nautilus-kalshi` crate provides clients, data models and parsing for the **Kalshi Trade API**
for trading binary option contracts.

Kalshi publishes market data and order state over REST, so both clients poll the venue and the
adapter carries no WebSocket client. See the [Kalshi integration guide](https://nautilustrader.io/docs/nightly/integrations/kalshi/) for what that means in practice.

## API endpoints

The adapter communicates with the Kalshi Trade API over REST:

| API            | Base URL                                           | Auth                   | Purpose                                |
| -------------- | -------------------------------------------------- | ---------------------- | -------------------------------------- |
| Trade API      | `https://external-api.kalshi.com/trade-api/v2`     | RSA-PSS signed headers | Markets, trades, books, and portfolio. |
| Demo Trade API | `https://external-api.demo.kalshi.co/trade-api/v2` | RSA-PSS signed headers | The same surface on the demo exchange. |

The exchange also serves the same API on its shared hostnames:
`https://api.elections.kalshi.com/trade-api/v2` for production, and
`https://demo-api.kalshi.co/trade-api/v2` for the demo exchange.

## Authentication

Authenticated requests carry three headers: the API key ID, a millisecond timestamp, and a
base64-encoded RSA-PSS SHA-256 signature. The signature covers `timestamp + method + path`, where
the path is the route from the API root including the `/trade-api/v2` prefix and excluding any
query parameters. The headers are `KALSHI-ACCESS-KEY`, `KALSHI-ACCESS-TIMESTAMP`, and
`KALSHI-ACCESS-SIGNATURE`.

Both clients resolve credentials from the configuration, falling back to the `KALSHI_API_KEY_ID`
and `KALSHI_API_KEY_PEM` environment variables.

## NautilusTrader

[NautilusTrader](https://nautilustrader.io) is an open-source, production-grade, Rust-native
engine for multi-asset, multi-venue trading systems.

The system spans research, deterministic simulation, and live execution within a single
event-driven architecture, providing research-to-live semantic parity.

## Feature flags

This crate provides feature flags to control source code inclusion during compilation:

- `extension-module`: Builds as a Python extension module.
- `high-precision` (default): Enables
  [high-precision mode](https://nautilustrader.io/docs/nightly/getting_started/installation/#precision-mode)
  to use 128-bit value types.
- `python`: Enables Python bindings from [PyO3](https://pyo3.rs).

## Documentation

See the [Kalshi integration guide](https://nautilustrader.io/docs/nightly/integrations/kalshi/)
and [crate docs](https://docs.rs/nautilus-kalshi) for more detailed usage.

## License

The source code for NautilusTrader is available on GitHub under the [GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).

---

NautilusTrader™ is developed and maintained by Nautech Systems, a technology
company specializing in the development of high-performance trading systems.
For more information, visit <https://nautilustrader.io>.

Use of this software is subject to the [Disclaimer](https://nautilustrader.io/legal/disclaimer/).

<img src="https://github.com/nautechsystems/nautilus_trader/raw/develop/assets/nautilus-logo-white.png" alt="logo" width="300" height="auto"/>

© 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
