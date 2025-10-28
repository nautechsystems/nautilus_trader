# Databento

NautilusTrader provides an adapter for integrating with the Databento API and [Databento Binary Encoding (DBN)](https://databento.com/docs/standards-and-conventions/databento-binary-encoding) format data.
As Databento is purely a market data provider, there is no execution client provided - although a sandbox environment with simulated execution could still be set up.
It's also possible to match Databento data with Interactive Brokers execution, or to calculate traditional asset class signals for crypto trading.

The capabilities of this adapter include:

- Loading historical data from DBN files and decoding into Nautilus objects for backtesting or writing to the data catalog.
- Requesting historical data which is decoded to Nautilus objects to support live trading and backtesting.
- Subscribing to real-time data feeds which are decoded to Nautilus objects to support live trading and sandbox environments.

:::tip
[Databento](https://databento.com/signup) currently offers 125 USD in free data credits (historical data only) for new account sign-ups.

With careful requests, this is more than enough for testing and evaluation purposes.
We recommend you make use of the [/metadata.get_cost](https://databento.com/docs/api-reference-historical/metadata/metadata-get-cost) endpoint.
:::

## Overview

The adapter implementation takes the [databento-rs](https://crates.io/crates/databento) crate as a dependency,
which is the official Rust client library provided by Databento.

:::info
There is **no** need for an optional extra installation of `databento`, as the core components of the
adapter are compiled as static libraries and linked automatically during the build process.
:::

The following adapter classes are available:

- `DatabentoDataLoader`: Loads Databento Binary Encoding (DBN) data from files.
- `DatabentoInstrumentProvider`: Integrates with the Databento API (HTTP) to provide latest or historical instrument definitions.
- `DatabentoHistoricalClient`: Integrates with the Databento API (HTTP) for historical market data requests.
- `DatabentoLiveClient`: Integrates with the Databento API (raw TCP) for subscribing to real-time data feeds.
- `DatabentoDataClient`: Provides a `LiveMarketDataClient` implementation for running a trading node in real time.

:::info
As with the other integration adapters, most users will simply define a configuration for a live trading node (covered below),
and won't need to necessarily work with these lower level components directly.
:::

## Examples

You can find live example scripts [here](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/databento/).

## Databento documentation

Databento provides extensive documentation for new users which can be found in the [Databento new users guide](https://databento.com/docs/quickstart/new-user-guides).
We recommend also referring to the Databento documentation in conjunction with this NautilusTrader integration guide.

## Databento Binary Encoding (DBN)

Databento Binary Encoding (DBN) is an extremely fast message encoding and storage format for normalized market data.
The [DBN specification](https://databento.com/docs/standards-and-conventions/databento-binary-encoding) includes a simple, self-describing metadata header and a fixed set of struct definitions,
which enforce a standardized way to normalize market data.

The integration provides a decoder which can convert DBN format data to Nautilus objects.

The same Rust implemented Nautilus decoder is used for:

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
| [TCBBO](https://databento.com/docs/schemas-and-data-formats/tcbbo)            | `(QuoteTick, TradeTick)`          | Trade-sampled consolidated BBO. |
| [TBBO](https://databento.com/docs/schemas-and-data-formats/tbbo)              | `(QuoteTick, TradeTick)`          | Trade-sampled best bid/offer.   |
| [TRADES](https://databento.com/docs/schemas-and-data-formats/trades)          | `TradeTick`                       | Trade ticks.                    |
| [OHLCV_1S](https://databento.com/docs/schemas-and-data-formats/ohlcv-1s)      | `Bar`                             | 1-second bars.                  |
| [OHLCV_1M](https://databento.com/docs/schemas-and-data-formats/ohlcv-1m)      | `Bar`                             | 1-minute bars.                  |
| [OHLCV_1H](https://databento.com/docs/schemas-and-data-formats/ohlcv-1h)      | `Bar`                             | 1-hour bars.                    |
| [OHLCV_1D](https://databento.com/docs/schemas-and-data-formats/ohlcv-1d)      | `Bar`                             | Daily bars.                     |
| [OHLCV_EOD](https://databento.com/docs/schemas-and-data-formats/ohlcv-eod)    | `Bar`                             | End-of-day bars.                |
| [DEFINITION](https://databento.com/docs/schemas-and-data-formats/definition)  | `Instrument` (various types)      | Instrument definitions.         |
| [IMBALANCE](https://databento.com/docs/schemas-and-data-formats/imbalance)    | `DatabentoImbalance`              | Auction imbalance data.         |
| [STATISTICS](https://databento.com/docs/schemas-and-data-formats/statistics)  | `DatabentoStatistics`             | Market statistics.              |
| [STATUS](https://databento.com/docs/schemas-and-data-formats/status)          | `InstrumentStatus`                | Market status updates.          |

### Schema considerations

- **TBBO and TCBBO**: Trade-sampled feeds that pair every trade with the BBO immediately *before* the trade's effect (TBBO per-venue, TCBBO consolidated across venues). Use when you need trades aligned with contemporaneous quotes without managing two streams.
- **MBP-1 and CMBP-1 (L1)**: Event-level updates; emit trades only on trade events. Choose for a complete top-of-book event tape. For quote+trade alignment, prefer TBBO/TCBBO; otherwise, use TRADES.
- **MBP-10 (L2)**: Top 10 levels with trades. Good for depth-aware strategies that don't need per-order detail; lighter than MBO with much of the structure you need including number of orders per level.
- **MBO (L3)**: Per-order events enable queue position modeling and exact book reconstruction. Highest volume/cost; start at node initialization to ensure proper replay context.
- **BBO_1S/BBO_1M and CBBO_1S/CBBO_1M**: Sampled top-of-book quotes at fixed intervals (1s/1m), no trades. Best for monitoring/spreads/low-cost signal generation; not suited for fine-grained microstructure.
- **TRADES**: Trades only. Pair with MBP-1 (`include_trades=True`) or use TBBO/TCBBO if you need quote context aligned with trades.
- **OHLCV_ (incl. OHLCV_EOD)**: Aggregated bars derived from trades. Prefer for higher-timeframe analytics/backtests; ensure bar timestamps represent close time (set `bars_timestamp_on_close=True`).
- **Imbalance / Statistics / Status**: Venue operational data; subscribe via `subscribe_data` with a `DataType` carrying `instrument_id` metadata.

:::tip
**Consolidated schemas** (CMBP_1, CBBO_1S, CBBO_1M, TCBBO) aggregate data across multiple venues,
providing a unified view of the market. These are particularly useful for cross-venue analysis and
when you need a comprehensive market picture.
:::

:::info
See also the Databento [Schemas and data formats](https://databento.com/docs/schemas-and-data-formats) guide.
:::

## Schema selection for live subscriptions

The following table shows how Nautilus subscription methods map to Databento schemas:

| Nautilus Subscription Method    | Default Schema | Available Databento Schemas                                                  | Nautilus Data Type |
|:--------------------------------|:---------------|:-----------------------------------------------------------------------------|:-------------------|
| `subscribe_quote_ticks()`       | `mbp-1`        | `mbp-1`, `bbo-1s`, `bbo-1m`, `cmbp-1`, `cbbo-1s`, `cbbo-1m`, `tbbo`, `tcbbo` | `QuoteTick`        |
| `subscribe_trade_ticks()`       | `trades`       | `trades`, `tbbo`, `tcbbo`, `mbp-1`, `cmbp-1`                                 | `TradeTick`        |
| `subscribe_order_book_depth()`  | `mbp-10`       | `mbp-10`                                                                     | `OrderBookDepth10` |
| `subscribe_order_book_deltas()` | `mbo`          | `mbo`                                                                        | `OrderBookDeltas`  |
| `subscribe_bars()`              | varies         | `ohlcv-1s`, `ohlcv-1m`, `ohlcv-1h`, `ohlcv-1d`                               | `Bar`              |

:::note
The examples below assume you're within a `Strategy` or `Actor` class context where `self` has access to subscription methods.
Remember to import the necessary types:

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

For specialized Databento data types like imbalance and statistics, use the generic `subscribe_data` method:

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

Databento market data includes an `instrument_id` field which is an integer assigned
by either the original source venue, or internally by Databento during normalization.

It's important to realize that this is different to the Nautilus `InstrumentId`
which is a string made up of a symbol + venue with a period separator i.e. `"{symbol}.{venue}"`.

The Nautilus decoder will use the Databento `raw_symbol` for the Nautilus `symbol` and an [ISO 10383 MIC](https://www.iso20022.org/market-identifier-codes) (Market Identifier Code)
from the Databento instrument definition message for the Nautilus `venue`.

Databento datasets are identified with a *dataset ID* which is not the same
as a venue identifier. You can read more about Databento dataset naming conventions [here](https://databento.com/docs/api-reference-historical/basics/datasets).

Of particular note is for CME Globex MDP 3.0 data (`GLBX.MDP3` dataset ID), the following
exchanges are all grouped under the `GLBX` venue. These mappings can be determined from the
instruments `exchange` field:

- `CBCM`: XCME-XCBT inter-exchange spread
- `NYUM`: XNYM-DUMX inter-exchange spread
- `XCBT`: Chicago Board of Trade (CBOT)
- `XCEC`: Commodities Exchange Center (COMEX)
- `XCME`: Chicago Mercantile Exchange (CME)
- `XFXS`: CME FX Link spread
- `XNYM`: New York Mercantile Exchange (NYMEX)

:::info
Other venue MICs can be found in the `venue` field of responses from the [metadata.list_publishers](https://databento.com/docs/api-reference-historical/metadata/metadata-list-publishers) endpoint.
:::

## Timestamps

Databento data includes various timestamp fields including (but not limited to):

- `ts_event`: The matching-engine-received timestamp expressed as the number of nanoseconds since the UNIX epoch.
- `ts_in_delta`: The matching-engine-sending timestamp expressed as the number of nanoseconds before `ts_recv`.
- `ts_recv`: The capture-server-received timestamp expressed as the number of nanoseconds since the UNIX epoch.
- `ts_out`: The Databento sending timestamp.

Nautilus data includes at *least* two timestamps (required by the `Data` contract):

- `ts_event`: UNIX timestamp (nanoseconds) when the data event occurred.
- `ts_init`: UNIX timestamp (nanoseconds) when the data instance was created.

When decoding and normalizing Databento to Nautilus we generally assign the Databento `ts_recv` value to the Nautilus
`ts_event` field, as this timestamp is much more reliable and consistent, and is guaranteed to be monotonically increasing per instrument.
The exception to this are the `DatabentoImbalance` and `DatabentoStatistics` data types, which have fields for all timestamps as these types are defined specifically for the adapter.

:::info
See the following Databento docs for further information:

- [Databento standards and conventions - timestamps](https://databento.com/docs/standards-and-conventions/common-fields-enums-types#timestamps)
- [Databento timestamping guide](https://databento.com/docs/architecture/timestamping-guide)

:::

## Data types

The following section discusses Databento schema -> Nautilus data type equivalence
and considerations.

:::info
See Databento [schemas and data formats](https://databento.com/docs/schemas-and-data-formats).
:::

### Instrument definitions

Databento provides a single schema to cover all instrument classes, these are
decoded to the appropriate Nautilus `Instrument` types.

The following Databento instrument classes are supported by NautilusTrader:

| Databento instrument class | Code |  Nautilus instrument type    |
|----------------------------|------|------------------------------|
| Stock                      | `K`  | `Equity`                     |
| Future                     | `F`  | `FuturesContract`            |
| Call                       | `C`  | `OptionContract`             |
| Put                        | `P`  | `OptionContract`             |
| Future spread              | `S`  | `FuturesSpread`              |
| Option spread              | `T`  | `OptionSpread`               |
| Mixed spread               | `M`  | `OptionSpread`               |
| FX spot                    | `X`  | `CurrencyPair`               |
| Bond                       | `B`  | Not yet available            |

### MBO (market by order)

This schema is the highest granularity data offered by Databento, and represents
full order book depth. Some messages also provide trade information, and so when
decoding MBO messages Nautilus will produce an `OrderBookDelta` and optionally a
`TradeTick`.

The Nautilus live data client will buffer MBO messages until an `F_LAST` flag
is seen. A discrete `OrderBookDeltas` container object will then be passed to the
registered handler.

Order book snapshots are also buffered into a discrete `OrderBookDeltas` container
object, which occurs during the replay startup sequence.

### MBP-1 (market by price, top-of-book)

This schema represents the top-of-book only (quotes *and* trades). Like with MBO messages, some
messages carry trade information, and so when decoding MBP-1 messages Nautilus
will produce a `QuoteTick` and *also* a `TradeTick` if the message is a trade.

### TBBO and TCBBO (top-of-book with trades)

The TBBO (Top Book with Trades) and TCBBO (Top Consolidated Book with Trades) schemas provide
both quote and trade data in each message. When subscribing to quotes using these schemas,
you'll automatically receive both `QuoteTick` and `TradeTick` data, making them more efficient
than subscribing to quotes and trades separately. TCBBO provides consolidated data across venues.

### OHLCV (bar aggregates)

The Databento bar aggregation messages are timestamped at the **open** of the bar interval.
The Nautilus decoder will normalize the `ts_event` timestamps to the **close** of the bar
(original `ts_event` + bar interval).

### Imbalance & Statistics

The Databento `imbalance` and `statistics` schemas cannot be represented as a built-in Nautilus data types,
and so they have specific types defined in Rust `DatabentoImbalance` and `DatabentoStatistics`.
Python bindings are provided via PyO3 (Rust) so the types behave a little differently to built-in Nautilus
data types, where all attributes are PyO3 provided objects and not directly compatible
with certain methods which may expect a Cython provided type. There are PyO3 -> legacy Cython
object conversion methods available, which can be found in the API reference.

Here is a general pattern for converting a PyO3 `Price` to a Cython `Price`:

```python
price = Price.from_raw(pyo3_price.raw, pyo3_price.precision)
```

Additionally requesting for and subscribing to these data types requires the use of the
lower level generic methods for custom data types. The following example subscribes to the `imbalance`
schema for the `AAPL.XNAS` instrument (Apple Inc trading on the Nasdaq exchange):

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

Or requesting the previous days `statistics` schema for the `ES.FUT` parent symbol (all active E-mini S&P 500 futures contracts on the CME Globex exchange):

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

## Performance considerations

When backtesting with Databento DBN data, there are two options:

- Store the data in DBN (`.dbn.zst`) format files and decode to Nautilus objects on every run.
- Convert the DBN files to Nautilus objects and then write to the data catalog once (stored as Nautilus Parquet format on disk).

Whilst the DBN -> Nautilus decoder is implemented in Rust and has been optimized,
the best performance for backtesting will be achieved by writing the Nautilus
objects to the data catalog, which performs the decoding step once.

[DataFusion](https://arrow.apache.org/datafusion/) provides a query engine backend to efficiently load and stream
the Nautilus Parquet data from disk, which achieves extremely high throughput (at least an order of magnitude faster
than converting DBN -> Nautilus on the fly for every backtest run).

:::note
Performance benchmarks are currently under development.
:::

## Loading DBN data

You can load DBN files and convert the records to Nautilus objects using the
`DatabentoDataLoader` class. There are two main purposes for doing so:

- Pass the converted data to `BacktestEngine.add_data` directly for backtesting.
- Pass the converted data to `ParquetDataCatalog.write_data` for later streaming use with a `BacktestNode`.

### DBN data to a BacktestEngine

This code snippet demonstrates how to load DBN data and pass to a `BacktestEngine`.
Since the `BacktestEngine` needs an instrument added, we'll use a test instrument
provided by the `TestInstrumentProvider` (you could also pass an instrument object
which was parsed from a DBN file too).
The data is a month of TSLA (Tesla Inc) trades on the Nasdaq exchange:

```python
# Add instrument
TSLA_NASDAQ = TestInstrumentProvider.equity(symbol="TSLA")
engine.add_instrument(TSLA_NASDAQ)

# Decode data to legacy Cython objects
loader = DatabentoDataLoader()
trades = loader.from_dbn_file(
    path=TEST_DATA_DIR / "databento" / "temp" / "tsla-xnas-20240107-20240206.trades.dbn.zst",
    instrument_id=TSLA_NASDAQ.id,
)

# Add data
engine.add_data(trades)
```

### DBN data to a ParquetDataCatalog

This code snippet demonstrates how to load DBN data and write to a `ParquetDataCatalog`.
We pass a value of false for the `as_legacy_cython` flag, which will ensure the
DBN records are decoded as PyO3 (Rust) objects. It's worth noting that legacy Cython
objects can also be passed to `write_data`, but these need to be converted back to
pyo3 objects under the hood (so passing PyO3 objects is an optimization).

### Loading instruments

**Important**: When loading market data (MBO, trades, quotes, bars, etc.) into a catalog, you must first load the corresponding instrument definitions from DEFINITION schema files.
The catalog needs instruments to be present before it can store market data. Market data files (MBO, TRADES, etc.) do not contain instrument definitions.

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

When preparing a catalog for backtesting with multiple data types (e.g., MBO order book data), always load instruments first:

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
You can verify your instruments loaded correctly by calling `catalog.instruments()` which returns a list of all instruments in the catalog. If this returns an empty list, you need to load DEFINITION files first.
:::

:::info
To obtain DEFINITION schema files from Databento, use the Databento API or CLI to download instrument definitions for your symbols and date ranges.
See the [Databento documentation](https://databento.com/docs/api-reference-historical/timeseries/timeseries-get-range) for details on requesting definition data.
:::

:::info
See also the [Data concepts guide](../concepts/data.md).
:::

### Historical loader options

The `from_dbn_file` method supports several important parameters:

- `instrument_id`: Passing this improves decode speed by skipping symbology lookup.
- `price_precision`: Override the default price precision for the instrument.
- `include_trades`: For MBP-1/CMBP-1 schemas, setting this to `True` will emit both `QuoteTick` and `TradeTick` objects when trade data is present.
- `as_legacy_cython`: Set to `False` when loading IMBALANCE or STATISTICS schemas (required) or for performance when writing to catalog.

:::warning
IMBALANCE and STATISTICS schemas require `as_legacy_cython=False` as these are PyO3-only types. Setting `as_legacy_cython=True` will raise a `ValueError`.
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
**Cost optimization**: Avoid subscribing to both TBBO/TCBBO and separate trade subscriptions for the same instrument, as these schemas already include trade data. This prevents duplicates and reduces costs.
:::

## Real-time client architecture

The `DatabentoDataClient` is a Python class which contains other Databento adapter classes.
There are two `DatabentoLiveClient`s per Databento dataset:

- One for MBO (order book deltas) real-time feeds
- One for all other real-time feeds

:::warning
There is currently a limitation that all MBO (order book deltas) subscriptions for a dataset have to be made at
node startup, to then be able to replay data from the beginning of the session. If subsequent subscriptions
arrive after start, then an error will be logged (and the subscription ignored).

There is no such limitation for any of the other Databento schemas.
:::

A single `DatabentoHistoricalClient` instance is reused between the `DatabentoInstrumentProvider` and `DatabentoDataClient`,
which makes historical instrument definitions and data requests.

## Configuration

The most common use case is to configure a live `TradingNode` to include a
Databento data client. To achieve this, add a `DATABENTO` section to your client
configuration(s):

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

Then, create a `TradingNode` and add the client factory:

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

The Databento data client provides the following configuration options:

| Option                    | Default | Description |
|---------------------------|---------|-------------|
| `api_key`                 | `None`  | Databento API secret. When `None`, falls back to the `DATABENTO_API_KEY` environment variable. |
| `http_gateway`            | `None`  | Historical HTTP gateway override, useful for testing custom endpoints. |
| `live_gateway`            | `None`  | Raw TCP real-time gateway override, typically only used for testing. |
| `use_exchange_as_venue`   | `True`  | If `True`, uses the exchange MIC for Nautilus venues (e.g., `XCME`). When `False`, retains the default GLBX mapping. |
| `timeout_initial_load`    | `15.0`  | Seconds to wait for instrument definitions to load per dataset before proceeding. |
| `mbo_subscriptions_delay` | `3.0`   | Seconds to buffer before enabling MBO/L3 streams so initial snapshots can replay in order. |
| `bars_timestamp_on_close` | `True`  | Timestamp bars on the close (`ts_event`/`ts_init`). Set `False` to timestamp on the open. |
| `venue_dataset_map`       | `None`  | Optional mapping of Nautilus venues to Databento dataset codes. |
| `parent_symbols`          | `None`  | Optional mapping `{dataset: {parent symbols}}` to preload definition trees (e.g., `{"GLBX.MDP3": {"ES.FUT", "ES.OPT"}}`). |
| `instrument_ids`          | `None`  | Sequence of Nautilus `InstrumentId` values to preload definitions for at startup. |

:::tip
We recommend using environment variables to manage your credentials.
:::

:::info
For additional features or to contribute to the Databento adapter, please see our
[contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md).
:::
