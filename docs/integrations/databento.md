# Databento

NautilusTrader includes an adapter for the [Databento](https://databento.com/) API and
[Databento Binary Encoding (DBN)](https://databento.com/docs/standards-and-conventions/databento-binary-encoding) format data.
Databento is a market data provider only. The adapter does not include an execution client,
but you can pair it with a sandbox for simulated execution.
You can also match Databento data with Interactive Brokers execution,
or calculate traditional asset class signals for crypto trading.

The adapter supports:

- Loading historical data from DBN files and decoding to Nautilus objects for backtesting or catalog storage.
- Requesting historical data decoded to Nautilus objects for live trading and backtesting.
- Subscribing to real-time data feeds decoded to Nautilus objects for live trading and sandbox environments.

:::tip
[Databento](https://databento.com/signup) offers 125 USD in free data credits (historical only) for new sign-ups.

With careful requests, this covers testing and evaluation.
Check the [/metadata.get_cost](https://databento.com/docs/api-reference-historical/metadata/metadata-get-cost)
endpoint before requesting data.
:::

## Overview

The adapter uses the [databento-rs](https://crates.io/crates/databento) crate,
Databento's official Rust client library.

:::info
No separate `databento` installation is needed. The adapter compiles as a static
library and links automatically during the build.
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

Live example scripts are available [here](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/databento/).

## Databento documentation

See the [Databento new users guide](https://databento.com/docs/quickstart/new-user-guides).
Refer to it alongside this integration guide.

## Databento Binary Encoding (DBN)

Databento Binary Encoding (DBN) is a fast message encoding and storage format for
normalized market data. The [DBN specification](https://databento.com/docs/standards-and-conventions/databento-binary-encoding)
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
| [OHLCV_EOD](https://databento.com/docs/schemas-and-data-formats/ohlcv-eod)    | `Bar`                             | End‑of‑day bars.                |
| [DEFINITION](https://databento.com/docs/schemas-and-data-formats/definition)  | `Instrument` (various types)      | Instrument definitions.         |
| [IMBALANCE](https://databento.com/docs/schemas-and-data-formats/imbalance)    | `DatabentoImbalance`              | Auction imbalance data.         |
| [STATISTICS](https://databento.com/docs/schemas-and-data-formats/statistics)  | `DatabentoStatistics`             | Market statistics.              |
| [STATUS](https://databento.com/docs/schemas-and-data-formats/status)          | `InstrumentStatus`                | Market status updates.          |

### Schema considerations

- **TBBO and TCBBO**: Trade-sampled feeds that pair every trade with the BBO immediately *before* the trade's effect (TBBO per-venue, TCBBO consolidated). Use when you need trades aligned with contemporaneous quotes without managing two streams.
- **MBP-1 and CMBP-1 (L1)**: Event-level updates; emit trades only on trade events. Choose for a complete top-of-book event tape. For quote+trade alignment, prefer TBBO/TCBBO; otherwise use TRADES.
- **MBP-10 (L2)**: Top 10 levels with trades. Lighter than MBO for depth-aware strategies. Includes orders per level.
- **MBO (L3)**: Per-order events for queue position modeling and exact book reconstruction. Highest volume/cost; start at node initialization for proper replay context.
- **BBO_1S/BBO_1M and CBBO_1S/CBBO_1M**: Sampled top-of-book quotes at fixed intervals (1s/1m), no trades. Good for monitoring, spreads, and low-cost signals. Not suited for microstructure work.
- **TRADES**: Trades only. Pair with MBP-1 (`include_trades=True`) or use TBBO/TCBBO for quote context with trades.
- **OHLCV_ (incl. OHLCV_EOD)**: Aggregated bars from trades. Use for higher-timeframe analytics. Set `bars_timestamp_on_close=True` for close timestamps.
- **Imbalance / Statistics / Status**: Venue operational data; subscribe via `subscribe_data` with a `DataType` carrying `instrument_id` metadata.

:::tip
Consolidated schemas (CMBP_1, CBBO_1S, CBBO_1M, TCBBO) aggregate data across
multiple venues. Useful for cross-venue analysis.
:::

:::info
See also the Databento [Schemas and data formats](https://databento.com/docs/schemas-and-data-formats) guide.
:::

## Schema selection for live subscriptions

Nautilus subscription methods map to Databento schemas as follows:

| Nautilus Subscription Method    | Default Schema | Available Databento Schemas                                                  | Nautilus Data Type |
|:--------------------------------|:---------------|:-----------------------------------------------------------------------------|:-------------------|
| `subscribe_quote_ticks()`       | `mbp-1`        | `mbp-1`, `bbo-1s`, `bbo-1m`, `cmbp-1`, `cbbo-1s`, `cbbo-1m`, `tbbo`, `tcbbo` | `QuoteTick`        |
| `subscribe_trade_ticks()`       | `trades`       | `trades`, `tbbo`, `tcbbo`, `mbp-1`, `cmbp-1`                                 | `TradeTick`        |
| `subscribe_order_book_depth()`  | `mbp-10`       | `mbp-10`                                                                     | `OrderBookDepth10` |
| `subscribe_order_book_deltas()` | `mbo`          | `mbo`                                                                        | `OrderBookDeltas`  |
| `subscribe_bars()`              | varies         | `ohlcv-1s`, `ohlcv-1m`, `ohlcv-1h`, `ohlcv-1d`                               | `Bar`              |

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

### Quote subscriptions (MBP / L1)

```python
# Default MBP-1 quotes (may include trades)
self.subscribe_quote_ticks(instrument_id, client_id=DATABENTO_CLIENT_ID)

# Explicit MBP-1 schema
self.subscribe_quote_ticks(
    instrument_id=instrument_id,
    params={"schema": "mbp-1"},
    client_id=DATABENTO_CLIENT_ID,
)

# 1-second BBO snapshots (quotes only, no trades)
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

# Trade-sampled BBO (includes both quotes AND trades)
self.subscribe_quote_ticks(
    instrument_id=instrument_id,
    params={"schema": "tbbo"},  # Will receive both QuoteTick and TradeTick onto the message bus
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

### Order book depth subscriptions (MBP / L2)

```python
# Subscribe to top 10 levels of market depth
self.subscribe_order_book_depth(
    instrument_id=instrument_id,
    depth=10  # MBP-10 schema is automatically selected
)

# The depth parameter must be 10 for Databento
# This will receive OrderBookDepth10 updates
```

### Order book deltas subscriptions (MBO / L3)

```python
# Subscribe to full order book updates (market by order)
self.subscribe_order_book_deltas(
    instrument_id=instrument_id,
    book_type=BookType.L3_MBO  # Uses MBO schema
)

# Note: MBO subscriptions must be made at node startup for Databento
# to ensure proper replay from session start
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

# Subscribe to daily bars with end-of-day schema (only valid for DAY aggregation)
self.subscribe_bars(
    bar_type=BarType.from_str(f"{instrument_id}-1-DAY-LAST-EXTERNAL"),
    params={"schema": "ohlcv-eod"},  # Override to use end-of-day bars
)
```

### Custom data type subscriptions

Imbalance, statistics, and status data require the generic `subscribe_data` method:

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

# Subscribe to instrument status updates
from nautilus_trader.model.data import InstrumentStatus
self.subscribe_data(
    data_type=DataType(InstrumentStatus, metadata={"instrument_id": instrument_id}),
    client_id=DATABENTO_CLIENT_ID,
)
```

## Instrument IDs and symbology

Databento market data includes an `instrument_id` field: an integer assigned by
the source venue or by Databento during normalization. This differs from the
Nautilus `InstrumentId`, a string of symbol + venue separated by a period:
`"{symbol}.{venue}"`.

The decoder maps the Databento `raw_symbol` to the Nautilus `symbol` and uses an
[ISO 10383 MIC](https://www.iso20022.org/market-identifier-codes) (Market Identifier Code) from the
definition message for the Nautilus `venue`.

Databento identifies datasets with a *dataset ID*, separate from venue identifiers.
See [Databento dataset naming conventions](https://databento.com/docs/api-reference-historical/basics/datasets)
for details.

For CME Globex MDP 3.0 (`GLBX.MDP3`), these exchanges group under the `GLBX` venue.
The instrument's `exchange` field determines the mapping:

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
- `ts_out`: Databento sending timestamp.

Nautilus data requires at least two timestamps (per the `Data` contract):

- `ts_event`: UNIX timestamp (nanoseconds) when the data event occurred.
- `ts_init`: UNIX timestamp (nanoseconds) when the data instance was created.

The decoder maps Databento `ts_recv` to Nautilus `ts_event`. This timestamp is
more reliable and monotonically increases per instrument. The exceptions are
`DatabentoImbalance` and `DatabentoStatistics`, which carry all timestamp fields
since they are adapter-specific types.

:::info
See the following Databento docs for further information:

- [Databento standards and conventions - timestamps](https://databento.com/docs/standards-and-conventions/common-fields-enums-types#timestamps)
- [Databento timestamping guide](https://databento.com/docs/architecture/timestamping-guide)

:::

## Data types

This section covers Databento schema to Nautilus data type mapping.

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
| Bond                       | `B`  | Not yet available        |

### Price precision

Databento raw prices are fixed-point integers scaled by 1e-9. The adapter derives
price precision from the instrument's tick size in the definition message.

For live feeds, the feed handler maintains a per-instrument precision map populated
from `InstrumentDefMsg` records as they arrive. Market data handlers look up
precision from this map. Without a prior definition, precision falls back to 2
(USD default).

**Instrument definitions must arrive before market data** for correct precision on
instruments with non-standard tick sizes (e.g., treasury futures with fractional
ticks like 1/256). Subscribe to `DEFINITION` schema for your instruments before
or alongside market data subscriptions.

For historical and file-based loading, pass an explicit `price_precision` parameter
to override the default.

:::tip
The Python adapter automatically subscribes to instrument definitions before
market data, so the precision map populates without extra configuration. For
direct Rust client usage, subscribe to `DEFINITION` schema before market data.
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

#### Trade ID derivation (CMBP1 / TCBBO)

The CMBP1 and TCBBO schemas do not publish a native trade identifier. The
decoder derives a deterministic `TradeId` by FNV-1a hashing the instrument ID,
`ts_event`, `ts_recv`, price, size, and aggressor side of the trade. The same
venue event yields the same trade ID across replays, so downstream dedup stays
intact. Two logically distinct trades with identical fields collide; this
matches the venue's inability to distinguish them.

### OHLCV (bar aggregates)

Databento timestamps bar messages at the **open** of the interval. The decoder
normalizes `ts_event` to the bar **close** (original `ts_event` + interval).

### Imbalance & Statistics

The `imbalance` and `statistics` schemas have no built-in Nautilus equivalents.
The adapter defines `DatabentoImbalance` and `DatabentoStatistics` in Rust.

PyO3 bindings expose these types in Python. Their attributes are PyO3 objects
and may not be compatible with methods expecting Cython types. See the API
reference for PyO3 to Cython conversion methods.

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

Request the previous day's `statistics` for the `ES.FUT` parent symbol
(all active E-mini S&P 500 futures):

```python
from nautilus_trader.adapters.databento import DATABENTO_CLIENT_ID
from nautilus_trader.adapters.databento import DatabentoStatistics
from nautilus_trader.model import DataType

instrument_id = InstrumentId.from_str("ES.FUT.GLBX")
metadata = {
    "instrument_id": instrument_id,
    "start": "2024-03-06",
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
decoding. Requires the `arrow` feature flag.

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

# Step 1: Load instrument definitions FIRST
# You must obtain DEFINITION schema files from Databento for your instruments
instruments = loader.from_dbn_file(
    path=TEST_DATA_DIR / "databento" / "temp" / "tsla-xnas-definition.dbn.zst",
    as_legacy_cython=False,  # Use PyO3 for optimal performance
)

# Write instruments to catalog
catalog.write_data(instruments)

# Step 2: Now load and write market data
instrument_id = InstrumentId.from_str("TSLA.XNAS")

# Decode trades to pyo3 objects
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
print(catalog.instruments())  # Should show your loaded instruments
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
See also the [Data concepts guide](../concepts/data.md).
:::

### Historical loader options

Parameters for `from_dbn_file`:

- `instrument_id`: Speeds up decoding by skipping symbology lookup.
- `price_precision`: Overrides the default price precision.
- `include_trades`: For MBP-1/CMBP-1 schemas, `True` emits both `QuoteTick` and `TradeTick` when trade data is present.
- `as_legacy_cython`: Set to `False` for IMBALANCE/STATISTICS schemas (required) or for better catalog write performance.

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
    include_trades=True,  # Get both quotes and trades if available
    as_legacy_cython=True,
)

# Load consolidated BBO quotes
cbbo_quotes = loader.from_dbn_file(
    path="consolidated.cbbo-1s.dbn.zst",
    instrument_id=InstrumentId.from_str("AAPL.XNAS"),
    as_legacy_cython=False,  # Use PyO3 for better performance
)

# Load TCBBO (trade-sampled consolidated BBO) - provides both quotes and trades
# Note: include_trades=True loads quotes, include_trades=False loads trades
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
All MBO subscriptions for a dataset must be made at node startup to replay from
session start. Subscriptions after start are logged as errors and ignored.

This limitation does not apply to other schemas.
:::

A single `DatabentoHistoricalClient` serves both `DatabentoInstrumentProvider`
and `DatabentoDataClient` for historical requests.

## Configuration

Add a `DATABENTO` section to your `TradingNode` client configuration:

```python
from nautilus_trader.adapters.databento import DATABENTO
from nautilus_trader.live.node import TradingNode

config = TradingNodeConfig(
    ...,  # Omitted
    data_clients={
        DATABENTO: {
            "api_key": None,  # 'DATABENTO_API_KEY' env var
            "http_gateway": None,  # Override for the default HTTP historical gateway
            "live_gateway": None,  # Override for the default raw TCP real-time gateway
            "instrument_provider": InstrumentProviderConfig(load_all=True),
            "instrument_ids": None,  # Nautilus instrument IDs to load on start
            "parent_symbols": None,  # Databento parent symbols to load on start
        },
    },
    ..., # Omitted
)
```

Create the `TradingNode` and register the factory:

```python
from nautilus_trader.adapters.databento.factories import DatabentoLiveDataClientFactory
from nautilus_trader.live.node import TradingNode

# Instantiate the live trading node with a configuration
node = TradingNode(config=config)

# Register the client factory with the node
node.add_data_client_factory(DATABENTO, DatabentoLiveDataClientFactory)

# Finally build the node
node.build()
```

### Configuration parameters

| Option                    | Default | Description                                                                                                          |
|---------------------------|---------|----------------------------------------------------------------------------------------------------------------------|
| `api_key`                 | `None`  | Databento API secret. Falls back to the `DATABENTO_API_KEY` environment variable when `None`.                        |
| `http_gateway`            | `None`  | Historical HTTP gateway override for testing custom endpoints.                                                       |
| `live_gateway`            | `None`  | Raw TCP real‑time gateway override, typically for testing only.                                                       |
| `use_exchange_as_venue`   | `True`  | Use the exchange MIC for Nautilus venues (e.g., `XCME`). `False` retains the default GLBX mapping.                   |
| `timeout_initial_load`    | `15.0`  | Seconds to wait for instrument definitions per dataset before proceeding.                                            |
| `mbo_subscriptions_delay` | `3.0`   | Seconds to buffer before enabling MBO/L3 streams so initial snapshots replay in order.                               |
| `bars_timestamp_on_close` | `True`  | Timestamp bars on the close (`ts_event`/`ts_init`). `False` timestamps on the open.                                 |
| `reconnect_timeout_mins`  | `10`    | Minutes to attempt reconnection before giving up. `None` retries indefinitely. See [Connection stability](#connection-stability). |
| `venue_dataset_map`       | `None`  | Optional Nautilus venue to Databento dataset code mapping.                                                            |
| `parent_symbols`          | `None`  | Optional `{dataset: {parent symbols}}` to preload definition trees (e.g., `{"GLBX.MDP3": {"ES.FUT", "ES.OPT"}}`).   |
| `instrument_ids`          | `None`  | Nautilus `InstrumentId` values to preload definitions for at startup.                                                |

:::tip
Use environment variables for credentials.
:::

### Connection stability

The live client reconnects automatically on:

- **Network interruptions**: Temporary connectivity issues.
- **Gateway restarts**: Databento Sunday maintenance (see [Maintenance Schedule](https://databento.com/docs/api-reference-live/basics#maintenance-schedule)).
- **Market closures**: Sessions ending during off-hours.

#### Reconnection strategy

Backoff strategy depends on the timeout configuration:

**With timeout** (default 10 minutes):

- Exponential backoff capped at **60 seconds**.
- Pattern: 1s, 2s, 4s, 8s, 16s, 32s, 60s, 60s... (with jitter).
- Reconnects quickly within the timeout window.

**Without timeout** (`reconnect_timeout_mins=None`):

- Exponential backoff capped at **10 minutes**.
- Pattern: 1s, 2s, 4s, 8s, 16s, 32s, 64s, 128s, 256s, 512s, 600s, 600s... (with jitter).
- Suited for unattended systems through overnight closures and scheduled maintenance.

All reconnections include:

- **Jitter**: Random delay (up to 1 second) to prevent simultaneous reconnection storms.
- **Automatic resubscription**: Restores all active subscriptions after reconnecting.
- **Cycle reset**: Each successful session (>60s) resets the timeout clock.

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

Databento restarts live gateways every Sunday (all clients disconnect):

| Dataset            | Maintenance Time (UTC) |
|--------------------|------------------------|
| CME Globex         | 09:30                  |
| All ICE venues     | 09:45                  |
| All other datasets | 10:30                  |

The default 10-minute timeout covers typical restarts. For unattended systems,
use `reconnect_timeout_mins=None` or a longer value. See the
[Databento Maintenance Schedule](https://databento.com/docs/api-reference-live/basics/maintenance-schedule)
for details.

## Contributing

:::info
To contribute, see the
[contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md).
:::
