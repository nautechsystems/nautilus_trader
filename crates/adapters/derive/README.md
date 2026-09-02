# nautilus-derive

[![build](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml/badge.svg?branch=master)](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml)
[![Documentation](https://img.shields.io/docsrs/nautilus-derive)](https://docs.rs/nautilus-derive/latest/nautilus-derive/)
[![crates.io version](https://img.shields.io/crates/v/nautilus-derive.svg)](https://crates.io/crates/nautilus-derive)
![license](https://img.shields.io/github/license/nautechsystems/nautilus_trader?color=blue)
[![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?logo=discord&logoColor=white)](https://discord.gg/NautilusTrader)

[NautilusTrader](https://nautilustrader.io) adapter for the
[Derive](https://www.derive.xyz) decentralized derivatives exchange.

The `nautilus-derive` crate implements the Derive adapter for NautilusTrader, including typed HTTP
and WebSocket clients, REST and stream models, venue parsing, data and execution client wiring, and
EIP-712 signing for the official **Derive API**.

Derive offers European-style options, perpetual swaps, and spot markets on the Derive Chain, an
optimistic rollup that settles to Ethereum. Orders match off-chain and settle on-chain while users
retain custody through per-user smart-contract wallets.

## NautilusTrader

[NautilusTrader](https://nautilustrader.io) is an open-source, production-grade, Rust-native
engine for multi-asset, multi-venue trading systems.

The system spans research, deterministic simulation, and live execution within a single
event-driven architecture, providing research-to-live semantic parity.

## Feature flags

This crate provides feature flags to control source code inclusion during compilation:

- `examples`: Enables the crate's example binaries.
- `extension-module`: Builds as a Python extension module.
- `fuzz`: Enables libFuzzer integration for fuzz targets.
- `high-precision` (default): Enables
  [high-precision mode](https://nautilustrader.io/docs/nightly/getting_started/installation#precision-mode)
  to use 128-bit value types.
- `python`: Enables Python bindings from [PyO3](https://pyo3.rs).

## Fuzzing

Coverage-guided fuzz targets for Derive wire models, parsers, signing payloads, and nonce sequencing
live in [`fuzz/`](fuzz/README.md). They require the workspace-pinned `cargo-fuzz` binary and a Rust
nightly toolchain.

## Documentation

See the [Derive integration guide](https://nautilustrader.io/docs/nightly/integrations/derive)
and [crate docs](https://docs.rs/nautilus-derive) for more detailed usage.

## License

The source code for NautilusTrader is available on GitHub under the
[GNU Lesser General Public License v3.0](https://github.com/nautechsystems/nautilus_trader/blob/develop/LICENSE).

---

NautilusTrader™ is developed and maintained by Nautech Systems, a technology
company specializing in the development of high-performance trading systems.
For more information, visit <https://nautilustrader.io>.

Use of this software is subject to the [Disclaimer](https://nautilustrader.io/legal/disclaimer/).

<img src="https://github.com/nautechsystems/nautilus_trader/raw/develop/assets/nautilus-logo-white.png" alt="logo" width="300" height="auto"/>

© 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
