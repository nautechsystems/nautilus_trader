# Data

The NautilusTrader platform provides a set of built-in data types specifically designed to represent a trading domain.
These data types include:

- `OrderBookDelta` (L1/L2/L3): Represents the most granular order book updates.
- `OrderBookDeltas` (L1/L2/L3): Batches multiple order book deltas for more efficient processing.
- `OrderBookDepth10`: Aggregated order book snapshot (up to 10 levels per bid and ask side).
- `QuoteTick`: Represents the best bid and ask prices along with their sizes at the top-of-book.
- `TradeTick`: A single trade/match event between counterparties.
- `Bar`: OHLCV (Open, High, Low, Close, Volume) bar/candle, aggregated using a specified *aggregation method*.
- `InstrumentStatus`: An instrument-level status event.
- `InstrumentClose`: The closing price of an instrument.

NautilusTrader is designed primarily to operate on granular order book data, providing the highest realism
for execution simulations in backtesting.
However, backtests can also be conducted on any of the supported market data types, depending on the desired simulation fidelity.

## Order books

A high-performance order book implemented in Rust is available to maintain order book state based on provided data.

`OrderBook` instances are maintained per instrument for both backtesting and live trading, with the following book types available:
- `L3_MBO`: **Market by order (MBO)** or L3 data, uses every order book event at every price level, keyed by order ID.
- `L2_MBP`: **Market by price (MBP)** or L2 data, aggregates order book events by price level.
- `L1_MBP`: **Market by price (MBP)** or L1 data, also known as best bid and offer (BBO), captures only top-level updates.

:::note
Top-of-book data, such as `QuoteTick`, `TradeTick` and `Bar`, can also be used for backtesting, with markets operating on `L1_MBP` book types.
:::

## Timestamps

Each of these data types defines two timestamp fields:

- `ts_event`: UNIX timestamp (nanoseconds) representing when the data event occurred.
- `ts_init`: UNIX timestamp (nanoseconds) marking when the object was initialized.

For backtesting, data is ordered by `ts_init` using a stable sort. For persisted data, the `ts_init` field indicates when the message was originally received.

The `ts_event` timestamp enhances analytics by enabling some latency analysis. Relative latency can be measured as the difference between `ts_init` and `ts_event`,
though it's important to remember that the clocks producing these timestamps are likely not synchronized.

## Instruments

The following instrument definitions are available:

- `Betting`: Represents an instrument in a betting market.
- `BinaryOption`: Represents a generic binary option instrument.
- `Cfd`: Represents a Contract for Difference (CFD) instrument.
- `Commodity`:  Represents a commodity instrument in a spot/cash market.
- `CryptoFuture`: Represents a deliverable futures contract instrument, with crypto assets as underlying and for settlement.
- `CryptoPerpetual`: Represents a crypto perpetual futures contract instrument (a.k.a. perpetual swap).
- `CurrencyPair`: Represents a generic currency pair instrument in a spot/cash market.
- `Equity`: Represents a generic equity instrument.
- `FuturesContract`: Represents a generic deliverable futures contract instrument.
- `FuturesSpread`: Represents a generic deliverable futures spread instrument.
- `Index`: Represents a generic index instrument.
- `OptionsContract`: Represents a generic options contract instrument.
- `OptionsSpread`: Represents a generic options spread instrument.
- `Synthetic`: Represents a synthetic instrument with prices derived from component instruments using a formula.

## Bars and aggregation

A *bar*—also known as a candle, candlestick, or kline—is a data structure that represents price and
volume information over a specific period, including the opening price, highest price, lowest price, closing price,
and traded volume (or ticks as a volume proxy). These values are generated using an *aggregation method*,
which groups data based on specific criteria to create the bar.

The implemented aggregation methods are:

| Name               | Description                                                                | Category     |
|:-------------------|:---------------------------------------------------------------------------|:-------------|
| `TICK`             | Aggregation of a number of ticks.                                          | Threshold    |
| `TICK_IMBALANCE`   | Aggregation of the buy/sell imbalance of ticks.                            | Threshold    |
| `TICK_RUNS`        | Aggregation of sequential buy/sell runs of ticks.                          | Information  |
| `VOLUME`           | Aggregation of traded volume.                                              | Threshold    |
| `VOLUME_IMBALANCE` | Aggregation of the buy/sell imbalance of traded volume.                    | Threshold    |
| `VOLUME_RUNS`      | Aggregation of sequential runs of buy/sell traded volume.                  | Information  |
| `VALUE`            | Aggregation of the notional value of trades (also known as "Dollar bars"). | Threshold    |
| `VALUE_IMBALANCE`  | Aggregation of the buy/sell imbalance of trading by notional value.        | Information  |
| `VALUE_RUNS`       | Aggregation of sequential buy/sell runs of trading by notional value.      | Threshold    |
| `MILLISECOND`      | Aggregation of time intervals with millisecond granularity.                | Time         |
| `SECOND`           | Aggregation of time intervals with second granularity.                     | Time         |
| `MINUTE`           | Aggregation of time intervals with minute granularity.                     | Time         |
| `HOUR`             | Aggregation of time intervals with hour granularity.                       | Time         |
| `DAY`              | Aggregation of time intervals with day granularity.                        | Time         |
| `WEEK`             | Aggregation of time intervals with week granularity.                       | Time         |
| `MONTH`            | Aggregation of time intervals with month granularity.                      | Time         |

### Bar types

NautilusTrader defines a unique *bar type* (`BarType`) based on the following components:

- **Instrument ID** (`InstrumentId`): Specifies the particular instrument for the bar.
- **Bar Specification** (`BarSpecification`):
  - `step`: Defines the interval or frequency of each bar.
  - `aggregation`: Specifies the method used for data aggregation (see the list above).
  - `price_type`: Indicates the price basis of the bar (e.g., bid, ask, mid, last).
- **Aggregation Source** (`AggregationSource`): Indicates whether the bar was aggregated internally (within Nautilus) or externally (by a trading venue or data provider).

Bar data can be aggregated either internally or externally:

- `INTERNAL`: The bar is aggregated inside the local Nautilus system boundary.
- `EXTERNAL`: The bar is aggregated outside the local Nautilus system boundary (typically by a trading venue or data provider).

Bar types can also be classified as either *standard* or *composite*:

- **Standard**: Generated from granular market data, such as quotes or trade ticks.
- **Composite**: Derived from a higher-granularity bar type through subsampling.

### Defining standard bars

You can define bar types from strings using the following convention:

`{instrument_id}-{step}-{aggregation}-{price_type}-{INTERNAL | EXTERNAL}`

For example, to define a `BarType` for AAPL trades (last price) on Nasdaq (XNAS) using a 5-minute interval, aggregated from trades locally by Nautilus:
```python
bar_type = BarType.from_str("AAPL.XNAS-5-MINUTE-LAST-INTERNAL")
```

### Defining composite bars

Composite bars are derived by aggregating higher-granularity bars into the desired bar type.
To define a composite bar, use a similar convention to standard bars:

`{instrument_id}-{step}-{aggregation}-{price_type}-INTERNAL@{step}-{aggregation}-{INTERNAL | EXTERNAL}`

**Notes**:
- The derived bar type must use an `INTERNAL` aggregation source (since this is how the bar is aggregated).
- The sampled bar type must have a higher granularity than the derived bar type.
- The sampled instrument ID is inferred to match that of the derived bar type.
- Composite bars can be aggregated *from* `INTERNAL` or `EXTERNAL` aggregation sources.

For example, to define a `BarType` for AAPL trades (last price) on Nasdaq (XNAS) using a 5-minute interval, aggregated locally by Nautilus, from 1-minute interval bars aggregated externally:
```python
bar_type = BarType.from_str("AAPL.XNAS-5-MINUTE-LAST-INTERNAL@1-MINUTE-EXTERNAL")
```

## Data flow

The platform ensures consistency by flowing data through the same pathways across all system [environment contexts](/concepts/architecture.md#environment-contexts)
(e.g., `backtest`, `sandbox`, `live`). Data is primarily transported via the `MessageBus` to the `DataEngine` and then distributed to subscribed or registered handlers.

For users who need more flexibility, the platform also supports the creation of custom data types. For details on how to implement user-defined data types, refer to the advanced [Custom data guide](advanced/custom_data.md).

## Loading data

NautilusTrader facilitates data loading and conversion for three main use cases:

- Providing data for a `BacktestEngine` to run backtests.
- Persisting the Nautilus-specific Parquet format for the data catalog via `ParquetDataCatalog.write_data(...)` to be later used with a `BacktestNode`.
- For research purposes (to ensure data is consistent between research and backtesting).

Regardless of the destination, the process remains the same: converting diverse external data formats into Nautilus data structures.

To achieve this, two main components are necessary:
- A type of DataLoader (normally specific per raw source/format) which can read the data and return a `pd.DataFrame` with the correct schema for the desired Nautilus object
- A type of DataWrangler (specific per data type) which takes this `pd.DataFrame` and returns a `list[Data]` of Nautilus objects

### Data loaders

Data loader components are typically specific for the raw source/format and per integration. For instance, Binance order book data is stored in its raw CSV file form with
an entirely different format to [Databento Binary Encoding (DBN)](https://databento.com/docs/knowledge-base/new-users/dbn-encoding/getting-started-with-dbn) files.

### Data wranglers

Data wranglers are implemented per specific Nautilus data type, and can be found in the `nautilus_trader.persistence.wranglers` module.
Currently there exists:

- `OrderBookDeltaDataWrangler`
- `QuoteTickDataWrangler`
- `TradeTickDataWrangler`
- `BarDataWrangler`

:::warning
At the risk of causing confusion, there are also a growing number of DataWrangler v2 components, which will take a `pd.DataFrame` typically
with a different fixed width Nautilus arrow v2 schema, and output pyo3 Nautilus objects which are only compatible with the new version
of the Nautilus core, currently in development.

**These pyo3 provided data objects are not compatible where the legacy Cython objects are currently used (adding directly to a `BacktestEngine` etc).**
:::

### Transformation pipeline

**Process flow**:

1. Raw data (e.g., CSV) is input into the pipeline.
2. DataLoader processes the raw data and converts it into a `pd.DataFrame`.
3. DataWrangler further processes the `pd.DataFrame` to generate a list of Nautilus objects.
4. The Nautilus `list[Data]` is the output of the data loading process.

The following diagram illustrates how raw data is transformed into Nautilus data structures:
```
  ┌──────────┐    ┌──────────────────────┐                  ┌──────────────────────┐
  │          │    │                      │                  │                      │
  │          │    │                      │                  │                      │
  │ Raw data │    │                      │  `pd.DataFrame`  │                      │
  │ (CSV)    ├───►│      DataLoader      ├─────────────────►│     DataWrangler     ├───► Nautilus `list[Data]`
  │          │    │                      │                  │                      │
  │          │    │                      │                  │                      │
  │          │    │                      │                  │                      │
  └──────────┘    └──────────────────────┘                  └──────────────────────┘

```

Conceretely, this would involve:

- `BinanceOrderBookDeltaDataLoader.load(...)` which reads CSV files provided by Binance from disk, and returns a `pd.DataFrame`.
- `OrderBookDeltaDataWrangler.process(...)` which takes the `pd.DataFrame` and returns `list[OrderBookDelta]`.

The following example shows how to accomplish the above in Python:
```python
from nautilus_trader import TEST_DATA_DIR
from nautilus_trader.persistence.loaders import BinanceOrderBookDeltaDataLoader
from nautilus_trader.persistence.wranglers import OrderBookDeltaDataWrangler
from nautilus_trader.test_kit.providers import TestInstrumentProvider


# Load raw data
data_path = TEST_DATA_DIR / "binance" / "btcusdt-depth-snap.csv"
df = BinanceOrderBookDeltaDataLoader.load(data_path)

# Set up a wrangler
instrument = TestInstrumentProvider.btcusdt_binance()
wrangler = OrderBookDeltaDataWrangler(instrument)

# Process to a list `OrderBookDelta` Nautilus objects
deltas = wrangler.process(df)
```

## Data catalog

The data catalog is a central store for Nautilus data, persisted in the [Parquet](https://parquet.apache.org) file format.

We have chosen Parquet as the storage format for the following reasons:

- It performs much better than CSV/JSON/HDF5/etc in terms of compression ratio (storage size) and read performance.
- It does not require any separate running components (for example a database).
- It is quick and simple to get up and running with.

The Arrow schemas used for the Parquet format are either single sourced in the core `persistence` Rust crate, or available
from the `/serialization/arrow/schema.py` module.

:::note
2023-10-14: The current plan is to eventually phase out the Python schemas module, so that all schemas are single sourced in the Rust core.
:::

### Initializing

The data catalog can be initialized from a `NAUTILUS_PATH` environment variable, or by explicitly passing in a path like object.

The following example shows how to initialize a data catalog where there is pre-existing data already written to disk at the given path.

```python
from pathlib import Path
from nautilus_trader.persistence.catalog import ParquetDataCatalog


CATALOG_PATH = Path.cwd() / "catalog"

# Create a new catalog instance
catalog = ParquetDataCatalog(CATALOG_PATH)
```

### Writing data

New data can be stored in the catalog, which is effectively writing the given data to disk in the Nautilus-specific Parquet format.
All Nautilus built-in `Data` objects are supported, and any data which inherits from `Data` can be written.

The following example shows the above list of Binance `OrderBookDelta` objects being written:

```python
catalog.write_data(deltas)
```

### Basename template

Nautilus makes no assumptions about how data may be partitioned between files for a particular
data type and instrument ID.

The `basename_template` keyword argument is an additional optional naming component for the output files. 
The template should include placeholders that will be filled in with actual values at runtime. 
These values can be automatically derived from the data or provided as additional keyword arguments.

For example, using a basename template like `"{date}"` for AUD/USD.SIM quote tick data, 
and assuming `"date"` is a provided or derivable field, could result in a filename like 
`"2023-01-01.parquet"` under the `"quote_tick/audusd.sim/"` catalog directory.
If not provided, a default naming scheme will be applied. This parameter should be specified as a
keyword argument, like `write_data(data, basename_template="{date}")`.

:::warning
Any data which already exists under a filename will be overwritten.
If a `basename_template` is not provided, then its very likely existing data for the data type and instrument ID will
be overwritten. To prevent data loss, ensure that the `basename_template` (or the default naming scheme)
generates unique filenames for different data sets.
:::

Rust Arrow schema implementations are available for the follow data types (enhanced performance):

- `OrderBookDelta`
- `QuoteTick`
- `TradeTick`
- `Bar`

### Reading data
Any stored data can then we read back into memory:
```python
from nautilus_trader.core.datetime import dt_to_unix_nanos
import pandas as pd


start = dt_to_unix_nanos(pd.Timestamp("2020-01-03", tz=pytz.utc))
end =  dt_to_unix_nanos(pd.Timestamp("2020-01-04", tz=pytz.utc))

deltas = catalog.order_book_deltas(instrument_ids=[instrument.id.value], start=start, end=end)
```

### Streaming data

When running backtests in streaming mode with a `BacktestNode`, the data catalog can be used to stream the data in batches.

The following example shows how to achieve this by initializing a `BacktestDataConfig` configuration object:

```python
from nautilus_trader.config import BacktestDataConfig
from nautilus_trader.model.data import OrderBookDelta


data_config = BacktestDataConfig(
    catalog_path=str(catalog.path),
    data_cls=OrderBookDelta,
    instrument_id=instrument.id,
    start_time=start,
    end_time=end,
)
```

This configuration object can then be passed into a `BacktestRunConfig` and then in turn passed into a `BacktestNode` as part of a run.
See the [Backtest (high-level API)](../getting_started/backtest_high_level.md) tutorial for further details.
