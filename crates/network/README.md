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

- `python`: Exposes the `TransportBackend` enum through [PyO3](https://pyo3.rs).
- `extension-module`: Builds as a Python extension module.
- `turmoil`: Enables deterministic network simulation testing with [turmoil](https://github.com/tokio-rs/turmoil).
- `transport-sockudo`: Adds the [sockudo-ws](https://crates.io/crates/sockudo-ws) WebSocket backend, selectable via `WebSocketConfig.backend`.

## WebSocket performance

The 512 B text round-trip benchmark measures 50,000 messages after 1,000 warmup messages. Values
are the median of three `bench-lto` runs on an AMD Ryzen Threadripper 9980X with the CPU governor
set to `performance` and ASLR disabled. Lower latency is better.

| Library                    | p50 (µs) | p95 (µs) | p99 (µs) | p99.9 (µs) |
| -------------------------- | -------: | -------: | -------: | ---------: |
| `tokio-tungstenite 0.30.0` |    2.033 |    2.985 |    3.305 |      6.149 |
| `sockudo-ws 2.0.1`         |    0.601 |    0.631 |    0.651 |      0.721 |

On this workload, `sockudo-ws 2.0.1` has 80% lower p99 latency than
`tokio-tungstenite 0.30.0`. See the [full WebSocket benchmark report](benches/BENCHMARKS.md)
for all payloads, burst latency, throughput, methodology, and limitations.

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

Some real localhost socket and WebSocket unit tests are Linux-only for CI stability. On macOS,
use the Turmoil tests and soak for deterministic reconnect/path-search coverage, and rely on a
Linux run for host TCP unit coverage.

To sweep Turmoil reconnect seeds continuously:

```bash
scripts/soak-network-turmoil.sh
```

Set `NAUTILUS_TURMOIL_SOAK_COUNT` for a bounded run. The soak alternates the
Tungstenite and Sockudo WebSocket backends on the same seed when
`transport-sockudo` is enabled.

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
