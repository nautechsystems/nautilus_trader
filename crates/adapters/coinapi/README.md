# nautilus-coinapi

[NautilusTrader](https://nautilustrader.io) adapter for the
[CoinAPI Market Data API](https://www.coinapi.io/products/market-data-api?ref=nautilus).
API Bricks provides CoinAPI and FinFeedAPI.

The adapter provides authenticated REST and WebSocket access to cryptocurrency market data across
multiple exchanges. It supports native venue instrument definitions, live feeds, historical requests,
and loaders for downloaded flat files. It is implemented in Rust with optional Python bindings.
Order execution and account management are unsupported.

## NautilusTrader

[NautilusTrader](https://nautilustrader.io) is an open-source, production-grade, Rust-native
engine for multi-asset, multi-venue trading systems.

The system spans research, deterministic simulation, and live execution within a single
event-driven architecture, providing research-to-live semantic parity.

## Capabilities and limits

- **Instrument definitions**: Explicitly selected spot, perpetual, and dated futures instruments map
  to native venue IDs. Only the documented exchange and product forms are supported. Invalid,
  unsupported, or ambiguous definitions are skipped; a missing selected instrument fails connection.
- **Order books**: L2 market-by-price (MBP) and L3 market-by-order (MBO) deltas include snapshot gating,
  sequence gap recovery, and increasing output sequences across reconnects. L3 requires individual
  order IDs. Fixed-depth L2 snapshots use `book5`, `book20`, or `book50`, up to 50 levels.
  Every book subscription on a client must use the same channel.
- **Live feeds**: Quotes, trades, and supported `LAST` bars with `EXTERNAL` aggregation share one
  WebSocket connection.
  Bars publish when a later period arrives; retained periods are discarded on reconnect to avoid
  incomplete bars. Transport reconnects restore subscriptions; a CoinAPI `error` ends the streams.
- **History**: Trades, quotes, and bars require `start` and page through `[start, end)`, using now when
  `end` is absent. Results can be truncated with a warning when the local credit budget runs out.
  History can lag the live feed and does not fall back to latest-data endpoints. REST book requests
  return current L2 snapshots; historical book snapshots are unsupported.
- **Flat files**: Rust and Python load downloaded semicolon-delimited CSV exports for books, quotes,
  trades, and OHLCV. Trade, quote, and book loaders also support chunked streaming. The adapter does
  not list or download files from S3.

The data client converts prices and sizes exactly at instrument precision. Finer values are rejected, with a
[bounded exception for OHLCV volume](../../../docs/integrations/coinapi.md#precision).
Price precision can be overridden for selected instruments; size precision has no override.
Only exchange-reported trade sides set the aggressor; CoinAPI estimates map to `NO_AGGRESSOR`.

CoinAPI and native adapters must load disjoint instrument sets within a node. Separate client IDs
do not isolate definitions or feeds for the same instrument, and the adapter does not detect these
conflicts. CoinAPI data resolutions can differ from venue trading increments. See
[native adapter routing](../../../docs/integrations/coinapi.md#using-coinapi-with-native-adapters)
and the [capability matrix](../../../docs/integrations/coinapi.md#supported-capabilities).

## Configuration

Pass an API key explicitly or set `COINAPI_API_KEY`. The data client resolves it at construction and
reuses it on reconnect. REST requests and WebSocket upgrades send `X-CoinAPI-Key`; `hello` frames
carry no credential. Importing the Python package neither reads credentials nor connects.
REST redirects are rejected, ambient proxies are disabled, and `proxy_url` selects an explicit
HTTP(S) proxy for both transports. Production endpoints use HTTPS and WSS; sandbox access is unverified.

Configure explicit instruments with `CoinApiInstrumentSelection` and register
`CoinApiDataClientFactory` with a `LiveNode`. Python exports these types, `CoinApiDataClientConfig`,
`COINAPI`, `COINAPI_CLIENT_ID`, and flat-file loaders from `nautilus_trader.adapters.coinapi`.
The raw HTTP and WebSocket clients are Rust APIs. Use `COINAPI_CLIENT_ID` for requests and subscriptions
when routing alongside other data clients. Selections, price overrides, and transport controls are
validated at client construction.

The defaults allow **100 local REST credits over the client's lifetime**, with one request in flight.
Each metadata or current book snapshot attempt costs one credit. Historical trade, quote, and bar
attempts reserve `ceil(limit / 100)` credits; successful pages refund unused reservations above the
charge for returned records, with a minimum charge of one credit. Failed and canceled attempts keep
their reservations, and retries spend credits. All REST requests share the allowance, which does not
reset on reconnect.
It does not track provider billing or coordinate account-wide usage.

Set explicit history windows and record limits: an unlimited range can reserve all remaining credits
for its first page. WebSocket scope combines the session's data types, symbols, and bar periods, so
unrequested combinations can still incur billable transfer even though the adapter filters them out.
See [configuration and defaults](../../../docs/integrations/coinapi.md#configuration) and
[usage and access constraints](../../../docs/integrations/coinapi.md#usage-and-access-constraints).

## Feature flags

This crate provides feature flags to control source code inclusion during compilation,
depending on the intended use case:

- `examples`: Enables the crate's example binaries.
- `extension-module`: Builds as a Python extension module.
- `fuzz`: Enables libFuzzer integration for fuzz targets.
- `high-precision` (default): Enables
  [high-precision mode](https://nautilustrader.io/docs/nightly/getting_started/installation/#precision-mode)
  to use 128-bit value types.
- `python`: Enables Python bindings for identity, configuration, factories, and flat-file loaders
  through [PyO3](https://pyo3.rs).
- `simulation`: Routes HTTP and WebSocket through deterministic transport when built with
  `cfg(madsim)`.

## Integrator attribution

The adapter automatically sends the fixed referral header `X-Referrer: nautilus` on every REST
request and WebSocket upgrade request, including retries and reconnects. API Bricks records the
referral code and determines usage attribution on its side. The header carries no credential or
account data.

See the [CoinAPI integration guide](../../../docs/integrations/coinapi.md#integrator-attribution)
for signup referral links and attribution details.

## Fuzzing

Coverage-guided fuzz targets for WebSocket, book, and REST parsing live in [`fuzz/`](fuzz/README.md).
They require the workspace-pinned `cargo-fuzz` binary and a Rust nightly toolchain.

## Documentation

- [Integration guide](../../../docs/integrations/coinapi.md): Setup, symbology, API contracts, lifecycle,
  and provider limits.
- [Rust example](examples/node_data_tester.rs) and
  [Python example](../../../examples/live/coinapi/data_tester.py): Market-data collection and historical
  requests through `LiveNode` with a bounded REST credit budget.
- [Flat-file guide](../../../docs/integrations/coinapi.md#flat-files): CSV layouts, loading and streaming,
  timestamp rules, and precision handling.
- [Integration tests](tests/integration/): Data-client contracts, instrument conversion, protocol models,
  and deterministic transport checks.

## License

Licensed under the [GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).
