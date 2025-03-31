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
- `OptionContract`: Represents a generic option contract instrument.
- `OptionSpread`: Represents a generic option spread instrument.
- `Synthetic`: Represents a synthetic instrument with prices derived from component instruments using a formula.

## Bars and aggregation

### Introduction to Bars

A *bar* (also known as a candle, candlestick or kline) is a data structure that represents
price and volume information over a specific period, including:

- Opening price
- Highest price
- Lowest price
- Closing price
- Traded volume (or ticks as a volume proxy)

These bars are generated using an *aggregation method*, which groups data based on specific criteria.

### Purpose of data aggregation

Data aggregation in NautilusTrader transforms granular market data into structured bars or candles for several reasons:

- To provide data for technical indicators and strategy development.
- Because time-aggregated data (like minute bars) are often sufficient for many strategies.
- To reduce costs compared to high-frequency L1/L2/L3 market data.

### Aggregation methods

The platform implements various aggregation methods:

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

### Types of aggregation

NautilusTrader implements three distinct data aggregation methods:

1. **Trade-to-bar aggregation**: Creates bars from `TradeTick` objects (executed trades)
   - Use case: For strategies analyzing execution prices or when working directly with trade data.
   - Always uses the `LAST` price type in the bar specification.

2. **Quote-to-bar aggregation**: Creates bars from `QuoteTick` objects (bid/ask prices)
   - Use case: For strategies focusing on bid/ask spreads or market depth analysis.
   - Uses `BID`, `ASK`, or `MID` price types in the bar specification.

3. **Bar-to-bar aggregation**: Creates larger-timeframe `Bar` objects from smaller-timeframe `Bar` objects
   - Use case: For resampling existing smaller timeframe bars (1-minute) into larger timeframes (5-minute, hourly).
   - Always requires the `@` symbol in the specification.

### Bar types and Components

NautilusTrader defines a unique *bar type* (`BarType` class) based on the following components:

- **Instrument ID** (`InstrumentId`): Specifies the particular instrument for the bar.
- **Bar Specification** (`BarSpecification`):
  - `step`: Defines the interval or frequency of each bar.
  - `aggregation`: Specifies the method used for data aggregation (see the above table).
  - `price_type`: Indicates the price basis of the bar (e.g., bid, ask, mid, last).
- **Aggregation Source** (`AggregationSource`): Indicates whether the bar was aggregated internally (within Nautilus)
- or externally (by a trading venue or data provider).

Bar types can also be classified as either *standard* or *composite*:

- **Standard**: Generated from granular market data, such as quote-ticks or trade-ticks.
- **Composite**: Derived from a higher-granularity bar type through subsampling (like 5-MINUTE bars aggregate from 1-MINUTE bars).

### Aggregation sources

Bar data aggregation can be either *internal* or *external*:

- `INTERNAL`: The bar is aggregated inside the local Nautilus system boundary.
- `EXTERNAL`: The bar is aggregated outside the local Nautilus system boundary (typically by a trading venue or data provider).

For bar-to-bar aggregation, the target bar type is always `INTERNAL` (since you're doing the aggregation within NautilusTrader),
but the source bars can be either `INTERNAL` or `EXTERNAL`, i.e., you can aggregate externally provided bars or already
aggregated internal bars.

### Defining Bar Types with String Syntax

#### Standard bars

You can define standard bar types from strings using the following convention:

`{instrument_id}-{step}-{aggregation}-{price_type}-{INTERNAL | EXTERNAL}`

For example, to define a `BarType` for AAPL trades (last price) on Nasdaq (XNAS) using a 5-minute interval
aggregated from trades locally by Nautilus:

```python
bar_type = BarType.from_str("AAPL.XNAS-5-MINUTE-LAST-INTERNAL")
```

#### Composite bars

Composite bars are derived by aggregating higher-granularity bars into the desired bar type. To define a composite bar,
use this convention:

`{instrument_id}-{step}-{aggregation}-{price_type}-INTERNAL@{step}-{aggregation}-{INTERNAL | EXTERNAL}`

**Notes**:
- The derived bar type must use an `INTERNAL` aggregation source (since this is how the bar is aggregated).
- The sampled bar type must have a higher granularity than the derived bar type.
- The sampled instrument ID is inferred to match that of the derived bar type.
- Composite bars can be aggregated *from* `INTERNAL` or `EXTERNAL` aggregation sources.

For example, to define a `BarType` for AAPL trades (last price) on Nasdaq (XNAS) using a 5-minute interval
aggregated locally by Nautilus, from 1-minute interval bars aggregated externally:

```python
bar_type = BarType.from_str("AAPL.XNAS-5-MINUTE-LAST-INTERNAL@1-MINUTE-EXTERNAL")
```

### Aggregation syntax examples

The `BarType` string format encodes both the target bar type and, optionally, the source data type:

```
{instrument_id}-{step}-{aggregation}-{price_type}-{source}@{step}-{aggregation}-{source}
```

The part after the `@` symbol is optional and only used for bar-to-bar aggregation:

- **Without `@`**: Aggregates from `TradeTick` objects (when price_type is `LAST`) or `QuoteTick` objects (when price_type is `BID`, `ASK`, or `MID`).
- **With `@`**: Aggregates from existing `Bar` objects (specifying the source bar type).

#### Trade-to-bar example

```python
def on_start(self) -> None:
    # Define a bar type for aggregating from TradeTick objects
    # Uses price_type=LAST which indicates TradeTick data as source
    bar_type = BarType.from_str("6EH4.XCME-50-VOLUME-LAST-INTERNAL")

    # Request historical data (will receive bars in on_historical_data handler)
    self.request_bars(bar_type)

    # Subscribe to live data (will receive bars in on_bar handler)
    self.subscribe_bars(bar_type)
```

#### Quote-to-bar example

```python
def on_start(self) -> None:
    # Create 1-minute bars from ASK prices (in QuoteTick objects)
    bar_type_ask = BarType.from_str("6EH4.XCME-1-MINUTE-ASK-INTERNAL")

    # Create 1-minute bars from BID prices (in QuoteTick objects)
    bar_type_bid = BarType.from_str("6EH4.XCME-1-MINUTE-BID-INTERNAL")

    # Create 1-minute bars from MID prices (middle between ASK and BID prices in QuoteTick objects)
    bar_type_mid = BarType.from_str("6EH4.XCME-1-MINUTE-MID-INTERNAL")

    # Request historical data and subscribe to live data
    self.request_bars(bar_type_ask)    # Historical bars processed in on_historical_data
    self.subscribe_bars(bar_type_ask)  # Live bars processed in on_bar
```

#### Bar-to-bar example

```python
def on_start(self) -> None:
    # Create 5-minute bars from 1-minute bars (Bar objects)
    # Format: target_bar_type@source_bar_type
    # Note: price type (LAST) is only needed on the left target side, not on the source side
    bar_type = BarType.from_str("6EH4.XCME-5-MINUTE-LAST-INTERNAL@1-MINUTE-EXTERNAL")

    # Request historical data (processed in on_historical_data(...) handler)
    self.request_bars(bar_type)

    # Subscribe to live updates (processed in on_bar(...) handler)
    self.subscribe_bars(bar_type)
```

#### Advanced bar-to-bar example

You can create complex aggregation chains where you aggregate from already aggregated bars:

```python
# First create 1-minute bars from TradeTick objects (LAST indicates TradeTick source)
primary_bar_type = BarType.from_str("6EH4.XCME-1-MINUTE-LAST-INTERNAL")

# Then create 5-minute bars from 1-minute bars
# Note the @1-MINUTE-INTERNAL part identifying the source bars
intermediate_bar_type = BarType.from_str("6EH4.XCME-5-MINUTE-LAST-INTERNAL@1-MINUTE-INTERNAL")

# Then create hourly bars from 5-minute bars
# Note the @5-MINUTE-INTERNAL part identifying the source bars
hourly_bar_type = BarType.from_str("6EH4.XCME-1-HOUR-LAST-INTERNAL@5-MINUTE-INTERNAL")
```

### Working with Bars: Request vs. Subscribe

NautilusTrader provides two distinct operations for working with bars:

- **`request_bars()`**: Fetches historical data processed by the `on_historical_data()` handler.
- **`subscribe_bars()`**: Establishes a real-time data feed processed by the `on_bar()` handler.

These methods work together in a typical workflow:
1. First, `request_bars()` loads historical data to initialize indicators or state of strategy with past market behavior.
2. Then, `subscribe_bars()` ensures the strategy continues receiving new bars as they form in real-time.

Example usage in `on_start()`:

```python
def on_start(self) -> None:
    # Define bar type
    bar_type = BarType.from_str("6EH4.XCME-5-MINUTE-LAST-INTERNAL")

    # Request historical data to initialize indicators
    # These bars will be delivered to the on_historical_data(...) handler in strategy
    self.request_bars(bar_type)

    # Subscribe to real-time updates
    # New bars will be delivered to the on_bar(...) handler in strategy
    self.subscribe_bars(bar_type)

    # Register indicators to receive bar updates (they will be automatically updated)
    self.register_indicator_for_bars(bar_type, self.my_indicator)
```

Required handlers in your strategy to receive the data:

```python
def on_historical_data(self, data):
    # Processes batches of historical bars from request_bars()
    # Note: indicators registered with register_indicator_for_bars
    # are updated automatically with historical data
    pass

def on_bar(self, bar):
    # Processes individual bars in real-time from subscribe_bars()
    # Indicators registered with this bar type will update automatically and they will be updated before this handler is called
    pass
```

### Historical data requests with aggregation

When requesting historical bars for backtesting or initializing indicators, you can use the `request_bars()` method, which supports both direct requests and aggregation:

```python
# Request raw 1-minute bars (aggregated from TradeTick objects as indicated by LAST price type)
self.request_bars(BarType.from_str("6EH4.XCME-1-MINUTE-LAST-EXTERNAL"))

# Request 5-minute bars aggregated from 1-minute bars
self.request_bars(BarType.from_str("6EH4.XCME-5-MINUTE-LAST-INTERNAL@1-MINUTE-EXTERNAL"))
```

If historical aggregated bars are needed, you can use specialized request `request_aggregated_bars()` method:

```python
# Request bars that are aggregated from historical trade ticks
self.request_aggregated_bars(BarType.from_str("6EH4.XCME-100-VOLUME-LAST-INTERNAL"))

# Request bars that are aggregated from other bars
self.request_aggregated_bars(BarType.from_str("6EH4.XCME-5-MINUTE-LAST-INTERNAL@1-MINUTE-EXTERNAL"))
```

### Common Pitfalls

**Register indicators before requesting data**: Ensure indicators are registered before requesting historical data so they get updated properly.

```python
# Correct order
self.register_indicator_for_bars(bar_type, self.ema)
self.request_bars(bar_type)

# Incorrect order
self.request_bars(bar_type)  # Indicator won't receive historical data
self.register_indicator_for_bars(bar_type, self.ema)
```

## Timestamps

The platform uses two fundamental timestamp fields that appear across many objects, including market data, orders, and events.
These timestamps serve distinct purposes and help maintain precise timing information throughout the system:

- `ts_event`: UNIX timestamp (nanoseconds) representing when an event actually occurred.
- `ts_init`: UNIX timestamp (nanoseconds) representing when Nautilus created the internal object representing that event.

### Examples

| **Event Type**   | **`ts_event`**                                        | **`ts_init`** |
| -----------------| ------------------------------------------------------| --------------|
| `TradeTick`      | Time when trade occurred at the exchange.             | Time when Nautilus received the trade data. |
| `QuoteTick`      | Time when quote occurred at the exchange.             | Time when Nautilus received the quote data. |
| `OrderBookDelta` | Time when order book update occurred at the exchange. | Time when Nautilus received the order book update. |
| `Bar`            | Time of the bar's closing (exact minute/hour).        | Time when Nautilus generated (for internal bars) or received the bar data (for external bars). |
| `OrderFilled`    | Time when order was filled at the exchange.           | Time when Nautilus received and processed the fill confirmation. |
| `OrderCanceled`  | Time when cancellation was processed at the exchange. | Time when Nautilus received and processed the cancellation confirmation. |
| `NewsEvent`      | Time when the news was published.                     | Time when the event object was created (if internal event) or received (if external event) in Nautilus. |
| Custom event     | Time when event conditions actually occurred.         | Time when the event object was created (if internal event) or received (if external event) in Nautilus. |

:::note
The `ts_init` field represents a more general concept than just the "time of reception" for events.
It denotes the timestamp when an object, such as a data point or command, was initialized within Nautilus.
This distinction is important because `ts_init` is not exclusive to "received events" — it applies to any internal
initialization process.

For example, the `ts_init` field is also used for commands, where the concept of reception does not apply.
This broader definition ensures consistent handling of initialization timestamps across various object types in the system.
:::

### Latency analysis

The dual timestamp system enables latency analysis within the platform:

- Latency can be calculated as `ts_init - ts_event`.
- This difference represents total system latency, including network transmission time, processing overhead, and any queueing delays.
- It's important to remember that the clocks producing these timestamps are likely not synchronized.

### Environment specific behavior

#### Backtesting environment

- Data is ordered by `ts_init` using a stable sort.
- This behavior ensures deterministic processing order and simulates realistic system behavior, including latencies.

#### Live trading environment

- Data is processed as it arrives, ensuring minimal latency and allowing for real-time decision-making.
  - `ts_init` field records the exact moment when data is received by Nautilus in real-time.
  - `ts_event` reflects the time the event occurred externally, enabling accurate comparisons between external event timing and system reception.
- We can use the difference between `ts_init` and `ts_event` to detect network or processing delays.

### Other notes and considerations

- For data from external sources, `ts_init` is always the same as or later than `ts_event`.
- For data created within Nautilus, `ts_init` and `ts_event` can be the same because the object is initialized at the same time the event happens.
- Not every type with a `ts_init` field necessarily has a `ts_event` field. This reflects cases where:
  - The initialization of an object happens at the same time as the event itself.
  - The concept of an external event time does not apply.

#### Persisted Data

The `ts_init` field indicates when the message was originally received.

## Data flow

The platform ensures consistency by flowing data through the same pathways across all system [environment contexts](/concepts/architecture.md#environment-contexts)
(e.g., `backtest`, `sandbox`, `live`). Data is primarily transported via the `MessageBus` to the `DataEngine`
and then distributed to subscribed or registered handlers.

For users who need more flexibility, the platform also supports the creation of custom data types.
For details on how to implement user-defined data types, refer to the advanced [Custom data guide](advanced/custom_data.md).

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
with a different fixed width Nautilus Arrow v2 schema, and output PyO3 Nautilus objects which are only compatible with the new version
of the Nautilus core, currently in development.

**These PyO3 provided data objects are not compatible where the legacy Cython objects are currently used (e.g., adding directly to a `BacktestEngine`).**
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

Concretely, this would involve:

- `BinanceOrderBookDeltaDataLoader.load(...)` which reads CSV files provided by Binance from disk, and returns a `pd.DataFrame`.
- `OrderBookDeltaDataWrangler.process(...)` which takes the `pd.DataFrame` and returns `list[OrderBookDelta]`.

The following example shows how to accomplish the above in Python:
```python
from nautilus_trader import TEST_DATA_DIR
from nautilus_trader.adapters.binance.loaders import BinanceOrderBookDeltaDataLoader
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
Any stored data can then be read back into memory:
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
from nautilus_trader.model import OrderBookDelta


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

## Data migrations

NautilusTrader defines an internal data format specified in the `nautilus_model` crate.
These models are serialized into Arrow record batches and written to Parquet files.
Nautilus backtesting is most efficient when using these Nautilus-format Parquet files.

However, migrating the data model between [precision modes](../getting_started/installation.md#precision-mode) and schema changes can be challenging.
This guide explains how to handle data migrations using our utility tools.

### Migration tools

The `nautilus_persistence` crate provides two key utilities:

#### `to_json`

Converts Parquet files to JSON while preserving metadata:

- Creates two files:

  - `<input>.json`: Contains the deserialized data
  - `<input>.metadata.json`: Contains schema metadata and row group configuration

- Automatically detects data type from filename:

  - `OrderBookDelta` (contains "deltas" or "order_book_delta")
  - `QuoteTick` (contains "quotes" or "quote_tick")
  - `TradeTick` (contains "trades" or "trade_tick")
  - `Bar` (contains "bars")

#### `to_parquet`

Converts JSON back to Parquet format:

- Reads both the data JSON and metadata JSON files
- Preserves row group sizes from original metadata
- Uses ZSTD compression
- Creates `<input>.parquet`

### Migration Process

The following migration examples both use trades data (you can also migrate the other data types in the same way).
All commands should be run from the root of the `nautilus_core/persistence/` crate directory.

#### Migrating from standard-precision (64-bit) to high-precision (128-bit)

This example describes a scenario where you want to migrate from standard-precision schema to high-precision schema.

:::note
If you're migrating from a catalog that used the `Int64` and `UInt64` Arrow data types for prices and sizes,
be sure to check out commit [e284162](https://github.com/nautechsystems/nautilus_trader/commit/e284162cf27a3222115aeb5d10d599c8cf09cf50)
**before** compiling the code that writes the initial JSON.
:::

**1. Convert from standard-precision Parquet to JSON**:

```bash
cargo run --bin to_json trades.parquet
```

This will create `trades.json` and `trades.metadata.json` files.

**2. Convert from JSON to high-precision Parquet**:

Add the `--features high-precision` flag to write data as high-precision (128-bit) schema Parquet.

```bash
cargo run --features high-precision --bin to_parquet trades.json
```

This will create a `trades.parquet` file with high-precision schema data.

#### Migrating schema changes

This example describes a scenario where you want to migrate from one schema version to another.

**1. Convert from old schema Parquet to JSON**:

Add the `--features high-precision` flag if the source data uses a high-precision (128-bit) schema.

```bash
cargo run --bin to_json trades.parquet
```

This will create `trades.json` and `trades.metadata.json` files.

**2. Switch to new schema version**:
```bash
git checkout <new-version>
```

**3. Convert from JSON back to new schema Parquet**:
```bash
cargo run --features high-precision --bin to_parquet trades.json
```

This will create a `trades.parquet` file with the new schema.

### Best Practices

- Always test migrations with a small dataset first.
- Maintain backups of original files.
- Verify data integrity after migration.
- Perform migrations in a staging environment before applying them to production data.

## Custom Data
Due to the modular nature of the Nautilus design, it is possible to set up systems
with very flexible data streams, including custom user-defined data types. This
guide covers some possible use cases for this functionality.

It's possible to create custom data types within the Nautilus system. First you
will need to define your data by subclassing from `Data`.

:::info
As `Data` holds no state, it is not strictly necessary to call `super().__init__()`.
:::

```python
from nautilus_trader.core import Data


class MyDataPoint(Data):
    """
    This is an example of a user-defined data class, inheriting from the base class `Data`.

    The fields `label`, `x`, `y`, and `z` in this class are examples of arbitrary user data.
    """

    def __init__(
        self,
        label: str,
        x: int,
        y: int,
        z: int,
        ts_event: int,
        ts_init: int,
    ) -> None:
        self.label = label
        self.x = x
        self.y = y
        self.z = z
        self._ts_event = ts_event
        self._ts_init = ts_init

    @property
    def ts_event(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the data event occurred.

        Returns
        -------
        int

        """
        return self._ts_event

    @property
    def ts_init(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the object was initialized.

        Returns
        -------
        int

        """
        return self._ts_init

```

The `Data` abstract base class acts as a contract within the system and requires two properties
for all types of data: `ts_event` and `ts_init`. These represent the UNIX nanosecond timestamps
for when the event occurred and when the object was initialized, respectively.

The recommended approach to satisfy the contract is to assign `ts_event` and `ts_init`
to backing fields, and then implement the `@property` for each as shown above
(for completeness, the docstrings are copied from the `Data` base class).

:::info
These timestamps enable Nautilus to correctly order data streams for backtests
using monotonically increasing `ts_init` UNIX nanoseconds.
:::

We can now work with this data type for backtesting and live trading. For instance,
we could now create an adapter which is able to parse and create objects of this
type - and send them back to the `DataEngine` for consumption by subscribers.

You can publish a custom data type within your actor/strategy using the message bus
in the following way:

```python
self.publish_data(
    DataType(MyDataPoint, metadata={"some_optional_category": 1}),
    MyDataPoint(...),
)
```

The `metadata` dictionary optionally adds more granular information that is used in the
topic name to publish data with the message bus.

Extra metadata information can also be passed to a `BacktestDataConfig` configuration object in order to
enrich and describe custom data objects used in a backtesting context:

```python
from nautilus_trader.config import BacktestDataConfig

data_config = BacktestDataConfig(
    catalog_path=str(catalog.path),
    data_cls=MyDataPoint,
    metadata={"some_optional_category": 1},
)
```

You can subscribe to custom data types within your actor/strategy in the following way:

```python
self.subscribe_data(
    data_type=DataType(MyDataPoint,
    metadata={"some_optional_category": 1}),
    client_id=ClientId("MY_ADAPTER"),
)
```

The `client_id` provides an identifier to route the data subscription to a specific client.

This will result in your actor/strategy passing these received `MyDataPoint`
objects to your `on_data` method. You will need to check the type, as this
method acts as a flexible handler for all custom data.

```python
def on_data(self, data: Data) -> None:
    # First check the type of data
    if isinstance(data, MyDataPoint):
        # Do something with the data
```

### Publishing and receiving signal data

Here is an example of publishing and receiving signal data using the `MessageBus` from an actor or strategy.
A signal is an automatically generated custom data identified by a name containing only one value of a basic type
(str, float, int, bool or bytes).

```python
self.publish_signal("signal_name", value, ts_event)
self.subscribe_signal("signal_name")

def on_signal(self, signal):
    print("Signal", data)
```

### Option Greeks example

This example demonstrates how to create a custom data type for option Greeks, specifically the delta.
By following these steps, you can create custom data types, subscribe to them, publish them, and store
them in the `Cache` or `ParquetDataCatalog` for efficient retrieval.

```python
import msgspec
from nautilus_trader.core import Data
from nautilus_trader.core.datetime import unix_nanos_to_iso8601
from nautilus_trader.model import DataType
from nautilus_trader.serialization.base import register_serializable_type
from nautilus_trader.serialization.arrow.serializer import register_arrow
import pyarrow as pa

from nautilus_trader.model import InstrumentId
from nautilus_trader.core.datetime import dt_to_unix_nanos, unix_nanos_to_dt, format_iso8601


class GreeksData(Data):
    def __init__(
        self, instrument_id: InstrumentId = InstrumentId.from_str("ES.GLBX"),
        ts_event: int = 0,
        ts_init: int = 0,
        delta: float = 0.0,
    ) -> None:
        self.instrument_id = instrument_id
        self._ts_event = ts_event
        self._ts_init = ts_init
        self.delta = delta

    def __repr__(self):
        return (f"GreeksData(ts_init={unix_nanos_to_iso8601(self._ts_init)}, instrument_id={self.instrument_id}, delta={self.delta:.2f})")

    @property
    def ts_event(self):
        return self._ts_event

    @property
    def ts_init(self):
        return self._ts_init

    def to_dict(self):
        return {
            "instrument_id": self.instrument_id.value,
            "ts_event": self._ts_event,
            "ts_init": self._ts_init,
            "delta": self.delta,
        }

    @classmethod
    def from_dict(cls, data: dict):
        return GreeksData(InstrumentId.from_str(data["instrument_id"]), data["ts_event"], data["ts_init"], data["delta"])

    def to_bytes(self):
        return msgspec.msgpack.encode(self.to_dict())

    @classmethod
    def from_bytes(cls, data: bytes):
        return cls.from_dict(msgspec.msgpack.decode(data))

    def to_catalog(self):
        return pa.RecordBatch.from_pylist([self.to_dict()], schema=GreeksData.schema())

    @classmethod
    def from_catalog(cls, table: pa.Table):
        return [GreeksData.from_dict(d) for d in table.to_pylist()]

    @classmethod
    def schema(cls):
        return pa.schema(
            {
                "instrument_id": pa.string(),
                "ts_event": pa.int64(),
                "ts_init": pa.int64(),
                "delta": pa.float64(),
            }
        )
```

#### Publishing and receiving data

Here is an example of publishing and receiving data using the `MessageBus` from an actor or strategy:

```python
register_serializable_type(GreeksData, GreeksData.to_dict, GreeksData.from_dict)

def publish_greeks(self, greeks_data: GreeksData):
    self.publish_data(DataType(GreeksData), greeks_data)

def subscribe_to_greeks(self):
    self.subscribe_data(DataType(GreeksData))

def on_data(self, data):
    if isinstance(GreeksData):
        print("Data", data)
```

#### Writing and reading data using the cache

Here is an example of writing and reading data using the `Cache` from an actor or strategy:

```python
def greeks_key(instrument_id: InstrumentId):
    return f"{instrument_id}_GREEKS"

def cache_greeks(self, greeks_data: GreeksData):
    self.cache.add(greeks_key(greeks_data.instrument_id), greeks_data.to_bytes())

def greeks_from_cache(self, instrument_id: InstrumentId):
    return GreeksData.from_bytes(self.cache.get(greeks_key(instrument_id)))
```

#### Writing and reading data using a catalog

For streaming custom data to feather files or writing it to parquet files in a catalog
(`register_arrow` needs to be used):

```python
register_arrow(GreeksData, GreeksData.schema(), GreeksData.to_catalog, GreeksData.from_catalog)

from nautilus_trader.persistence.catalog import ParquetDataCatalog
catalog = ParquetDataCatalog('.')

catalog.write_data([GreeksData()])
```

### Creating a custom data class automatically

The `@customdataclass` decorator enables the creation of a custom data class with default
implementations for all the features described above.

Each method can also be overridden if needed. Here is an example of its usage:

```python
from nautilus_trader.model.custom import customdataclass


@customdataclass
class GreeksTestData(Data):
    instrument_id: InstrumentId = InstrumentId.from_str("ES.GLBX")
    delta: float = 0.0


GreeksTestData(
    instrument_id=InstrumentId.from_str("CL.GLBX"),
    delta=1000.0,
    ts_event=1,
    ts_init=2,
)
```

#### Custom data type stub

To enhance development convenience and improve code suggestions in your IDE, you can create a `.pyi`
stub file with the proper constructor signature for your custom data types as well as type hints for attributes.
This is particularly useful when the constructor is dynamically generated at runtime, as it allows the IDE to recognize
and provide suggestions for the class's methods and attributes.

For instance, if you have a custom data class defined in `greeks.py`, you can create a corresponding `greeks.pyi` file
with the following constructor signature:

```python
from nautilus_trader.core import Data
from nautilus_trader.model import InstrumentId


class GreeksData(Data):
    instrument_id: InstrumentId
    delta: float

    def __init__(
        self,
        ts_event: int = 0,
        ts_init: int = 0,
        instrument_id: InstrumentId = InstrumentId.from_str("ES.GLBX"),
        delta: float = 0.0,
  ) -> GreeksData: ...
```
