# nautilus-kalshi

[![build](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml/badge.svg?branch=master)](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml)
[![Documentation](https://img.shields.io/docsrs/nautilus-kalshi)](https://docs.rs/nautilus-kalshi/latest/nautilus_kalshi/)
[![crates.io version](https://img.shields.io/crates/v/nautilus-kalshi.svg)](https://crates.io/crates/nautilus-kalshi)
![license](https://img.shields.io/github/license/nautechsystems/nautilus_trader?color=blue)
[![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?logo=discord&logoColor=white)](https://discord.gg/NautilusTrader)

[NautilusTrader](https://nautilustrader.io) adapter for [Kalshi](https://kalshi.com), a regulated event contracts exchange.

The `nautilus-kalshi` crate provides instrument loading, live and historical market data, and order execution
for binary event contracts through the **Kalshi API v2**. It includes typed HTTP and WebSocket clients,
account and execution reconciliation, and live-node factories.

Execution recovery uses authoritative venue reports. Uncertain submissions retain conservative cash holds
and pause writes until fresh account recovery completes. Unresolved amendments and cancellations keep
writes blocked until order evidence resolves them. Direct client reconnects continue report collection
while writes remain blocked; node startup waits for write readiness.

The official Kalshi API reference can be found at <https://docs.kalshi.com/>.

## NautilusTrader

[NautilusTrader](https://nautilustrader.io) is an open-source, production-grade, Rust-native trading engine
with a deterministic, event-driven architecture for multi-asset, multi-venue systems.

The system spans research, simulation, and live execution, providing research-to-live semantic parity.

## Configuration

See the [integration guide](../../../docs/integrations/kalshi.md) for client settings,
credentials, and connection setup.

## Feature flags

Feature flags control code inclusion for Rust builds and Python bindings:

- `examples`: Enables book stress scenarios.
- `extension-module`: Builds as a Python extension module.
- `fuzz`: Enables libFuzzer integration for fuzz targets.
- `high-precision` (default): Enables
  [high-precision mode](https://nautilustrader.io/docs/nightly/getting_started/installation/#precision-mode)
  to use 128-bit value types.
- `python`: Enables Python bindings from [PyO3](https://pyo3.rs).
- `simulation`: Routes HTTP through deterministic network seams with `cfg(madsim)`.
  Simulation supports plaintext local HTTP peers; TLS and proxies remain native-only.

## Fuzzing

Coverage-guided fuzz targets for untrusted frames, fixed-point values, books, order construction,
execution reports, and subscription sequences live in [`fuzz/`](fuzz/README.md). They require the
workspace-pinned `cargo-fuzz` binary and a Rust nightly toolchain.

## Documentation

- [Integration guide](../../../docs/integrations/kalshi.md)
- [Rust examples](examples/)
- [Python examples](../../../examples/live/kalshi/)
- [Data and execution benchmarks](benches/BENCHMARKS.md)

## License

The source code for NautilusTrader is available on GitHub under the [GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).

---

NautilusTrader™ is developed and maintained by Nautech Systems, a technology
company specializing in the development of high-performance trading systems.
For more information, visit <https://nautilustrader.io>.

Use of this software is subject to the [Disclaimer](https://nautilustrader.io/legal/disclaimer/).

<img src="https://github.com/nautechsystems/nautilus_trader/raw/develop/assets/nautilus-logo-white.png" alt="logo" width="300" height="auto"/>

© 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
