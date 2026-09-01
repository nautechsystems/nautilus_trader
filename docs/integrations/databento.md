# Databento

NautilusTrader includes an adapter for the [Databento](https://databento.com/) API
and for data in
[Databento Binary Encoding (DBN)](https://databento.com/docs/standards-and-conventions/databento-binary-encoding).
Databento is a market data provider only, so the adapter does not include an execution client. Pair
it with a sandbox for simulated execution, route execution through another adapter such as
Interactive Brokers, or use it to calculate traditional asset class signals for crypto trading.

The adapter supports:

- Loading historical data from DBN files and decoding to Nautilus objects for backtesting or catalog storage.
- Requesting historical data decoded to Nautilus objects for live trading and backtesting.
- Subscribing to real-time data feeds decoded to Nautilus objects for live trading and sandbox environments.

:::tip
[Databento](https://databento.com/signup) offers $125 in free data credits for new sign-ups.
Apply the credits to historical data requests, or offset them against a subscription plan. Credits
are shared across a team and expire six months after signup.

With careful requests, this covers testing and evaluation. Check the
[metadata.get_cost](https://databento.com/docs/api-reference-historical/metadata/metadata-get-cost)
endpoint before requesting data.
:::

## Overview

The adapter uses the [databento-rs](https://crates.io/crates/databento) crate,
Databento's official Rust client library.

:::info
You do not need to install `databento` separately. The adapter compiles as a
static library and links automatically during the build.
:::

The following adapter classes are available:

- `DatabentoDataLoader`: Loads DBN data from files.
- `DatabentoHistoricalClient`: Fetches historical market data and instrument definitions via the Databento HTTP API.
- `DatabentoLiveClient`: Subscribes to real-time data feeds via Databento's raw TCP API.
- `DatabentoDataClient`: Data client for live trading nodes, wrapping the historical and live clients.
- `DatabentoDataClientFactory`: Builds the data client from a `DatabentoDataClientConfig` for `LiveNode`.

:::info
Most users configure a live trading node (covered below) and do not work with
these components directly.
:::

## Examples

- [Python examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/databento/)

Rust examples live under
[`crates/adapters/databento/examples/`](https://github.com/nautechsystems/nautilus_trader/tree/develop/crates/adapters/databento/examples/).
The data tester subscribes to live quotes and trades for the configured instrument when run:

```bash
cargo run --example databento-data-tester --package nautilus-databento
```

## Databento documentation

See the [Databento new users guide](https://databento.com/docs/quickstart/new-user-guides).
Refer to it alongside this integration guide.

## Databento Binary Encoding (DBN)

Databento Binary Encoding (DBN) is a fast message encoding and storage format for
normalized market data. The [DBN spec](https://databento.com/docs/standards-and-conventions/databento-binary-encoding)
includes a self-describing metadata header and a fixed set of struct definitions
that standardize how market data is normalized.

The adapter decodes DBN data to Nautilus objects. The same Rust decoder handles:

- Loading and decoding DBN files from disk.
- Decoding historical and live data in real time.

## Supported schemas

The following Databento schemas are supported by NautilusTrader:

| Databento schema                                                             | Nautilus data type               | Description                     |
| :--------------------------------------------------------------------------- | :------------------------------- | :------------------------------ |
| [MBO](https://databento.com/docs/schemas-and-data-formats/mbo)               | `OrderBookDelta`                 | Market by order (L3).           |
| [MBP_1](https://databento.com/docs/schemas-and-data-formats/mbp-1)           | `(QuoteTick, TradeTick \| None)` | Market by price (L1).           |
| [MBP_10](https://databento.com/docs/schemas-and-data-formats/mbp-10)         | `OrderBookDepth10`               | Market depth (L2).              |
| [BBO_1S](https://databento.com/docs/schemas-and-data-formats/bbo-1s)         | `QuoteTick`                      | 1-second best bid/offer.        |
| [BBO_1M](https://databento.com/docs/schemas-and-data-formats/bbo-1m)         | `QuoteTick`                      | 1-minute best bid/offer.        |
| [CMBP_1](https://databento.com/docs/schemas-and-data-formats/cmbp-1)         | `(QuoteTick, TradeTick \| None)` | Consolidated MBP across venues. |
| [CBBO_1S](https://databento.com/docs/schemas-and-data-formats/cbbo-1s)       | `QuoteTick`                      | Consolidated 1-second BBO.      |
| [CBBO_1M](https://databento.com/docs/schemas-and-data-formats/cbbo-1m)       | `QuoteTick`                      | Consolidated 1-minute BBO.      |
| [TCBBO](https://databento.com/docs/schemas-and-data-formats/tcbbo)           | `(QuoteTick, TradeTick)`         | Trade-sampled consolidated BBO. |
| [TBBO](https://databento.com/docs/schemas-and-data-formats/tbbo)             | `(QuoteTick, TradeTick)`         | Trade-sampled best bid/offer.   |
| [TRADES](https://databento.com/docs/schemas-and-data-formats/trades)         | `TradeTick`                      | Trade ticks.                    |
| [OHLCV_1S](https://databento.com/docs/schemas-and-data-formats/ohlcv-1s)     | `Bar`                            | 1-second bars.                  |
| [OHLCV_1M](https://databento.com/docs/schemas-and-data-formats/ohlcv-1m)     | `Bar`                            | 1-minute bars.                  |
| [OHLCV_1H](https://databento.com/docs/schemas-and-data-formats/ohlcv-1h)     | `Bar`                            | 1-hour bars.                    |
| [OHLCV_1D](https://databento.com/docs/schemas-and-data-formats/ohlcv-1d)     | `Bar`                            | Daily bars.                     |
| [DEFINITION](https://databento.com/docs/schemas-and-data-formats/definition) | `Instrument` (various types)     | Instrument definitions.         |
| [IMBALANCE](https://databento.com/docs/schemas-and-data-formats/imbalance)   | `DatabentoImbalance`             | Auction imbalance data.         |
| [STATISTICS](https://databento.com/docs/schemas-and-data-formats/statistics) | `DatabentoStatistics`            | Market statistics.              |
| [STATUS](https://databento.com/docs/schemas-and-data-formats/status)         | `InstrumentStatus`               | Market status updates.          |

:::note
Databento also documents reference schemas, including corporate actions,
adjustment factors, and security master data. This adapter maps only the schemas
listed above to Nautilus data types. Daily Databento OHLCV uses `ohlcv-1d`, and
`ohlcv-eod` records also decode to daily bars. Official settlement prices and
open interest come from the `statistics` schema, not OHLCV bars.
:::

:::info
Instrument definitions for unsupported `instrument_class` values (`'I'` Index,
`'B'` Bond) are skipped with a warning rather than aborting the batch.
FX spot definitions with currencies that Nautilus cannot map are also skipped.
Index definitions come mainly from the Cboe Global Indices Feed (`MAIN.CGIF`)
and the OPRA options publishers; `publishers.json` lists every publisher ID with
its dataset and venue. Open an issue if you need Nautilus modeling for these.

Statistics messages with `stat_type` values outside the modeled range (1-20) are
also skipped with a warning. This includes the venue-specific values
`VenueSpecificVolume1` (10001) and `VenueSpecificPrice1` (10002), which exceed
the `u8` Arrow column width used for persistence.
:::

### Schema considerations

- **TBBO and TCBBO**: Trade-sampled feeds that pair every trade with the BBO
  immediately *before* the trade's effect. Use them for trades aligned with
  contemporaneous quotes without managing two streams.
- **MBP-1 and CMBP-1 (L1)**: Event-level updates that emit trades only on trade
  events. Choose them for a complete top-of-book event tape. For quote and trade
  alignment, prefer TBBO or TCBBO.
- **MBP-10 (L2)**: Top 10 levels with trades. Use it for depth-aware strategies
  that do not need full MBO data. Includes orders per level. Databento serves
  this schema at 10 levels only, so depth requests must use `depth=10`.
- **MBO (L3)**: Per-order events for queue position modeling and exact book
  reconstruction. `subscribe_book_deltas()` requests no snapshot, so a node
  subscription streams from the point of subscription with no initial book state.
  A strategy that needs a complete book must start before the trading session,
  seed the book from a historical request, or drive `DatabentoLiveClient.subscribe`
  directly with `snapshot=True`.
- **BBO_1S/BBO_1M and CBBO_1S/CBBO_1M**: Sampled top-of-book updates at fixed
  intervals (1s or 1m). The adapter emits `QuoteTick` only for these schemas.
  Use them for monitoring, spreads, and low-cost signals. They are not suited
  for microstructure work.
- **TRADES**: Trades only. For quote context alongside trades, subscribe with
  MBP-1, which emits a `TradeTick` on every trade event, or use TBBO or TCBBO.
- **OHLCV**: Aggregated bars from trades. Use them for higher-timeframe
  analytics. Bars carry close timestamps by default; set
  `bars_timestamp_on_close=False` to timestamp on the interval open. Daily
  bars use `ohlcv-1d`; use `statistics` for official settlements and open
  interest.
- **Imbalance and statistics**: Venue operational data with no built-in Nautilus
  equivalent. Reach them through the historical client, the data loader, or the
  direct live client, not through node subscriptions or requests (see
  [Imbalance and statistics](#imbalance-and-statistics)).
- **Status**: Venue trading-state updates. Subscribe via
  `subscribe_instrument_status`.

:::tip
Consolidated schemas (CMBP_1, CBBO_1S, CBBO_1M, TCBBO) aggregate data across
multiple venues. Useful for cross-venue analysis.
:::

:::info
See also the Databento [Schemas and data formats](https://databento.com/docs/schemas-and-data-formats) guide.
:::

## Dataset availability and selection

Databento dataset IDs are separate from Nautilus venue identifiers. The adapter
supports the schemas listed above, but each Databento dataset exposes its own
subset. Check the metadata endpoints before adding a new dataset or schema to a
live configuration:

```bash
databento_auth="$(printf '%s:' "$DATABENTO_API_KEY" | base64 | tr -d '\n')"

curl --header "Authorization: Basic ${databento_auth}" \
  "https://hist.databento.com/v0/metadata.list_schemas?dataset=EQUS.MINI"

curl --header "Authorization: Basic ${databento_auth}" \
  "https://hist.databento.com/v0/metadata.list_unit_prices?dataset=EQUS.MINI"

curl --header "Authorization: Basic ${databento_auth}" \
  "https://hist.databento.com/v0/metadata.get_cost" \
  --data-urlencode "dataset=EQUS.MINI" \
  --data-urlencode "symbols=AAPL" \
  --data-urlencode "stype_in=raw_symbol" \
  --data-urlencode "schema=bbo-1s" \
  --data-urlencode "start=2026-06-24T14:30:00Z" \
  --data-urlencode "end=2026-06-24T14:31:00Z"
```

For the two common evaluation datasets:

- `GLBX.MDP3` is the CME Globex MDP 3.0 dataset for CME, CBOT, NYMEX, and
  COMEX futures, options on futures, and spreads. It supports MBO, MBP-1,
  MBP-10, TBBO, trades, BBO intervals, OHLCV, definitions, statistics, and
  status. It does not expose the consolidated equity schemas (`cmbp-1`,
  `cbbo-*`, or `tcbbo`).
- `EQUS.MINI` is Databento US Equities Mini. It is a derived aggregated
  top-of-book dataset with anonymized component venues. It supports MBP-1,
  TBBO, trades, BBO intervals, OHLCV, and definitions. It does not support
  MBO, MBP-10, imbalance, statistics, status, or consolidated schemas.

Use `EQUS` as the Nautilus venue for US Equities Mini instruments:
`AAPL.EQUS`, `MSFT.EQUS`, and so on. The built-in venue-to-dataset map routes
`EQUS` to `EQUS.MINI`. Venue codes such as `XNAS` and `XNYS` refer to
venue-specific datasets unless you override them with `venue_dataset_map`.

:::warning
If you override a venue such as `XNAS` to `EQUS.MINI`, keep downstream
instrument IDs consistent. Mini records carry the consolidated `EQUS` publisher,
and file or historical decoding without an explicit `instrument_id` emits
`*.EQUS` identifiers.
:::

Cost depends on the schema, symbols, and time range. For exploration, start with
tight ranges, `definition`, `bbo-1s`, `bbo-1m`, or `trades`, and call
`metadata.get_cost` before pulling historical time series data. Avoid duplicate
quote and trade subscriptions when a combined schema such as `mbp-1` or `tbbo`
already carries the data needed by the strategy.

## Subscriptions and requests

Nautilus subscription methods map to Databento schemas as follows:

| Nautilus subscription method    | Default schema | Available Databento schemas                                                  | Nautilus data type |
| :------------------------------ | :------------- | :--------------------------------------------------------------------------- | :----------------- |
| `subscribe_instrument()`        | `definition`   | `definition`                                                                 | `Instrument`       |
| `subscribe_quotes()`            | `mbp-1`        | `mbp-1`, `bbo-1s`, `bbo-1m`, `cmbp-1`, `cbbo-1s`, `cbbo-1m`, `tbbo`, `tcbbo` | `QuoteTick`        |
| `subscribe_trades()`            | `trades`       | `trades`, `tbbo`, `tcbbo`, `mbp-1`, `cmbp-1`                                 | `TradeTick`        |
| `subscribe_book_deltas()`       | `mbo`          | `mbo`                                                                        | `OrderBookDeltas`  |
| `subscribe_instrument_status()` | `status`       | `status`                                                                     | `InstrumentStatus` |

Pass a non-default schema through the `schema` subscription parameter, as shown in the examples
below. Only `subscribe_quotes()` and `subscribe_trades()` accept a choice; the other methods always
use the single schema listed. The matching historical requests, `request_quotes()` and
`request_trades()`, take the same `schema` values and defaults.

:::warning
The "Available Databento schemas" column lists adapter-supported choices for
that Nautilus subscription method. The selected dataset must also support the
schema. For example, `EQUS.MINI` cannot serve `mbo`, `mbp-10`, `statistics`, or
`status`.
:::

:::warning
The live data client does not handle `subscribe_book_depth10()`, `subscribe_bars()`, or
`subscribe_data()`. Those commands log a "handler not implemented" warning and deliver no data.
Reach MBP-10 depth and OHLCV bars through historical requests (`request_book_depth()` and
`request_bars()`), and imbalance and statistics through the historical client or the data loader.
:::

:::note
The examples below assume a `Strategy` or `DataActor` context where `self` has
subscription methods. Import the required types:

```python
from nautilus_trader.model import BarType
from nautilus_trader.model import BookType
from nautilus_trader.model import ClientId
from nautilus_trader.model import InstrumentId


DATABENTO_CLIENT_ID = ClientId.from_str("DATABENTO")
instrument_id = InstrumentId.from_str("ES.c.0.GLBX")
```

:::

### Instrument definition subscriptions

```python
# Stream definition messages, which also populate the live price precision map
self.subscribe_instrument(
    instrument_id=instrument_id,
    client_id=DATABENTO_CLIENT_ID,
)
```

### Quote subscriptions (MBP and L1)

```python
# Default MBP-1 quotes (also emits trades on trade events)
self.subscribe_quotes(instrument_id, client_id=DATABENTO_CLIENT_ID)

# Explicit MBP-1 schema
self.subscribe_quotes(
    instrument_id=instrument_id,
    params={"schema": "mbp-1"},
    client_id=DATABENTO_CLIENT_ID,
)

# 1-second BBO snapshots (adapter emits QuoteTick only)
self.subscribe_quotes(
    instrument_id=instrument_id,
    params={"schema": "bbo-1s"},
    client_id=DATABENTO_CLIENT_ID,
)

# Consolidated quotes across venues
self.subscribe_quotes(
    instrument_id=instrument_id,
    params={"schema": "cbbo-1s"},  # or "cmbp-1" for consolidated MBP
    client_id=DATABENTO_CLIENT_ID,
)

# Trade-sampled BBO (includes quotes and trades)
self.subscribe_quotes(
    instrument_id=instrument_id,
    params={"schema": "tbbo"},  # Receives QuoteTick and TradeTick on the message bus
    client_id=DATABENTO_CLIENT_ID,
)
```

### Trade subscriptions

```python
# Trade ticks only
self.subscribe_trades(instrument_id, client_id=DATABENTO_CLIENT_ID)

# Trades from MBP-1 feed (only when trade events occur)
self.subscribe_trades(
    instrument_id=instrument_id,
    params={"schema": "mbp-1"},
    client_id=DATABENTO_CLIENT_ID,
)

# Trade-sampled data (includes quotes at trade time)
self.subscribe_trades(
    instrument_id=instrument_id,
    params={"schema": "tbbo"},  # Also provides quotes at trade events
    client_id=DATABENTO_CLIENT_ID,
)
```

### Order book deltas subscriptions (MBO and L3)

```python
# Subscribe to full order book updates (market by order)
self.subscribe_book_deltas(
    instrument_id=instrument_id,
    book_type=BookType.L3_MBO,  # Uses MBO schema
    client_id=DATABENTO_CLIENT_ID,
)

# Deltas stream from the point of subscription with no initial book snapshot
```

### Instrument status subscriptions

```python
# Subscribe to venue trading-state updates
self.subscribe_instrument_status(
    instrument_id=instrument_id,
    client_id=DATABENTO_CLIENT_ID,
)
```

### Historical requests for depth and bars

MBP-10 depth and OHLCV bars are available as historical requests. The bar aggregation in the
`BarType` selects the OHLCV schema (`ohlcv-1s`, `ohlcv-1m`, `ohlcv-1h`, or `ohlcv-1d`), and depth
requests use `mbp-10`:

```python
import pandas as pd


# Request 1-minute bars (uses the ohlcv-1m schema)
self.request_bars(
    bar_type=BarType.from_str(f"{instrument_id}-1-MINUTE-LAST-EXTERNAL"),
    start=pd.Timestamp("2024-03-06", tz="UTC"),
    end=pd.Timestamp("2024-03-07", tz="UTC"),
    client_id=DATABENTO_CLIENT_ID,
)

# Request top 10 levels of market depth (Databento serves depth=10 only)
self.request_book_depth(
    instrument_id=instrument_id,
    depth=10,
    start=pd.Timestamp("2024-03-06T14:30", tz="UTC"),
    end=pd.Timestamp("2024-03-06T14:31", tz="UTC"),
    client_id=DATABENTO_CLIENT_ID,
)
```

## Instrument IDs and symbology

Databento market data includes an `instrument_id` field: a numeric ID assigned
by the publisher in most cases, or synthesized by Databento when the publisher
does not provide one. Databento only guarantees this ID is unique within a given
day. This differs from the Nautilus `InstrumentId`, a string of symbol + venue
separated by a period: `"{symbol}.{venue}"`.

The decoder maps the Databento `raw_symbol` to the Nautilus `symbol`. Publisher
IDs map to the default Nautilus venue through `publishers.json`. Subscription
`InstrumentId` metadata can also seed the symbol-to-venue map before market data
arrives.

Dataset IDs follow Databento's
[dataset naming conventions](https://databento.com/docs/api-reference-historical/basics/datasets),
which are distinct from the venue code in a Nautilus `InstrumentId`.

For historical requests and live subscriptions, the adapter sends the Nautilus
symbol portion of each `InstrumentId` as the Databento symbol and infers
`stype_in` from that string:

- Symbols ending in `.FUT` or `.OPT` use Databento parent symbology, for example
  `ES.FUT.XCME`.
- Three-part symbols whose last part is numeric use continuous symbology, for
  example `ES.c.0.GLBX`.
- All-numeric symbols use Databento `instrument_id` symbology.
- All other symbols use raw symbol symbology, for example `ESZ6.XCME` or
  `AAPL.EQUS`.

All symbols in one request or subscription must use the same symbology type.
Batch `AAPL.EQUS` with `MSFT.EQUS`, or `ES.FUT.XCME` with `NQ.FUT.XCME`, but do
not mix raw and parent symbols in one Databento request.

For CME Globex MDP 3.0 (`GLBX.MDP3`), publisher defaults map to the `GLBX`
venue. When `use_exchange_as_venue=True`, definition messages can override
`GLBX` with the instrument's exchange MIC:

- `CBCM`: XCME-XCBT inter-exchange spread
- `NYUM`: XNYM-DUMX inter-exchange spread
- `XCBT`: Chicago Board of Trade (CBOT)
- `XCEC`: Commodities Exchange Center (COMEX)
- `XCME`: Chicago Mercantile Exchange (CME)
- `XFXS`: CME FX Link spread
- `XNYM`: New York Mercantile Exchange (NYMEX)

:::info
Other venue MICs are in the `venue` field of responses from
the [metadata.list_publishers](https://databento.com/docs/api-reference-historical/metadata/metadata-list-publishers) endpoint.
:::

## Timestamps

Databento data includes these timestamp fields:

- `ts_event`: Matching-engine-received timestamp in nanoseconds since the UNIX epoch.
- `ts_in_delta`: Matching-engine-sending timestamp in nanoseconds before `ts_recv`.
- `ts_recv`: Capture-server-received timestamp in nanoseconds since the UNIX epoch.
- `ts_out`: Databento sending timestamp (live only).

Nautilus data requires at least two timestamps (per the `Data` contract):

- `ts_event`: UNIX timestamp (nanoseconds) when the data event occurred.
- `ts_init`: UNIX timestamp (nanoseconds) when the data instance was created.

Quote and trade-like schemas map Databento `ts_recv` to Nautilus `ts_event`
because it is more reliable and monotonically increases per Databento symbol.
Bars use the DBN bar interval timestamp; `bars_timestamp_on_close` controls
whether Nautilus bars use the interval open or close timestamp. `InstrumentStatus`
uses the DBN record header `ts_event`.
`DatabentoImbalance` and `DatabentoStatistics` preserve Databento timestamp
fields because they are adapter-specific types.

:::info
See these Databento docs for details:

- [Databento standards and conventions - timestamps](https://databento.com/docs/standards-and-conventions/common-fields-enums-types#timestamps)
- [Databento timestamping guide](https://databento.com/docs/architecture/timestamping-guide)

:::

## Data types

This section maps Databento schemas to Nautilus data types.

:::info
See Databento [schemas and data formats](https://databento.com/docs/schemas-and-data-formats).
:::

### Instrument definitions

Databento uses a single schema for all instrument classes. The decoder maps each
to the appropriate Nautilus `Instrument` type.

| Databento instrument class | Code | Nautilus instrument type |
| -------------------------- | ---- | ------------------------ |
| Stock                      | `K`  | `Equity`                 |
| Future                     | `F`  | `FuturesContract`        |
| Call                       | `C`  | `OptionContract`         |
| Put                        | `P`  | `OptionContract`         |
| Future spread              | `S`  | `FuturesSpread`          |
| Option spread              | `T`  | `OptionSpread`           |
| Mixed spread               | `M`  | `OptionSpread`           |
| FX spot                    | `X`  | `CurrencyPair`           |
| Index                      | `I`  | Not yet available        |
| Bond                       | `B`  | Not yet available        |

### Option expiration correction

OPRA option definitions (dataset `OPRA.PILLAR`) carry the expiration with date-level precision: the
time-of-day is zeroed to midnight UTC. An option expiring at 16:00 New York time therefore arrives
stamped on the prior evening in New York, which makes the matching engine treat the contract as
expired before its final trading session. The loader corrects such midnight-UTC OPRA expirations to
16:00 New York time by default, leaving every other dataset (and any expiration that already carries
an intraday time, such as CME Globex) untouched.

Override the default, or set per-underlying times, with `expiration_overrides`. It maps a dataset to
a mapping of underlying symbol to time, where the reserved key `default` sets the dataset-wide time:

```python
loader.load_instruments(
    filepath=path,
    use_exchange_as_venue=False,
    expiration_overrides={
        "OPRA.PILLAR": {"default": "16:00", "SPX": "09:30"},
    },
)
```

Times use `HH:MM` or `HH:MM:SS` in the exchange-local timezone (New York for OPRA). Only a dataset
with a built-in correction rule can be tuned, and `OPRA.PILLAR` is the only such dataset; an unknown
or rule-less dataset such as `GLBX.MDP3` raises a `ValueError`. The correction keys on the option's underlying, so
it cannot distinguish series that share an underlying but settle at different times (for example
AM-settled SPX versus PM-settled SPXW); set the time that matches the contracts you are loading.

### Price precision

Databento raw prices are fixed-point integers scaled by 1e-9. The adapter derives
price precision from the instrument's tick size in the definition message.

For live feeds, the feed handler maintains a per-instrument precision map populated
from `InstrumentDefMsg` records as they arrive. Market data handlers resolve
precision in this order:

1. Precision from an `InstrumentDefMsg` already seen for the Databento record `instrument_id`.
2. Subscription-supplied precision matched to the record `instrument_id` through a symbol mapping message.
3. Subscription-supplied precision matched directly on the Nautilus symbol.
4. The USD default precision of 2.

Supply precision with the `price_precision` parameter on `subscribe_quotes()` or
`subscribe_trades()`, or with `price_precisions` on the direct live client. No other subscription
method reads the parameter. Steps 2 and 3 key the same override two ways: matching the record
`instrument_id` after symbol mapping lets parent, continuous, and other non-raw symbology
subscriptions apply the override before definition metadata arrives.

**Instrument definitions must arrive before market data** for correct precision on
instruments with non-standard tick sizes (e.g., treasury futures with fractional
ticks like 1/256). Subscribe to instrument definitions (the Databento `definition`
schema) before or alongside market data subscriptions.

For historical requests and file-based loading, precision is resolved per
record in this order:

1. An explicit `price_precision` argument on the call.
2. A per-symbol cache populated by loading definitions (`load_instruments`
   on the file loader, `get_range_instruments` on the historical client) or
   by an explicit `set_price_precision(symbol, precision)` call.

Before each historical request, the data client seeds this cache when the request
carries no explicit precision and the symbol has none cached: it fetches the
instrument definition for the requested `instrument_id` first. When precision
cannot be resolved, loading fails with an explicit error rather than silently
defaulting to USD precision.

:::tip
Call `subscribe_instrument()` for each instrument at strategy start so definition
messages populate the live precision map. The feed handler keeps a
`price_precision` override per symbol for the whole dataset session, so passing it
once on a quote or trade subscription also covers order book deltas for that
symbol. `InstrumentStatus` carries no prices and needs no precision.
:::

### MBO (market by order)

MBO is the highest granularity data from Databento, representing full order book
depth. Some messages include trade data. The decoder produces an `OrderBookDelta`
and optionally a `TradeTick`.

The live client buffers MBO messages until a record carries the `F_LAST` flag
closing the match event, then passes one `OrderBookDeltas` container to the
handler. Records that decode to no delta (a fill attribution or a status action)
can still carry `F_LAST`, so the client honors the raw flag independently of the
decoded payload; otherwise a partial event would be stranded and merged into the
next event.

Snapshot records (`F_SNAPSHOT`) accumulate into the same buffer and flush with the
first non-snapshot event boundary, so a snapshot reaches the handler as one
`OrderBookDeltas` container rather than as individual deltas. When a subscription
carries a replay `start` anchor, the client suppresses emission until an event
timestamp passes that anchor, which keeps replayed history out of the live stream.

### MBP-1 (market by price, top-of-book)

MBP-1 represents top-of-book quotes and trades. Some messages carry trade data.
The decoder produces a `QuoteTick` and also a `TradeTick` when the message is
a trade.

### TBBO and TCBBO (top-of-book with trades)

TBBO and TCBBO provide both quote and trade data in each message. Both schemas
emit a `TradeTick` per message plus a `QuoteTick`, more efficient than separate
quote and trade subscriptions. The quote is skipped when either the bid or ask
price is undefined. TCBBO provides consolidated data across venues.

#### Trade ID derivation (CMBP-1 and TCBBO)

The CMBP-1 and TCBBO schemas do not publish a native trade identifier. The
decoder derives a deterministic `TradeId` by FNV-1a hashing the instrument ID,
`ts_event`, `ts_recv`, price, size, and aggressor side of the trade. The same
venue event yields the same trade ID across replays, so downstream dedup stays
intact. Two logically distinct trades with identical fields collide; this
matches the venue's inability to distinguish them.

### OHLCV (bar aggregates)

Databento timestamps bar messages at the **open** of the interval. By default,
the decoder normalizes bar `ts_event` to the bar **close**: the original
`ts_event` plus the interval. `ts_init` uses the live receipt time, or the close
time for historical and file-based loads when no explicit init timestamp is
supplied. Set `bars_timestamp_on_close=False` to timestamp bar `ts_event` on
the interval open.

### Imbalance and statistics

The `imbalance` and `statistics` schemas have no built-in Nautilus equivalents.
The adapter defines `DatabentoImbalance` and `DatabentoStatistics` in Rust, and
Python bindings expose both types from `nautilus_trader.adapters.databento`.

The live data client does not route these types through node subscriptions or
requests. Reach them one of three ways:

- `DatabentoDataLoader.load_imbalance` and `load_statistics` for DBN files.
- `DatabentoHistoricalClient.get_range_imbalance` and `get_range_statistics` for historical ranges.
- `DatabentoLiveClient.subscribe` with the `imbalance` or `statistics` schema for live streams.

Request a bounded range of `statistics` for the `ES.FUT` parent symbol
(all active E-mini S&P 500 futures). Both `get_range_*` methods are asynchronous.
Use Databento's Historical
[`metadata.get_cost`](https://databento.com/docs/api-reference-historical/metadata/metadata-get-cost)
endpoint before real historical pulls:

```python
import os

from nautilus_trader.adapters.databento import DatabentoHistoricalClient
from nautilus_trader.core import dt_to_unix_nanos
from nautilus_trader.model import InstrumentId


client = DatabentoHistoricalClient(
    key=os.environ["DATABENTO_API_KEY"],
    publishers_filepath="publishers.json",
    use_exchange_as_venue=False,
)

statistics = await client.get_range_statistics(
    dataset="GLBX.MDP3",
    instrument_ids=[InstrumentId.from_str("ES.FUT.GLBX")],
    start=dt_to_unix_nanos("2024-03-06T00:00:00Z"),
    end=dt_to_unix_nanos("2024-03-07T00:00:00Z"),
    price_precision=2,
)
```

A fresh historical client holds no cached precision, so the request needs
`price_precision` or a preceding `get_range_instruments` call for the same range;
otherwise the first decoded record aborts the request. A parent symbol such as
`ES.FUT` cannot be seeded with `set_price_precision`, because records resolve to
the individual contract symbols behind the parent.

### Arrow encoding for imbalance and statistics

Both types implement Arrow record batch encoding and decoding. The
`nautilus_databento::arrow` module exposes it in Rust behind the `arrow` feature flag:

```rust
use nautilus_databento::arrow::imbalance::{
    decode_imbalance_batch,
    imbalance_to_arrow_record_batch,
};

let batch = imbalance_to_arrow_record_batch(&imbalances)?;

let metadata = batch.schema().metadata().clone();
let decoded = decode_imbalance_batch(&metadata, &batch)?;
```

The `statistics` module follows the same pattern with
`decode_statistics_batch` and `statistics_to_arrow_record_batch`. Call
`get_databento_arrow_schema_map(DatabentoImbalance)` from Python to inspect the
Arrow field map for either type.

:::warning
Neither type is registered with the `ParquetDataCatalog` custom data encoders, so
`write_custom_data` and `query_custom_data` fail with an unregistered-type error,
and neither type streams through `BacktestNode` or `BacktestEngine`. For research
with imbalance or statistics data, load or request the records and process them
directly.
:::

## Performance considerations

Two options for backtesting with DBN data:

- Store data as DBN (`.dbn.zst`) files and decode to Nautilus objects every run.
- Convert DBN files to Nautilus objects once and write to the data catalog (Nautilus Parquet format).

The DBN decoder is optimized Rust, but writing to the catalog once gives the
best backtest performance.

[DataFusion](https://arrow.apache.org/datafusion/) streams Nautilus Parquet data
from disk at high throughput, at least an order of magnitude faster than
decoding DBN per run.

:::note
Measured decode and client throughput for this adapter is recorded in
[`crates/adapters/databento/benches/BENCHMARKS.md`](https://github.com/nautechsystems/nautilus_trader/blob/develop/crates/adapters/databento/benches/BENCHMARKS.md),
along with the command that reproduces it. Absolute numbers vary by machine, so
only same-machine deltas are meaningful.
:::

For live data, decoded delivery from the feed handler to Nautilus is
intentionally unbounded. This prevents slow consumers from stalling the feed
path; a process under memory pressure should fail rather than block live
decoding.

## Loading DBN data

`DatabentoDataLoader` decodes DBN files directly into Nautilus objects. It exposes a method for
each supported output type, including `load_instruments`, `load_order_book_deltas`,
`load_order_book_depth10`, `load_quotes`, `load_trades`, `load_bars`, `load_status`,
`load_imbalance`, and `load_statistics`.

Pass the publisher metadata file when it is not available beside the running executable:

```python
from nautilus_trader.adapters.databento import DatabentoDataLoader
from nautilus_trader.model import InstrumentId


loader = DatabentoDataLoader(publishers_filepath="publishers.json")

instruments = loader.load_instruments(
    filepath="equity-definitions.dbn.zst",
    use_exchange_as_venue=True,
)
trades = loader.load_trades(
    filepath="aapl-trades.dbn.zst",
    instrument_id=InstrumentId.from_str("AAPL.XNAS"),
)
```

Write definition data before market data when writing to a `ParquetDataCatalog`, because the catalog
needs the instrument before it can write records for that instrument:

```python
from nautilus_trader.persistence import ParquetDataCatalog


catalog = ParquetDataCatalog(base_path="catalog")
catalog.write_instruments(instruments)
catalog.write_trade_ticks(trades)
```

Use the schema-specific methods for files whose schema is not the default for that output type:

- `load_bbo_quotes` for BBO interval quotes.
- `load_cmbp_quotes` for CMBP-1 quotes.
- `load_cbbo_quotes` for CBBO quotes.
- `load_tbbo_trades` for TBBO trades.
- `load_tcbbo_trades` for TCBBO trades.

Call `schema_for_file` to read a file's schema from its DBN metadata header when picking the loader
method.

Optional `instrument_id` and `price_precision` arguments bypass symbology or precision lookup when
those values are already known. The bar loader also accepts `timestamp_on_close`.

## Real-time client architecture

The `DatabentoDataClient` wraps the other Databento adapter classes. It creates
one live feed handler per dataset on the first subscription for that dataset, and
every schema for that dataset shares the handler. The handler runs a single async
task that races the next gateway record against the next engine command, so
subscriptions added later reach the running session without a reconnect.

A single `DatabentoHistoricalClient` serves every historical request from the
data client, including the instrument definitions fetched to seed price precision.

:::warning
Databento drops a replay `start` anchor sent after a live session has started, so
a subscription made mid-session streams from that point forward with no history.
The feed handler logs an error when it sees a late `start`, and it strips `start`
from stored subscriptions so a reconnect never replays history a second time.
:::

## Configuration

Create `DatabentoDataClientConfig` from the adapter's public Python module. The API key and
`publishers.json` path are required:

```python
import os
from pathlib import Path

from nautilus_trader.adapters.databento import DatabentoDataClientConfig


config = DatabentoDataClientConfig(
    api_key=os.environ["DATABENTO_API_KEY"],
    publishers_filepath=Path("publishers.json"),
    use_exchange_as_venue=False,
)
```

Download the canonical
[`publishers.json`](https://github.com/nautechsystems/nautilus_trader/blob/develop/crates/adapters/databento/publishers.json)
and point `publishers_filepath` at the local copy.

| Option                    | Default  | Description                                             |
| ------------------------- | -------- | ------------------------------------------------------- |
| `api_key`                 | Required | Databento API key.                                      |
| `publishers_filepath`     | Required | Local path to Databento publisher metadata.             |
| `use_exchange_as_venue`   | `False`  | Use exchange MIC venues for GLBX instruments.           |
| `bars_timestamp_on_close` | `True`   | Timestamp bars on close instead of the interval open.   |
| `venue_dataset_map`       | `None`   | Override venue-to-dataset mappings from publisher data. |

Use `DatabentoDataClientConfig` with `DatabentoDataClientFactory`. The current
[Python example](https://github.com/nautechsystems/nautilus_trader/blob/develop/examples/live/databento/data_tester.py)
shows the complete `LiveNode.builder(...)` configuration.

### Connection stability

The live client reconnects automatically on:

- **Network interruptions**: Temporary connectivity issues.
- **Gateway restarts**: Databento scheduled live gateway restarts. See the
  [maintenance schedule](https://databento.com/docs/api-reference-live/basics/maintenance-schedule).
- **Market closures**: Sessions ending during off-hours.

#### Reconnection strategy

The factory-backed live client uses an internal 10-minute reconnection window with exponential
backoff from 1 second, capped at 60 seconds. The Python `DatabentoDataClientConfig` constructor does
not expose a reconnection timeout. Once the window elapses without a successful session, the client
gives up and reports an error rather than retrying indefinitely.

Stalled connections are detected by the upstream Databento client, which raises a heartbeat timeout
when no data arrives within the heartbeat interval plus 5 seconds. The feed handler treats that as a
connection error and enters the same backoff loop.

All reconnections include:

- **Jitter**: Random delay (up to 1 second) to prevent simultaneous reconnection storms.
- **Automatic resubscription**: Restores all active subscriptions after reconnecting.
- **Cycle reset**: Each successful session (>60s) resets the timeout clock and the backoff delay.
- **Command buffering**: Commands received during backoff are applied to the next session.

Individual unsubscribe requests log a warning and are ignored because Databento
live sessions do not support granular unsubscribe. Stop the session to remove a
subscription from the live gateway.

#### Scheduled maintenance

Databento restarts live gateways on this schedule (all clients disconnect):

| Dataset            | Restart time      |
| ------------------ | ----------------- |
| CME Globex         | Saturday 02:15 CT |
| All ICE venues     | Sunday 09:45 UTC  |
| All other datasets | Sunday 10:30 UTC  |

The internal 10-minute timeout covers typical restarts. See the
[Databento Maintenance Schedule](https://databento.com/docs/api-reference-live/basics/maintenance-schedule)
for details.

## Contributing

:::info
To contribute, see the
[contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md).
:::
