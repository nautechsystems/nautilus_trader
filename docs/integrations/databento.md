# Databento

NautilusTrader includes an adapter for the [Databento](https://databento.com/) API
and for data in
[Databento Binary Encoding (DBN)](https://databento.com/docs/standards-and-conventions/databento-binary-encoding).
Databento is a market data provider only. The adapter does not include an execution client,
but you can pair it with a sandbox for simulated execution.
You can also match Databento data with Interactive Brokers execution,
or calculate traditional asset class signals for crypto trading.

The adapter supports:

- Loading historical data from DBN files and decoding to Nautilus objects for backtesting or catalog storage.
- Requesting historical data decoded to Nautilus objects for live trading and backtesting.
- Subscribing to real-time data feeds decoded to Nautilus objects for live trading and sandbox environments.

:::tip
[Databento](https://databento.com/signup) offers $125 in free data credits for
new sign-ups. Databento currently allows those credits for historical data or
toward the first month of a subscription plan.

With careful requests, this covers testing and evaluation. Check the
[/metadata.get_cost](https://databento.com/docs/api-reference-historical/metadata/metadata-get-cost)
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
- `DatabentoInstrumentProvider`: Fetches latest or historical instrument definitions via the Databento HTTP API.
- `DatabentoHistoricalClient`: Fetches historical market data via the Databento HTTP API.
- `DatabentoLiveClient`: Subscribes to real-time data feeds via Databento's raw TCP API.
- `DatabentoDataClient`: `LiveMarketDataClient` implementation for live trading nodes.

:::info
Most users configure a live trading node (covered below) and do not work with
these components directly.
:::

## Examples

See the [live examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/databento/).

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

| Databento schema                                                              | Nautilus data type                | Description                     |
|:------------------------------------------------------------------------------|:----------------------------------|:--------------------------------|
| [MBO](https://databento.com/docs/schemas-and-data-formats/mbo)                | `OrderBookDelta`                  | Market by order (L3).           |
| [MBP_1](https://databento.com/docs/schemas-and-data-formats/mbp-1)            | `(QuoteTick, TradeTick \| None)`  | Market by price (L1).           |
| [MBP_10](https://databento.com/docs/schemas-and-data-formats/mbp-10)          | `OrderBookDepth10`                | Market depth (L2).              |
| [BBO_1S](https://databento.com/docs/schemas-and-data-formats/bbo-1s)          | `QuoteTick`                       | 1-second best bid/offer.        |
| [BBO_1M](https://databento.com/docs/schemas-and-data-formats/bbo-1m)          | `QuoteTick`                       | 1-minute best bid/offer.        |
| [CMBP_1](https://databento.com/docs/schemas-and-data-formats/cmbp-1)          | `(QuoteTick, TradeTick \| None)`  | Consolidated MBP across venues. |
| [CBBO_1S](https://databento.com/docs/schemas-and-data-formats/cbbo-1s)        | `QuoteTick`                       | Consolidated 1-second BBO.      |
| [CBBO_1M](https://databento.com/docs/schemas-and-data-formats/cbbo-1m)        | `QuoteTick`                       | Consolidated 1-minute BBO.      |
| [TCBBO](https://databento.com/docs/schemas-and-data-formats/tcbbo)            | `(QuoteTick, TradeTick)`          | Trade‑sampled consolidated BBO. |
| [TBBO](https://databento.com/docs/schemas-and-data-formats/tbbo)              | `(QuoteTick, TradeTick)`          | Trade‑sampled best bid/offer.   |
| [TRADES](https://databento.com/docs/schemas-and-data-formats/trades)          | `TradeTick`                       | Trade ticks.                    |
| [OHLCV_1S](https://databento.com/docs/schemas-and-data-formats/ohlcv-1s)      | `Bar`                             | 1-second bars.                  |
| [OHLCV_1M](https://databento.com/docs/schemas-and-data-formats/ohlcv-1m)      | `Bar`                             | 1-minute bars.                  |
| [OHLCV_1H](https://databento.com/docs/schemas-and-data-formats/ohlcv-1h)      | `Bar`                             | 1-hour bars.                    |
| [OHLCV_1D](https://databento.com/docs/schemas-and-data-formats/ohlcv-1d)      | `Bar`                             | Daily bars.                     |
| [DEFINITION](https://databento.com/docs/schemas-and-data-formats/definition)  | `Instrument` (various types)      | Instrument definitions.         |
| [IMBALANCE](https://databento.com/docs/schemas-and-data-formats/imbalance)    | `DatabentoImbalance`              | Auction imbalance data.         |
| [STATISTICS](https://databento.com/docs/schemas-and-data-formats/statistics)  | `DatabentoStatistics`             | Market statistics.              |
| [STATUS](https://databento.com/docs/schemas-and-data-formats/status)          | `InstrumentStatus`                | Market status updates.          |

:::note
Databento also documents reference schemas, including corporate actions,
adjustment factors, and security master data. This adapter currently maps only
the schemas listed above to Nautilus data types. Daily Databento OHLCV uses
`ohlcv-1d`. Official settlement prices and open interest come from the
`statistics` schema, not OHLCV bars.
:::

:::info
Instrument definitions for unsupported `instrument_class` values (`'I'` Index,
`'B'` Bond) are skipped with a warning rather than aborting the batch.
FX spot definitions with currencies that Nautilus cannot map are also skipped.
Index-emitting publishers include CGIF.TITANIUM (110), IEX Options (108), and
MEMX MX2 (109). Open an issue if you need Nautilus modeling for these.

Statistics messages with `stat_type` values outside the modeled range (currently
1-20) are also skipped with a warning. This includes venue-specific values
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
  that do not need full MBO data. Includes orders per level. Databento order
  book depth subscriptions support only `depth=10`.
- **MBO (L3)**: Per-order events for queue position modeling and exact book
  reconstruction. Start at node initialization for proper replay context.
- **BBO_1S/BBO_1M and CBBO_1S/CBBO_1M**: Sampled top-of-book updates at fixed
  intervals (1s or 1m). The adapter emits `QuoteTick` only for these schemas.
  Use them for monitoring, spreads, and low-cost signals. They are not suited
  for microstructure work.
- **TRADES**: Trades only. Pair with MBP-1 (`include_trades=True`) or use TBBO
  or TCBBO for quote context with trades.
- **OHLCV**: Aggregated bars from trades. Use them for higher-timeframe
  analytics. Set `bars_timestamp_on_close=True` for close timestamps. Daily
  bars use `ohlcv-1d`; use `statistics` for official settlements and open
  interest.
- **Imbalance and statistics**: Venue operational data. Subscribe via
  `subscribe_data` with a `DataType` carrying `instrument_id` metadata.
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

## Schema selection for live subscriptions

Nautilus subscription methods map to Databento schemas as follows:

| Nautilus subscription method    | Default schema | Available Databento schemas                                                  | Nautilus data type |
|:--------------------------------|:---------------|:-----------------------------------------------------------------------------|:-------------------|
| `subscribe_quote_ticks()`       | `mbp-1`        | `mbp-1`, `bbo-1s`, `bbo-1m`, `cmbp-1`, `cbbo-1s`, `cbbo-1m`, `tbbo`, `tcbbo` | `QuoteTick`        |
| `subscribe_trade_ticks()`       | `trades`       | `trades`, `tbbo`, `tcbbo`, `mbp-1`, `cmbp-1`                                 | `TradeTick`        |
| `subscribe_order_book_depth()`  | `mbp-10`       | `mbp-10`                                                                     | `OrderBookDepth10` |
| `subscribe_order_book_deltas()` | `mbo`          | `mbo`                                                                        | `OrderBookDeltas`  |
| `subscribe_bars()`              | varies         | `ohlcv-1s`, `ohlcv-1m`, `ohlcv-1h`, `ohlcv-1d`                               | `Bar`              |

:::warning
The "Available Databento schemas" column lists adapter-supported choices for
that Nautilus subscription method. The selected dataset must also support the
schema. For example, `EQUS.MINI` cannot serve `mbo`, `mbp-10`, `statistics`, or
`status`.
:::

:::note
The examples below assume a `Strategy` or `Actor` context where `self` has
subscription methods. Import the required types:

```python
from nautilus_trader.adapters.databento import DATABENTO_CLIENT_ID
from nautilus_trader.model import BarType
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.identifiers import InstrumentId
```

:::

### Quote subscriptions (MBP and L1)

```python
# Default MBP-1 quotes (may include trades)
self.subscribe_quote_ticks(instrument_id, client_id=DATABENTO_CLIENT_ID)

# Explicit MBP-1 schema
self.subscribe_quote_ticks(
    instrument_id=instrument_id,
    params={"schema": "mbp-1"},
    client_id=DATABENTO_CLIENT_ID,
)

# 1-second BBO snapshots (adapter emits QuoteTick only)
self.subscribe_quote_ticks(
    instrument_id=instrument_id,
    params={"schema": "bbo-1s"},
    client_id=DATABENTO_CLIENT_ID,
)

# Consolidated quotes across venues
self.subscribe_quote_ticks(
    instrument_id=instrument_id,
    params={"schema": "cbbo-1s"},  # or "cmbp-1" for consolidated MBP
    client_id=DATABENTO_CLIENT_ID,
)

# Trade-sampled BBO (includes quotes and trades)
self.subscribe_quote_ticks(
    instrument_id=instrument_id,
    params={"schema": "tbbo"},  # Receives QuoteTick and TradeTick on the message bus
    client_id=DATABENTO_CLIENT_ID,
)
```

### Trade subscriptions

```python
# Trade ticks only
self.subscribe_trade_ticks(instrument_id, client_id=DATABENTO_CLIENT_ID)

# Trades from MBP-1 feed (only when trade events occur)
self.subscribe_trade_ticks(
    instrument_id=instrument_id,
    params={"schema": "mbp-1"},
    client_id=DATABENTO_CLIENT_ID,
)

# Trade-sampled data (includes quotes at trade time)
self.subscribe_trade_ticks(
    instrument_id=instrument_id,
    params={"schema": "tbbo"},  # Also provides quotes at trade events
    client_id=DATABENTO_CLIENT_ID,
)
```

### Order book depth subscriptions (MBP and L2)

```python
# Subscribe to top 10 levels of market depth
self.subscribe_order_book_depth(
    instrument_id=instrument_id,
    depth=10  # MBP-10 schema is automatically selected
)

# The depth parameter must be 10 for Databento
# Receives OrderBookDepth10 updates
```

### Order book deltas subscriptions (MBO and L3)

```python
# Subscribe to full order book updates (market by order)
self.subscribe_order_book_deltas(
    instrument_id=instrument_id,
    book_type=BookType.L3_MBO  # Uses MBO schema
)

# Make MBO subscriptions at node startup so Databento can replay from session start
```

### Bar subscriptions

```python
# Subscribe to 1-minute bars (automatically uses ohlcv-1m schema)
self.subscribe_bars(
    bar_type=BarType.from_str(f"{instrument_id}-1-MINUTE-LAST-EXTERNAL")
)

# Subscribe to 1-second bars (automatically uses ohlcv-1s schema)
self.subscribe_bars(
    bar_type=BarType.from_str(f"{instrument_id}-1-SECOND-LAST-EXTERNAL")
)

# Subscribe to hourly bars (automatically uses ohlcv-1h schema)
self.subscribe_bars(
    bar_type=BarType.from_str(f"{instrument_id}-1-HOUR-LAST-EXTERNAL")
)

# Subscribe to daily bars (automatically uses ohlcv-1d schema)
self.subscribe_bars(
    bar_type=BarType.from_str(f"{instrument_id}-1-DAY-LAST-EXTERNAL")
)
```

### Custom data type subscriptions

Imbalance and statistics data require the generic `subscribe_data` method:

```python
from nautilus_trader.adapters.databento import DATABENTO_CLIENT_ID
from nautilus_trader.adapters.databento import DatabentoImbalance
from nautilus_trader.adapters.databento import DatabentoStatistics
from nautilus_trader.model import DataType

# Subscribe to imbalance data
self.subscribe_data(
    data_type=DataType(DatabentoImbalance, metadata={"instrument_id": instrument_id}),
    client_id=DATABENTO_CLIENT_ID,
)

# Subscribe to statistics data
self.subscribe_data(
    data_type=DataType(DatabentoStatistics, metadata={"instrument_id": instrument_id}),
    client_id=DATABENTO_CLIENT_ID,
)
```

Instrument status uses the dedicated status subscription API:

```python
# Subscribe to instrument status updates
self.subscribe_instrument_status(
    instrument_id=instrument_id,
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

Databento identifies datasets with a *dataset ID*, separate from venue identifiers.
See [Databento dataset naming conventions](https://databento.com/docs/api-reference-historical/basics/datasets)
for details.

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
uses the status event timestamp from the decoded status message.
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
|----------------------------|------|--------------------------|
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
loader.from_dbn_file(
    path,
    expiration_overrides={
        "OPRA.PILLAR": {"default": "16:00", "SPX": "09:30"},
    },
)
```

Times use `HH:MM` or `HH:MM:SS` in the exchange-local timezone (New York for OPRA). Only datasets
with a built-in correction rule (currently `OPRA.PILLAR`) can be tuned; an unknown or rule-less
dataset such as `GLBX.MDP3` raises a `ValueError`. The correction keys on the option's underlying, so
it cannot distinguish series that share an underlying but settle at different times (for example
AM-settled SPX versus PM-settled SPXW); set the time that matches the contracts you are loading.

### Price precision

Databento raw prices are fixed-point integers scaled by 1e-9. The adapter derives
price precision from the instrument's tick size in the definition message.

For live feeds, the feed handler maintains a per-instrument precision map populated
from `InstrumentDefMsg` records as they arrive. Market data handlers resolve
precision in this order:

1. `InstrumentDefMsg` metadata for the Databento record `instrument_id`.
2. Cached instrument precision passed by the Python subscription path.
3. Explicit `price_precisions` passed to the direct live client.
4. The USD default precision of 2.

The fallback maps are keyed by Databento record `instrument_id` after symbol
mapping, so parent, continuous, and other non-raw symbology requests can still
use cached or explicit precision until definition metadata arrives.

**Instrument definitions must arrive before market data** for correct precision on
instruments with non-standard tick sizes (e.g., treasury futures with fractional
ticks like 1/256). Subscribe to `DEFINITION` schema for your instruments before
or alongside market data subscriptions.

For historical requests and file-based loading, precision is resolved per
record in this order:

1. An explicit `price_precision` argument on the call.
2. A per-symbol cache populated by loading definitions (`load_instruments`
   on the file loader, `get_range_instruments` on the historical client) or
   by an explicit `set_price_precision(symbol, precision)` call.

The Python data client seeds the historical-client cache from the instrument
provider before every request, so already-loaded instruments need no extra
configuration. When precision cannot be resolved, loading fails with an
explicit error rather than silently defaulting to USD precision.

:::tip
The Python adapter automatically subscribes to instrument definitions before
market data and passes cached instrument precision as a fallback, so the
precision map populates without extra configuration. For direct Rust client
usage, subscribe to `DEFINITION` schema before market data or pass explicit
precision fallbacks.
:::

### MBO (market by order)

MBO is the highest granularity data from Databento, representing full order book
depth. Some messages include trade data. The decoder produces an `OrderBookDelta`
and optionally a `TradeTick`.

The live client buffers MBO messages until it sees an `F_LAST` flag, then passes
an `OrderBookDeltas` container to the handler.

The client also buffers order book snapshots into `OrderBookDeltas` during the
replay startup sequence.

### MBP-1 (market by price, top-of-book)

MBP-1 represents top-of-book quotes and trades. Some messages carry trade data.
The decoder produces a `QuoteTick` and also a `TradeTick` when the message is
a trade.

### TBBO and TCBBO (top-of-book with trades)

TBBO and TCBBO provide both quote and trade data in each message. Both schemas
emit `QuoteTick` and `TradeTick` per message, more efficient than separate quote
and trade subscriptions. TCBBO provides consolidated data across venues.

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
The adapter defines `DatabentoImbalance` and `DatabentoStatistics` in Rust.

PyO3 bindings expose these types in Python. Their attributes are PyO3 objects
and may not work with methods expecting Cython types. See the API reference for
PyO3 to Cython conversion methods.

Convert a PyO3 `Price` to a Cython `Price`:

```python
price = Price.from_raw(pyo3_price.raw, pyo3_price.precision)
```

Requesting and subscribing to these types requires the generic `subscribe_data`
method. Subscribe to `imbalance` for `AAPL.XNAS`:

```python
from nautilus_trader.adapters.databento import DATABENTO_CLIENT_ID
from nautilus_trader.adapters.databento import DatabentoImbalance
from nautilus_trader.model import DataType

instrument_id = InstrumentId.from_str("AAPL.XNAS")
self.subscribe_data(
    data_type=DataType(DatabentoImbalance, metadata={"instrument_id": instrument_id}),
    client_id=DATABENTO_CLIENT_ID,
)
```

Request a bounded range of `statistics` for the `ES.FUT` parent symbol
(all active E-mini S&P 500 futures). Use Databento's Historical
[`metadata.get_cost`](https://databento.com/docs/api-reference-historical/metadata/metadata-get-cost)
endpoint before real historical pulls:

```python
from nautilus_trader.adapters.databento import DATABENTO_CLIENT_ID
from nautilus_trader.adapters.databento import DatabentoStatistics
from nautilus_trader.model import DataType

instrument_id = InstrumentId.from_str("ES.FUT.GLBX")
metadata = {
    "instrument_id": instrument_id,
    "start": "2024-03-06",
    "end": "2024-03-07",
}
self.request_data(
    data_type=DataType(DatabentoStatistics, metadata=metadata),
    client_id=DATABENTO_CLIENT_ID,
)
```

### Catalog persistence

Both types support Arrow serialization for catalog storage. The Arrow serializers
register automatically when you import the adapter package.

#### Writing to the catalog

```python
from nautilus_trader.adapters.databento import DatabentoDataLoader
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.persistence.catalog import ParquetDataCatalog

catalog = ParquetDataCatalog.from_env()
loader = DatabentoDataLoader()

imbalances = loader.from_dbn_file(
    path="aapl-imbalance.dbn.zst",
    instrument_id=InstrumentId.from_str("AAPL.XNAS"),
    as_legacy_cython=False,  # Required for Databento-specific types
)

catalog.write_data(imbalances)
```

#### Reading from the catalog

```python
from nautilus_trader.adapters.databento import DatabentoImbalance

results = catalog.query(DatabentoImbalance, identifiers=["AAPL.XNAS"])

for imbalance in results:
    print(imbalance.ref_price)  # DatabentoImbalance fields
```

:::warning
Catalog persistence supports writing and querying these types, but streaming
them through `BacktestNode` or `BacktestEngine` is not yet supported. For
backtesting with imbalance or statistics data, query the catalog directly and
process the results in your strategy or analysis code.
:::

#### Encoding and decoding in Rust

The `nautilus_databento::arrow` module provides Arrow record batch encoding and
decoding. Enable the `arrow` feature flag.

```rust
use nautilus_databento::arrow::imbalance::{
    decode_imbalance_batch,
    imbalance_to_arrow_record_batch,
};

let batch = imbalance_to_arrow_record_batch(imbalances)?;

let metadata = batch.schema().metadata().clone();
let decoded = decode_imbalance_batch(&metadata, batch)?;
```

The `statistics` module follows the same pattern with
`decode_statistics_batch` and `statistics_to_arrow_record_batch`.

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
Performance benchmarks are under development.
:::

For live data, decoded delivery from the feed handler to Nautilus is
intentionally unbounded. This prevents slow consumers from stalling the feed
path; a process under memory pressure should fail rather than block live
decoding.

## Loading DBN data

The `DatabentoDataLoader` class loads DBN files and converts records to Nautilus
objects. Two primary uses:

- Pass data to `BacktestEngine.add_data` for backtesting.
- Write data to `ParquetDataCatalog` for streaming with a `BacktestNode`.

### DBN data to a BacktestEngine

Load DBN data and pass to a `BacktestEngine`. The engine requires an instrument.
This example uses `TestInstrumentProvider` (an instrument parsed from a DBN
file also works). The data covers one month of TSLA trades on Nasdaq:

```python
# Add instrument
TSLA_NASDAQ = TestInstrumentProvider.equity(symbol="TSLA")
engine.add_instrument(TSLA_NASDAQ)

# Decode data to Cython objects
loader = DatabentoDataLoader()
trades = loader.from_dbn_file(
    path=TEST_DATA_DIR / "databento" / "temp" / "tsla-xnas-20240107-20240206.trades.dbn.zst",
    instrument_id=TSLA_NASDAQ.id,
)

# Add data
engine.add_data(trades)
```

### DBN data to a ParquetDataCatalog

Load DBN data and write to a `ParquetDataCatalog`. Set `as_legacy_cython=False`
to decode as PyO3 objects.

### Loading instruments

**Important**: Load instrument definitions from DEFINITION schema files before
loading market data into a catalog. The catalog requires instruments before it
can store market data. Market data files do not contain instrument definitions.

```python
# Initialize the catalog interface
# (will use the `NAUTILUS_PATH` env var as the path)
catalog = ParquetDataCatalog.from_env()

loader = DatabentoDataLoader()

# Step 1: Load instrument definitions first
# Obtain DEFINITION schema files from Databento for your instruments
instruments = loader.from_dbn_file(
    path=TEST_DATA_DIR / "databento" / "temp" / "tsla-xnas-definition.dbn.zst",
    as_legacy_cython=False,  # Use PyO3 for optimal performance
)

# Write instruments to catalog
catalog.write_data(instruments)

# Step 2: Now load and write market data
instrument_id = InstrumentId.from_str("TSLA.XNAS")

# Decode trades to PyO3 objects
trades = loader.from_dbn_file(
    path=TEST_DATA_DIR / "databento" / "temp" / "tsla-xnas-20240107-20240206.trades.dbn.zst",
    instrument_id=instrument_id,
    as_legacy_cython=False,  # This is an optimization for writing to the catalog
)

# Write market data
catalog.write_data(trades)
```

#### Loading multiple data types for backtesting

Always load instruments before market data:

```python
from nautilus_trader.adapters.databento.loaders import DatabentoDataLoader
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.persistence.catalog import ParquetDataCatalog

catalog = ParquetDataCatalog.from_env()
loader = DatabentoDataLoader()

# Step 1: Load instrument definitions from DEFINITION files
instruments = loader.from_dbn_file(
    path="equity-definitions.dbn.zst",
    as_legacy_cython=False,
)
catalog.write_data(instruments)

# Step 2: Load market data (MBO, trades, quotes, etc.)
instrument_id = InstrumentId.from_str("AAPL.XNAS")

# Load MBO order book deltas
deltas = loader.from_dbn_file(
    path="aapl-mbo.dbn.zst",
    instrument_id=instrument_id,  # Optional but improves performance
    as_legacy_cython=False,
)
catalog.write_data(deltas)

# Load trades
trades = loader.from_dbn_file(
    path="aapl-trades.dbn.zst",
    instrument_id=instrument_id,
    as_legacy_cython=False,
)
catalog.write_data(trades)

# Verify instruments are in the catalog
print(catalog.instruments())  # Shows your loaded instruments
```

:::tip
Call `catalog.instruments()` to verify. An empty list means you need to load
DEFINITION files first.
:::

:::info
Download DEFINITION schema files through the Databento API or CLI for your
symbols and date ranges. See the
[Databento documentation](https://databento.com/docs/api-reference-historical/timeseries/timeseries-get-range)
for details.
:::

:::info
See also the [Data concepts guide](../concepts/data/).
:::

### Historical loader options

Parameters for `from_dbn_file`:

- `instrument_id`: Speeds up decoding by skipping symbology lookup.
- `price_precision`: Override applied to every record read. When omitted, the
  loader resolves precision per symbol from its cache (populated by
  `load_instruments` or `set_price_precision`); loading fails if unresolved.
- `include_trades`: For MBP-1/CMBP-1 schemas, `True` emits both `QuoteTick`
  and `TradeTick` when trade data is present.
- `as_legacy_cython`: Set to `False` for IMBALANCE/STATISTICS schemas
  (required) or for better catalog write performance.

:::warning
IMBALANCE and STATISTICS schemas require `as_legacy_cython=False` (PyO3-only
types). `True` raises a `ValueError`.
:::

### Loading consolidated data

Consolidated schemas aggregate data across multiple venues:

```python
# Load consolidated MBP-1 quotes
loader = DatabentoDataLoader()
cmbp_quotes = loader.from_dbn_file(
    path="consolidated.cmbp-1.dbn.zst",
    instrument_id=InstrumentId.from_str("AAPL.XNAS"),
    include_trades=True,  # Includes both quotes and trades if available
    as_legacy_cython=True,
)

# Load consolidated BBO quotes
cbbo_quotes = loader.from_dbn_file(
    path="consolidated.cbbo-1s.dbn.zst",
    instrument_id=InstrumentId.from_str("AAPL.XNAS"),
    as_legacy_cython=False,  # Use PyO3 for better performance
)

# Load TCBBO (trade-sampled consolidated BBO) with quotes and trades
# include_trades=True loads quotes, include_trades=False loads trades
tcbbo_quotes = loader.from_dbn_file(
    path="consolidated.tcbbo.dbn.zst",
    instrument_id=InstrumentId.from_str("AAPL.XNAS"),
    include_trades=True,  # Loads quotes
    as_legacy_cython=True,
)

tcbbo_trades = loader.from_dbn_file(
    path="consolidated.tcbbo.dbn.zst",
    instrument_id=InstrumentId.from_str("AAPL.XNAS"),
    include_trades=False,  # Loads trades
    as_legacy_cython=True,
)
```

:::tip
Avoid subscribing to both TBBO/TCBBO and separate trade feeds for the same
instrument. These schemas already include trades. Duplicating wastes cost and
creates duplicate data.
:::

## Real-time client architecture

The `DatabentoDataClient` wraps the other Databento adapter classes. Each
dataset uses two `DatabentoLiveClient` instances:

- One for MBO (order book deltas) real-time feeds
- One for all other real-time feeds

:::warning
Make all MBO subscriptions for a dataset at node startup to replay from session
start. The client logs subscriptions after start as errors and ignores them.

This limitation does not apply to other schemas.
:::

A single `DatabentoHistoricalClient` serves both `DatabentoInstrumentProvider`
and `DatabentoDataClient` for historical requests.

## Configuration

Add a `DATABENTO` section to your `TradingNode` client configuration. Load
specific instruments; the adapter does not support `load_all=True` for
Databento datasets because a dataset can contain millions of definitions.

```python
from nautilus_trader.adapters.databento import DATABENTO
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.model.identifiers import InstrumentId

instrument_ids = [
    InstrumentId.from_str("ESZ6.XCME"),  # GLBX.MDP3
    # InstrumentId.from_str("AAPL.EQUS"),  # EQUS.MINI
]

config = TradingNodeConfig(
    data_clients={
        DATABENTO: {
            "api_key": None,  # 'DATABENTO_API_KEY' env var
            "http_gateway": None,  # Override for the default HTTP historical gateway
            "live_gateway": None,  # Override for the default raw TCP real-time gateway
            "instrument_provider": InstrumentProviderConfig(
                load_ids=frozenset(instrument_ids),
            ),
            "instrument_ids": instrument_ids,  # Definitions to load on start
            "parent_symbols": {"GLBX.MDP3": {"ES.FUT"}},  # Optional definition trees
        },
    },
)
```

Create the `TradingNode` and register the factory:

```python
from nautilus_trader.adapters.databento.factories import DatabentoLiveDataClientFactory
from nautilus_trader.live.node import TradingNode

# Create the live trading node with the configuration
node = TradingNode(config=config)

# Register the client factory with the node
node.add_data_client_factory(DATABENTO, DatabentoLiveDataClientFactory)

# Build the node
node.build()
```

### Configuration parameters

| Option                    | Default | Description                                                           |
|---------------------------|---------|-----------------------------------------------------------------------|
| `api_key`                 | `None`  | Databento API secret; falls back to `DATABENTO_API_KEY`.              |
| `http_gateway`            | `None`  | Historical HTTP endpoint override, mainly for tests.                  |
| `live_gateway`            | `None`  | Live TCP endpoint override, mainly for tests.                         |
| `instrument_provider`     | default | Provider settings; use `load_ids`, not `load_all=True`.               |
| `use_exchange_as_venue`   | `True`  | Use exchange MIC venues for GLBX definitions.                         |
| `timeout_initial_load`    | `15.0`  | Definition load timeout per dataset, in seconds.                      |
| `mbo_subscriptions_delay` | `3.0`   | Delay before starting MBO/L3 streams, in seconds.                     |
| `bars_timestamp_on_close` | `True`  | Use bar close time for `ts_event`; `False` uses open.                 |
| `reconnect_timeout_mins`  | `10`    | Retry window in minutes; `None` retries indefinitely.                 |
| `venue_dataset_map`       | `None`  | Override venue‑to‑dataset mappings.                                   |
| `parent_symbols`          | `None`  | Preload parent definition trees by dataset.                           |
| `instrument_ids`          | `None`  | Definitions to preload at startup.                                    |

:::tip
Use environment variables for credentials.
:::

### Connection stability

The live client reconnects automatically on:

- **Network interruptions**: Temporary connectivity issues.
- **Gateway restarts**: Databento scheduled live gateway restarts. See the
  [maintenance schedule](https://databento.com/docs/api-reference-live/basics#maintenance-schedule).
- **Market closures**: Sessions ending during off-hours.

#### Reconnection strategy

Backoff strategy depends on the timeout configuration:

**With timeout** (default 10 minutes):

- Exponential backoff capped at **60 seconds**.
- Pattern: 1s, 2s, 4s, 8s, 16s, 32s, 60s, 60s, and so on (with jitter).
- Reconnects quickly within the timeout window.

**Without timeout** (`reconnect_timeout_mins=None`):

- Exponential backoff capped at **10 minutes**.
- Pattern: 1s, 2s, 4s, 8s, 16s, 32s, 64s, 128s, 256s, 512s, 600s, 600s,
  and so on (with jitter).
- Suited for unattended systems through overnight closures and scheduled maintenance.

All reconnections include:

- **Jitter**: Random delay (up to 1 second) to prevent simultaneous reconnection storms.
- **Automatic resubscription**: Restores all active subscriptions after reconnecting.
- **Cycle reset**: Each successful session (>60s) resets the timeout clock.

Individual unsubscribe requests log a warning and are ignored because Databento
live sessions do not support granular unsubscribe. Stop the session to remove a
subscription from the live gateway.

#### Timeout configuration

The `reconnect_timeout_mins` parameter controls how long the client attempts reconnection:

**Default (10 minutes)**: Suitable for most use cases.

- Handles transient network issues.
- Survives scheduled gateway restarts.
- Stops retrying overnight when markets close.
- Requires manual intervention for longer outages.

:::warning
Setting `reconnect_timeout_mins=None` retries indefinitely. Use only for
unattended systems that must survive overnight market closures. This can mask
persistent configuration or authentication issues.
:::

#### Scheduled maintenance

Databento restarts live gateways on this schedule (all clients disconnect):

| Dataset            | Restart time      |
|--------------------|-------------------|
| CME Globex         | Saturday 02:15 CT |
| All ICE venues     | Sunday 09:45 UTC  |
| All other datasets | Sunday 10:30 UTC  |

The default 10-minute timeout covers typical restarts. For unattended systems,
use `reconnect_timeout_mins=None` or a longer value. See the
[Databento Maintenance Schedule](https://databento.com/docs/api-reference-live/basics/maintenance-schedule)
for details.

## Contributing

:::info
To contribute, see the
[contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md).
:::
