# nautilus-trader

[![crates.io version](https://img.shields.io/crates/v/nautilus-trader.svg)](https://crates.io/crates/nautilus-trader)
[![Documentation](https://docs.rs/nautilus-trader/badge.svg)](https://docs.rs/nautilus-trader)

Container crate for [NautilusTrader](https://nautilustrader.io).

This crate re-exports the core, model, and common component crates as a small
stable entry point. Use the individual `nautilus-*` crates for adapter,
backtest, live, and other crate-specific APIs.

The first re-exported modules are:

- `common`: Common machinery from `nautilus-common`.
- `core`: Core primitives, identifiers, time, and precision support from `nautilus-core`.
- `model`: Trading domain model and data types from `nautilus-model`.

Use the other component crates that match your use case:

- `nautilus-data`: Data engine and market data processing.
- `nautilus-backtest`: Backtesting machinery.
- `nautilus-live`: Live trading machinery.
- `nautilus-trading`: Strategy and actor APIs.
- `nautilus-execution`: Execution engine and order management.
- `nautilus-portfolio`: Portfolio accounting.
- `nautilus-risk`: Risk engine.

Venue adapters publish as separate crates.

## NautilusTrader

[NautilusTrader](https://nautilustrader.io) is an open-source, production-grade,
Rust-native engine for multi-asset, multi-venue trading systems.

The system spans research, deterministic simulation, and live execution within a
single event-driven architecture, providing research-to-live semantic parity.

## Feature flags

This crate has no feature flags.

## Documentation

See [the NautilusTrader documentation](https://nautilustrader.io/docs) and the
component crate docs on [docs.rs](https://docs.rs/releases/search?query=nautilus).

## License

The source code for NautilusTrader is available on GitHub under the
[GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).

Use of this software is subject to the
[Disclaimer](https://nautilustrader.io/legal/disclaimer/).
