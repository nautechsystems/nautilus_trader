# nautilus-betfair

[![build](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml/badge.svg?branch=master)](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml)
[![Documentation](https://img.shields.io/docsrs/nautilus-betfair)](https://docs.rs/nautilus-betfair/latest/nautilus-betfair/)
[![crates.io version](https://img.shields.io/crates/v/nautilus-betfair.svg)](https://crates.io/crates/nautilus-betfair)
![license](https://img.shields.io/github/license/nautechsystems/nautilus_trader?color=blue)
[![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?logo=discord&logoColor=white)](https://discord.gg/NautilusTrader)

[NautilusTrader](https://nautilustrader.io) adapter for the [Betfair](https://www.betfair.com/) betting exchange.

The `nautilus-betfair` crate provides data and execution clients, streaming
and REST API models, and full NautilusTrader integration for the
[Betfair](https://www.betfair.com/) betting exchange.

The official API reference can be found at <https://docs.developer.betfair.com/>.

## NautilusTrader

[NautilusTrader](https://nautilustrader.io) is an open-source, production-grade, Rust-native
engine for multi-asset, multi-venue trading systems.

The system spans research, deterministic simulation, and live execution within a single
event-driven architecture, providing research-to-live semantic parity.

## Feature flags

This crate provides feature flags to control source code inclusion during compilation:

- `high-precision`: Enables [128-bit value types](https://nautilustrader.io/docs/nightly/getting_started/installation#precision-mode) from `nautilus-model`.

## Documentation

See [the docs](https://docs.rs/nautilus-betfair) for more detailed usage.

## License

The source code for NautilusTrader is available on GitHub under the [GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).

---

NautilusTrader™ is developed and maintained by Nautech Systems, a technology
company specializing in the development of high-performance trading systems.
For more information, visit <https://nautilustrader.io>.

Use of this software is subject to the [Disclaimer](https://nautilustrader.io/legal/disclaimer/).

<img src="https://github.com/nautechsystems/nautilus_trader/raw/develop/assets/nautilus-logo-white.png" alt="logo" width="300" height="auto"/>

© 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
