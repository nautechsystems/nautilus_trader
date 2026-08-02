# nautilus-interactive-brokers

[![build](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml/badge.svg?branch=master)](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml)
[![Documentation](https://img.shields.io/docsrs/nautilus-interactive-brokers)](https://docs.rs/nautilus-interactive-brokers/latest/nautilus-interactive-brokers/)
[![crates.io version](https://img.shields.io/crates/v/nautilus-interactive-brokers.svg)](https://crates.io/crates/nautilus-interactive-brokers)
![license](https://img.shields.io/github/license/nautechsystems/nautilus_trader?color=blue)
[![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?logo=discord&logoColor=white)](https://discord.gg/NautilusTrader)

[NautilusTrader](https://nautilustrader.io) adapter for
[Interactive Brokers](https://www.interactivebrokers.com).

The `nautilus-interactive-brokers` crate wraps the [`ibapi`](https://crates.io/crates/ibapi)
client and connects it to NautilusTrader's live data, execution, historical data, and instrument
loading infrastructure. Optional PyO3 bindings expose the same implementation through
`nautilus_trader`.

## NautilusTrader

[NautilusTrader](https://nautilustrader.io) is an open‑source, production‑grade, Rust‑native
engine for multi‑asset, multi‑venue trading systems.

The system spans research, deterministic simulation, and live execution within a single
event‑driven architecture, providing research‑to‑live semantic parity.

## What this crate provides

- `data`: `InteractiveBrokersDataClient` for market data subscriptions and live streaming.
- `execution`: `InteractiveBrokersExecutionClient` for order submission, account synchronization,
  and execution updates.
- `historical`: `HistoricalInteractiveBrokersClient` for historical data requests.
- `providers`: `InteractiveBrokersInstrumentProvider` for contract lookup, instrument normalization,
  and symbology conversion.
- `gateway`: `DockerizedIBGateway` for managing a Dockerized IB Gateway when the `gateway` feature
  is enabled.
- `python`: PyO3 bindings exposed as `nautilus_pyo3.interactive_brokers` when the `python` feature
  is enabled.

## Feature flags

This crate provides feature flags to control source code inclusion during compilation:

- `python`: Enables PyO3 bindings for configs, enums, the historical client,
  and the instrument provider.
- `gateway`: Enables Dockerized IB Gateway support via `bollard`, including PyO3 bindings when
  combined with `python`.
- `extension-module`: Builds the crate as a Python extension module. This is
  the feature used by the `nautilus_trader` package and includes `python` and
  `gateway`.

## Default ports

Use `127.0.0.1` unless you are connecting to a remote host.

| Endpoint              | Trading mode | Default port |
| --------------------- | ------------ | -----------: |
| IB Gateway            | Paper        |       `4002` |
| IB Gateway            | Live         |       `4001` |
| TWS                   | Paper        |       `7497` |
| TWS                   | Live         |       `7496` |
| Dockerized IB Gateway | Paper        |       `4002` |
| Dockerized IB Gateway | Live         |       `4001` |

This crate defaults to `4002`, which matches paper‑trading IB Gateway and the
default Dockerized IB Gateway paper setup. If you are connecting to TWS or to a
live Gateway session, set the port explicitly in your config.

## Market data timestamps

Configure TWS or IB Gateway to return market data timestamps in UTC before connecting
NautilusTrader. The adapter does not convert these timestamps automatically at runtime.

## Documentation

- [Crate docs](https://docs.rs/nautilus-interactive-brokers): generated Rust API reference.
- [Interactive Brokers integration guide](https://nautilustrader.io/docs/nightly/integrations/ib/):
  setup, configuration, symbology, and usage.
- [Rust node examples](examples): live data and execution testers.
- [Python live‑node examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/interactive_brokers):
  Python configuration examples.

## License

The source code for NautilusTrader is available on GitHub under the
[GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).

---

NautilusTrader™ is developed and maintained by Nautech Systems, a technology
company specializing in the development of high‑performance trading systems.
For more information, visit <https://nautilustrader.io>.

Use of this software is subject to the [Disclaimer](https://nautilustrader.io/legal/disclaimer/).

<img src="https://github.com/nautechsystems/nautilus_trader/raw/develop/assets/nautilus-logo-white.png" alt="logo" width="300" height="auto"/>

© 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
