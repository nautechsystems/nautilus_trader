# nautilus-coinapi

[![build](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml/badge.svg?branch=master)](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml)
[![Documentation](https://img.shields.io/docsrs/nautilus-coinapi)](https://docs.rs/nautilus-coinapi/latest/nautilus_coinapi/)
[![crates.io version](https://img.shields.io/crates/v/nautilus-coinapi.svg)](https://crates.io/crates/nautilus-coinapi)
![license](https://img.shields.io/github/license/nautechsystems/nautilus_trader?color=blue)
[![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?logo=discord&logoColor=white)](https://discord.gg/NautilusTrader)

[NautilusTrader](https://nautilustrader.io) adapter for the
[CoinAPI Market Data API](https://www.coinapi.io/products/market-data-api?ref=nautilus).
API Bricks provides CoinAPI and FinFeedAPI.

The adapter provides authenticated REST and WebSocket access to cryptocurrency market data across
multiple exchanges. It supports native venue instrument definitions, live feeds, historical requests,
and loaders for downloaded flat files. It is implemented in Rust with optional Python bindings.
Order execution and account management are unsupported.

The official CoinAPI API reference can be found at <https://www.coinapi.io/products/market-data-api/docs?ref=nautilus>.

## NautilusTrader

[NautilusTrader](https://nautilustrader.io) is an open-source, production-grade, Rust-native trading engine
with a deterministic, event-driven architecture for multi-asset, multi-venue systems.

The system spans research, simulation, and live execution, providing research-to-live semantic parity.

## Capabilities and limits

| Capability                                                                 | Support                                                        |
| -------------------------------------------------------------------------- | -------------------------------------------------------------- |
| [Instruments](../../../docs/integrations/coinapi.md#supported-instruments) | Selected spot, perpetuals, and dated futures; native venue IDs |
| [Order books](../../../docs/integrations/coinapi.md#order-books)           | L2/L3 deltas with gap recovery; L2 snapshots up to 50 levels   |
| [Live feeds](../../../docs/integrations/coinapi.md#data-subscriptions)     | Quotes, trades, and supported external `LAST` bars             |
| [History](../../../docs/integrations/coinapi.md#historical-requests)       | Trades, quotes, and bars; current REST L2 snapshots only       |
| [Flat files](../../../docs/integrations/coinapi.md#flat-files)             | Local CSV loading; chunked quote, trade, and book streaming    |

The data client preserves prices and sizes at instrument precision. It rejects finer values, with a
[bounded exception for OHLCV volume](../../../docs/integrations/coinapi.md#precision).
Price precision can be overridden for selected instruments; size precision has no override.
Only exchange-reported trade sides set the aggressor; CoinAPI estimates map to `NO_AGGRESSOR`.

CoinAPI and native adapters must load disjoint instrument sets within a node. Separate client IDs
do not isolate definitions or feeds for the same instrument, and the adapter does not detect these
conflicts. CoinAPI data resolutions can differ from venue trading increments. See
[native adapter routing](../../../docs/integrations/coinapi.md#using-coinapi-with-native-adapters)
and the [capability matrix](../../../docs/integrations/coinapi.md#supported-capabilities).

## Configuration

See the [configuration guide](../../../docs/integrations/coinapi.md#configuration).

## Feature flags

Feature flags control code inclusion for Rust builds and Python bindings:

- `examples`: Enables the crate's example binaries.
- `extension-module`: Builds as a Python extension module.
- `fuzz`: Enables libFuzzer integration for fuzz targets.
- `high-precision` (default): Enables
  [high-precision mode](https://nautilustrader.io/docs/nightly/getting_started/installation/#precision-mode)
  to use 128-bit value types.
- `python`: Enables Python bindings for identity, configuration, factories, and flat-file loaders
  through [PyO3](https://pyo3.rs).
- `simulation`: Routes HTTP and WebSocket through deterministic transport when built with `cfg(madsim)`.

## Integrator attribution

REST requests and WebSocket handshakes include `X-Referrer: nautilus` for API Bricks usage attribution.
The fixed header contains no credential or account data.

See [attribution details](../../../docs/integrations/coinapi.md#integrator-attribution).

## Fuzzing

Coverage-guided fuzz targets for WebSocket, book, and REST parsing live in [`fuzz/`](fuzz/README.md).
They require the workspace-pinned `cargo-fuzz` binary and a Rust nightly toolchain.

## Documentation

- [Rust API reference](https://docs.rs/nautilus-coinapi)
- [Integration guide](../../../docs/integrations/coinapi.md)
- [Rust examples](examples/)
- [Python examples](../../../examples/live/coinapi/)

## License

Licensed under the [GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).
