# nautilus-network

[![build](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml/badge.svg?branch=master)](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml)
[![Documentation](https://img.shields.io/docsrs/nautilus-network)](https://docs.rs/nautilus-network/latest/nautilus-network/)
[![crates.io version](https://img.shields.io/crates/v/nautilus-network.svg)](https://crates.io/crates/nautilus-network)
![license](https://img.shields.io/github/license/nautechsystems/nautilus_trader?color=blue)
[![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?logo=discord&logoColor=white)](https://discord.gg/NautilusTrader)

Network functionality for [NautilusTrader](https://nautilustrader.io).

The `nautilus-network` crate provides networking components including HTTP, WebSocket, and raw TCP socket
clients, rate limiting, backoff strategies, and socket TLS utilities for connecting to
trading venues and data providers.

## NautilusTrader

[NautilusTrader](https://nautilustrader.io) is an open-source, production-grade, Rust-native
engine for multi-asset, multi-venue trading systems.

The system spans research, deterministic simulation, and live execution within a single
event-driven architecture, providing research-to-live semantic parity.

## Feature flags

This crate provides feature flags to control source code inclusion during compilation:

- `python`: Enables Python bindings from [PyO3](https://pyo3.rs).
- `extension-module`: Builds as a Python extension module.
- `turmoil`: Enables deterministic network simulation testing with [turmoil](https://github.com/tokio-rs/turmoil).
- `transport-sockudo`: Adds the [sockudo-ws](https://crates.io/crates/sockudo-ws) WebSocket backend, selectable via `WebSocketConfig.backend`.

## Testing

The crate includes both standard integration tests and deterministic network simulation tests using turmoil.

To run standard tests:

```bash
cargo nextest run -p nautilus-network
```

To run turmoil network simulation tests:

```bash
cargo nextest run -p nautilus-network --features turmoil
```

The turmoil tests simulate various network conditions (reconnections, partitions, etc.) in a deterministic way,
allowing reliable testing of network failure scenarios without flakiness.

## Documentation

See [the docs](https://docs.rs/nautilus-network) for more detailed usage.

## License

The source code for NautilusTrader is available on GitHub under the [GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).

---

NautilusTrader™ is developed and maintained by Nautech Systems, a technology
company specializing in the development of high-performance trading systems.
For more information, visit <https://nautilustrader.io>.

Use of this software is subject to the [Disclaimer](https://nautilustrader.io/legal/disclaimer/).

<img src="https://github.com/nautechsystems/nautilus_trader/raw/develop/assets/nautilus-logo-white.png" alt="logo" width="300" height="auto"/>

© 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
