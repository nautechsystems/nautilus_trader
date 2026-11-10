# nautilus-finfeed

[![build](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml/badge.svg?branch=master)](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml)
[![Documentation](https://img.shields.io/docsrs/nautilus-finfeed)](https://docs.rs/nautilus-finfeed/latest/nautilus_finfeed/)
[![crates.io version](https://img.shields.io/crates/v/nautilus-finfeed.svg)](https://crates.io/crates/nautilus-finfeed)
![license](https://img.shields.io/github/license/nautechsystems/nautilus_trader?color=blue)
[![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?logo=discord&logoColor=white)](https://discord.gg/NautilusTrader)

[NautilusTrader](https://nautilustrader.io) adapter for the
[FinFeedAPI Prediction Markets API](https://www.finfeedapi.com/products/prediction-markets-api?ref=nautilus).
API Bricks provides FinFeedAPI and CoinAPI.

The adapter provides authenticated REST access, checked Polymarket binary Yes/No instrument definitions,
historical data requests, and book, quote, trade, and closed-bar polling. It is implemented in Rust with
optional Python bindings. Order execution is unsupported.
Importing the Python package neither reads credentials nor connects.

The official FinFeedAPI API reference can be found at
<https://www.finfeedapi.com/products/prediction-markets-api/docs?ref=nautilus>.

## NautilusTrader

[NautilusTrader](https://nautilustrader.io) is an open-source, production-grade, Rust-native trading engine
with a deterministic, event-driven architecture for multi-asset, multi-venue systems.

The system spans research, simulation, and live execution, providing research-to-live semantic parity.

## Capabilities and limits

| Capability                                                                         | Support                                                                  |
| ---------------------------------------------------------------------------------- | ------------------------------------------------------------------------ |
| [Instruments](../../../docs/integrations/finfeed.md#supported-venues-and-outcomes) | Selected Polymarket Yes/No outcomes; independent FinFeed IDs             |
| [REST access](../../../docs/integrations/finfeed.md#python-rest-access)            | Discovery, metadata, activity, books, and historical data                |
| [Polling](../../../docs/integrations/finfeed.md#polling-subscriptions)             | Quotes, trades, replacement L2 books, and completed external `LAST` bars |
| [History](../../../docs/integrations/finfeed.md#historical-data)                   | Trades, quotes, L2 books, and bars; book replay requires a snapshot      |

Provider event timestamps are preserved rather than replaced with local arrival times. Historical
book replay derives a monotone group ordering clock for `ts_init` and request bounds, using the
cumulative maximum of processing timestamps when present and exchange timestamps otherwise.
This clock preserves source order but does not measure receipt time. Trades use
`NO_AGGRESSOR`. Book snapshots do not establish incremental continuity or full venue depth.
OHLCV `LAST` does not establish trade-only prices or final revisions. Prices and raw sizes remain exact;
OHLCV volume permits only [bounded normalization](../../../docs/integrations/finfeed.md#prices-and-finality).

Fresh `Closed` or `Resolved` metadata ends polling and rejects new subscriptions for that outcome.
Metadata has no periodic refresh, and no authoritative settlement or close event is inferred.
The provider end date sets the simulated trading cutoff; it does not establish actual venue closure or settlement.

Historical books, quotes, trades, and bars have bounded Rust and Python checks and live Polymarket
[samples](tests/README.md#observed-historical-samples). Positive bar delivery after reconnect remains unverified. See the
[capability matrix](../../../docs/integrations/finfeed.md#supported-capabilities) and
[evidence and limits](../../../docs/integrations/finfeed.md#limitations) before relying on a data surface.

## Configuration

See the [configuration guide](../../../docs/integrations/finfeed.md#configuration).

## Feature flags

Feature flags control code inclusion for Rust builds and Python bindings:

- `extension-module`: Builds as a Python extension module.
- `fuzz`: Enables the REST decoder and domain conversion libFuzzer target.
- `high-precision` (default): Enables
  [high-precision mode](https://nautilustrader.io/docs/nightly/getting_started/installation/#precision-mode)
  to use 128-bit value types.
- `python`: Enables Python bindings for identity, configuration, factories, and read-only REST access
  with typed queries and results through [PyO3](https://pyo3.rs).
- `simulation`: Routes time and HTTP transport through deterministic simulation with `cfg(madsim)`.
  Simulated transport supports plaintext local peers; HTTPS and explicit proxies require native tests.

## Integrator attribution

REST requests send `X-Referrer: nautilus` for API Bricks usage attribution.
The fixed header contains no credential or account data.

See [attribution details](../../../docs/integrations/finfeed.md#integrator-attribution).

## Fuzzing

Coverage-guided fuzz target `fuzz-rest` covers REST decoding and domain conversion. It requires the
workspace-pinned `cargo-fuzz` binary and a Rust nightly toolchain. See the
[fuzzing instructions](tests/README.md#robustness-and-polling-workloads) for a bounded run with fixture seeds.

## Documentation

- [Rust API reference](https://docs.rs/nautilus-finfeed)
- [Integration guide](../../../docs/integrations/finfeed.md)
- [Rust examples](examples/)
- [Python examples](../../../examples/live/finfeed/)

## License

Licensed under the [GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).
