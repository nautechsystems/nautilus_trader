# nautilus-binance

[![build](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml/badge.svg?branch=master)](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml)
[![Documentation](https://img.shields.io/docsrs/nautilus-binance)](https://docs.rs/nautilus-binance/latest/nautilus-binance/)
[![crates.io version](https://img.shields.io/crates/v/nautilus-binance.svg)](https://crates.io/crates/nautilus-binance)
![license](https://img.shields.io/github/license/nautechsystems/nautilus_trader?color=blue)
[![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?logo=discord&logoColor=white)](https://discord.gg/NautilusTrader)

[NautilusTrader](https://nautilustrader.io) adapter for the
[Binance](https://www.binance.com/) cryptocurrency exchange.

The `nautilus-binance` crate provides client bindings (HTTP & WebSocket), data models,
and helper utilities that wrap the official **Binance API** across:

- Spot trading (api.binance.com)
- Spot margin trading
- USD-M Futures (fapi.binance.com)
- COIN-M Futures (dapi.binance.com)
- European Options (eapi.binance.com)

## NautilusTrader

[NautilusTrader](https://nautilustrader.io) is an open-source, production-grade, Rust-native
engine for multi-asset, multi-venue trading systems.

The system spans research, deterministic simulation, and live execution within a single
event-driven architecture, providing research-to-live semantic parity.

## Authentication

This crate requires **Ed25519 API keys** for all authenticated endpoints (REST and WebSocket API).
Ed25519 is recommended by Binance for its superior performance and security. HMAC and RSA keys
are not supported.

Generate an Ed25519 keypair and register it with Binance:

```bash
# Generate private key (PKCS#8 PEM format)
openssl genpkey -algorithm ed25519 -out binance_ed25519_private.pem

# Extract public key for Binance registration
openssl pkey -in binance_ed25519_private.pem -pubout -out binance_ed25519_public.pem
```

Set credentials via environment variables:

```bash
export BINANCE_API_KEY="your-api-key-from-binance"
export BINANCE_API_SECRET="$(cat binance_ed25519_private.pem)"
```

## Feature flags

This crate provides feature flags to control source code inclusion during compilation:

- `python`: Enables Python bindings from [PyO3](https://pyo3.rs).
- `extension-module`: Builds as a Python extension module.

[High-precision mode](https://nautilustrader.io/docs/nightly/getting_started/installation#precision-mode) (128-bit value types) is enabled by default.

## Documentation

See [the docs](https://docs.rs/nautilus-binance) for more detailed usage.

## License

The source code for NautilusTrader is available on GitHub under the [GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).

---

NautilusTrader™ is developed and maintained by Nautech Systems, a technology
company specializing in the development of high-performance trading systems.
For more information, visit <https://nautilustrader.io>.

Use of this software is subject to the [Disclaimer](https://nautilustrader.io/legal/disclaimer/).

<img src="https://github.com/nautechsystems/nautilus_trader/raw/develop/assets/nautilus-logo-white.png" alt="logo" width="300" height="auto"/>

© 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
