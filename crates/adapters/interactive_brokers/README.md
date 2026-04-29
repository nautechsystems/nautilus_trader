# Interactive Brokers Adapter

`nautilus-interactive-brokers` is the Rust adapter that integrates
NautilusTrader with Interactive Brokers TWS and IB Gateway.

The crate wraps the `ibapi` client and connects it to NautilusTrader's live
data, execution, historical data, and instrument loading infrastructure. It
also provides optional PyO3 bindings so the same Rust implementation can be
exposed through `nautilus_trader`.

## What this crate provides

- `data`: `InteractiveBrokersDataClient` for market data subscriptions and live
  streaming.
- `execution`: `InteractiveBrokersExecutionClient` for order submission,
  account synchronization, and execution updates.
- `historical`: `HistoricalInteractiveBrokersClient` for historical data
  requests.
- `providers`: `InteractiveBrokersInstrumentProvider` for contract lookup,
  instrument normalization, and symbology conversion.
- `gateway`: `DockerizedIBGateway` for managing a Dockerized IB Gateway when
  the `gateway` feature is enabled.
- `python`: PyO3 bindings exposed as
  `nautilus_pyo3.interactive_brokers` when the `python` feature is enabled.

## Feature flags

- `python`: Enables PyO3 bindings and Python-facing config and client types.
- `gateway`: Enables Dockerized IB Gateway support via `bollard`.
- `extension-module`: Builds the crate as a Python extension module. This is
  the feature used by the `nautilus_trader` package and includes `python` and
  `gateway`.

## Documentation and examples

- Full Interactive Brokers integration guide:
  [`docs/integrations/ib.md`](../../../docs/integrations/ib.md)
- Legacy live-node examples:
  [`examples/live/interactive_brokers`](../../../examples/live/interactive_brokers)
- PyO3 compatibility examples:
  [`examples/live/interactive_brokers_pyo3`](../../../examples/live/interactive_brokers_pyo3)

## Default ports

Use `127.0.0.1` unless you are connecting to a remote host.

| Endpoint | Trading mode | Default port |
|---|---|---:|
| IB Gateway | Paper | `4002` |
| IB Gateway | Live | `4001` |
| TWS | Paper | `7497` |
| TWS | Live | `7496` |
| Dockerized IB Gateway | Paper | `4002` |
| Dockerized IB Gateway | Live | `4001` |

This crate defaults to `4002`, which matches paper-trading IB Gateway and the
default Dockerized IB Gateway paper setup. If you are connecting to TWS or to a
live Gateway session, set the port explicitly in your config.

## Timestamp requirement

This adapter only supports UTC timestamps.

Configure TWS or IB Gateway to return timestamps in UTC before connecting
NautilusTrader. This is a user-managed setting in TWS / IB Gateway, not
something the adapter converts automatically at runtime.

## Status

This crate is the Rust implementation of NautilusTrader's Interactive Brokers
integration and is part of the ongoing migration from the legacy Python/Cython
adapter to the PyO3-based stack. The core adapter surface is already present,
but APIs may continue to evolve as the migration is completed.
