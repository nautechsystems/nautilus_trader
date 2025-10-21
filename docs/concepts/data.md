# Data

NautilusTrader provides a set of built-in data types specifically designed to represent a trading domain.
These data types include:

- `OrderBookDelta` (L1/L2/L3): Represents the most granular order book updates.
- `OrderBookDeltas` (L1/L2/L3): Batches multiple order book deltas for more efficient processing.
- `OrderBookDepth10`: Aggregated order book snapshot (up to 10 levels per bid and ask side).
- `QuoteTick`: Represents the best bid and ask prices along with their sizes at the top-of-book.
- `TradeTick`: A single trade/match event between counterparties.
- `Bar`: OHLCV (Open, High, Low, Close, Volume) bar/candle, aggregated using a specified *aggregation method*.
- `MarkPriceUpdate`: The current mark price for an instrument (typically used in derivatives trading).
- `IndexPriceUpdate`: The index price for an instrument (underlying price used for mark price calculations).
- `FundingRateUpdate`: The funding rate for perpetual contracts (periodic payments between long and short positions).
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

### Introduction to bars

A *bar* (also known as a candle, candlestick or kline) is a data structure that represents
price and volume information over a specific period, including:

- Opening price
- Highest price
- Lowest price
- Closing price
- Traded volume (or ticks as a volume proxy)

The system generates bars using an *aggregation method* that groups data by specific criteria.

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
| `RENKO`            | Aggregation based on fixed price movements (brick size in ticks).          | Threshold    |
| `MILLISECOND`      | Aggregation of time intervals with millisecond granularity.                | Time         |
| `SECOND`           | Aggregation of time intervals with second granularity.                     | Time         |
| `MINUTE`           | Aggregation of time intervals with minute granularity.                     | Time         |
| `HOUR`             | Aggregation of time intervals with hour granularity.                       | Time         |
| `DAY`              | Aggregation of time intervals with day granularity.                        | Time         |
| `WEEK`             | Aggregation of time intervals with week granularity.                       | Time         |
| `MONTH`            | Aggregation of time intervals with month granularity.                      | Time         |
| `YEAR`             | Aggregation of time intervals with year granularity.                       | Time         |

:::note
The following bar aggregations are not currently implemented:

- `VOLUME_IMBALANCE`
- `VOLUME_RUNS`
- `VALUE_IMBALANCE`
- `VALUE_RUNS`

:::

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

### Bar types

NautilusTrader defines a unique *bar type* (`BarType` class) based on the following components:

- **Instrument ID** (`InstrumentId`): Specifies the particular instrument for the bar.
- **Bar Specification** (`BarSpecification`):
  - `step`: Defines the interval or frequency of each bar.
  - `aggregation`: Specifies the method used for data aggregation (see the above table).
  - `price_type`: Indicates the price basis of the bar (e.g., bid, ask, mid, last).
- **Aggregation Source** (`AggregationSource`): Indicates whether the bar was aggregated internally (within Nautilus).
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

### Defining bar types with *string syntax*

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

### Working with bars: request vs. subscribe

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
self.request_aggregated_bars([BarType.from_str("6EH4.XCME-100-VOLUME-LAST-INTERNAL")])

# Request bars that are aggregated from other bars
self.request_aggregated_bars([BarType.from_str("6EH4.XCME-5-MINUTE-LAST-INTERNAL@1-MINUTE-EXTERNAL")])
```

### Common pitfalls

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

### Environment-specific behavior

#### Backtesting environment

- Data is ordered by `ts_init` using a stable sort.
- This behavior ensures deterministic processing order and simulates realistic system behavior, including latencies.

#### Live trading environment

- The system processes data as it arrives to minimize latency and enable real-time decisions.
  - `ts_init` field records the exact moment when data is received by Nautilus in real-time.
  - `ts_event` reflects the time the event occurred externally, enabling accurate comparisons between external event timing and system reception.
- We can use the difference between `ts_init` and `ts_event` to detect network or processing delays.

### Other notes and considerations

- For data from external sources, `ts_init` is always the same as or later than `ts_event`.
- For data created within Nautilus, `ts_init` and `ts_event` can be the same because the object is initialized at the same time the event happens.
- Not every type with a `ts_init` field necessarily has a `ts_event` field. This reflects cases where:
  - The initialization of an object happens at the same time as the event itself.
  - The concept of an external event time does not apply.

#### Persisted data

The `ts_init` field indicates when the message was originally received.

## Data flow

The platform ensures consistency by flowing data through the same pathways across all system [environment contexts](/concepts/architecture.md#environment-contexts)
(e.g., `backtest`, `sandbox`, `live`). Data is primarily transported via the `MessageBus` to the `DataEngine`
and then distributed to subscribed or registered handlers.

For users who need more flexibility, the platform also supports the creation of custom data types.
For details on how to implement user-defined data types, see the [Custom Data](#custom-data) section below.

## Loading data

NautilusTrader facilitates data loading and conversion for three main use cases:

- Providing data for a `BacktestEngine` to run backtests.
- Persisting the Nautilus-specific Parquet format for the data catalog via `ParquetDataCatalog.write_data(...)` to be later used with a `BacktestNode`.
- For research purposes (to ensure data is consistent between research and backtesting).

Regardless of the destination, the process remains the same: converting diverse external data formats into Nautilus data structures.

To achieve this, two main components are necessary:

- A type of DataLoader (normally specific per raw source/format) which can read the data and return a `pd.DataFrame` with the correct schema for the desired Nautilus object.
- A type of DataWrangler (specific per data type) which takes this `pd.DataFrame` and returns a `list[Data]` of Nautilus objects.

### Data loaders

Data loader components are typically specific for the raw source/format and per integration. For instance, Binance order book data is stored in its raw CSV file form with
an entirely different format to [Databento Binary Encoding (DBN)](https://databento.com/docs/knowledge-base/new-users/dbn-encoding/getting-started-with-dbn) files.

### Data wranglers

Data wranglers are implemented per specific Nautilus data type, and can be found in the `nautilus_trader.persistence.wranglers` module.
Currently there exists:

- `OrderBookDeltaDataWrangler`
- `OrderBookDepth10DataWrangler`
- `QuoteTickDataWrangler`
- `TradeTickDataWrangler`
- `BarDataWrangler`

:::warning
There are a number of **DataWrangler v2** components, which will take a `pd.DataFrame` typically
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

The data catalog is a central store for Nautilus data, persisted in the [Parquet](https://parquet.apache.org) file format. It serves as the primary data management system for both backtesting and live trading scenarios, providing efficient storage, retrieval, and streaming capabilities for market data.

### Overview and architecture

The NautilusTrader data catalog is built on a dual-backend architecture that combines the performance of Rust with the flexibility of Python:

**Core components:**

- **ParquetDataCatalog**: The main Python interface for data operations.
- **Rust backend**: High-performance query engine for core data types (OrderBookDelta, QuoteTick, TradeTick, Bar, MarkPriceUpdate).
- **PyArrow backend**: Flexible fallback for custom data types and advanced filtering.
- **fsspec integration**: Support for local and cloud storage (S3, GCS, Azure, etc.).

**Key benefits**:

- **Performance**: Rust backend provides optimized query performance for core market data types.
- **Flexibility**: PyArrow backend handles custom data types and complex filtering scenarios.
- **Scalability**: Efficient compression and columnar storage reduce storage costs and improve I/O performance.
- **Cloud native**: Built-in support for cloud storage providers through fsspec.
- **No dependencies**: Self-contained solution requiring no external databases or services.

**Storage format advantages:**

- Superior compression ratio and read performance compared to CSV/JSON/HDF5.
- Columnar storage enables efficient filtering and aggregation.
- Schema evolution support for data model changes.
- Cross-language compatibility (Python, Rust, Java, C++, etc.).

The Arrow schemas used for the Parquet format are primarily single-sourced in the core `persistence` Rust crate, with some legacy schemas available from the `/serialization/arrow/schema.py` module.

:::note
The current plan is to eventually phase out the Python schemas module, so that all schemas are single sourced in the Rust core for consistency and performance.
:::

### Initializing

The data catalog can be initialized from a `NAUTILUS_PATH` environment variable, or by explicitly passing in a path like object.

:::note NAUTILUS_PATH environment variable
The `NAUTILUS_PATH` environment variable should point to the **root** directory containing your Nautilus data. The catalog will automatically append `/catalog` to this path.

For example:

- If `NAUTILUS_PATH=/home/user/trading_data`.
- Then the catalog will be located at `/home/user/trading_data/catalog`.

This is a common pattern when using `ParquetDataCatalog.from_env()` - make sure your `NAUTILUS_PATH` points to the parent directory, not the catalog directory itself.
:::

The following example shows how to initialize a data catalog where there is pre-existing data already written to disk at the given path.

```python
from pathlib import Path
from nautilus_trader.persistence.catalog import ParquetDataCatalog


CATALOG_PATH = Path.cwd() / "catalog"

# Create a new catalog instance
catalog = ParquetDataCatalog(CATALOG_PATH)

# Alternative: Environment-based initialization
catalog = ParquetDataCatalog.from_env()  # Uses NAUTILUS_PATH environment variable
```

### Filesystem protocols and storage options

The catalog supports multiple filesystem protocols through fsspec integration, enabling seamless operation across local and cloud storage systems.

#### Supported filesystem protocols

**Local filesystem (`file`):**

```python
catalog = ParquetDataCatalog(
    path="/path/to/catalog",
    fs_protocol="file",  # Default protocol
)
```

**Amazon S3 (`s3`):**

```python
catalog = ParquetDataCatalog(
    path="s3://my-bucket/nautilus-data/",
    fs_protocol="s3",
    fs_storage_options={
        "key": "your-access-key-id",
        "secret": "your-secret-access-key",
        "endpoint_url": "https://s3.amazonaws.com",  # Optional custom endpoint
    }
)
```

**Google Cloud Storage (`gcs`):**

```python
catalog = ParquetDataCatalog(
    path="gcs://my-bucket/nautilus-data/",
    fs_protocol="gcs",
    fs_storage_options={
        "project": "my-project-id",
        "token": "/path/to/service-account.json",  # Or "cloud" for default credentials
    }
)
```

**Azure Blob Storage :**

`abfs` protocol

```python
catalog = ParquetDataCatalog(
    path="abfs://container@account.dfs.core.windows.net/nautilus-data/",
    fs_protocol="abfs",
    fs_storage_options={
        "account_name": "your-storage-account",
        "account_key": "your-account-key",
        # Or use SAS token: "sas_token": "your-sas-token"
    }
)
```

`az` protocol

```python
catalog = ParquetDataCatalog(
    path="az://container/nautilus-data/",
    fs_protocol="az",
    fs_storage_options={
        "account_name": "your-storage-account",
        "account_key": "your-account-key",
        # Or use SAS token: "sas_token": "your-sas-token"
    }
)
```

#### URI-based initialization

For convenience, you can use URI strings that automatically parse protocol and storage options:

```python
# Local filesystem
catalog = ParquetDataCatalog.from_uri("/path/to/catalog")

# S3 bucket
catalog = ParquetDataCatalog.from_uri("s3://my-bucket/nautilus-data/")

# With storage options
catalog = ParquetDataCatalog.from_uri(
    "s3://my-bucket/nautilus-data/",
    storage_options={
        "access_key_id": "your-key",
        "secret_access_key": "your-secret"
    }
)
```

### Writing data

Store data in the catalog using the `write_data()` method. All Nautilus built-in `Data` objects are supported, and any data which inherits from `Data` can be written.

```python
# Write a list of data objects
catalog.write_data(quote_ticks)

# Write with custom timestamp range
catalog.write_data(
    trade_ticks,
    start=1704067200000000000,  # Optional start timestamp override (UNIX nanoseconds)
    end=1704153600000000000,    # Optional end timestamp override (UNIX nanoseconds)
)

# Skip disjoint check for overlapping data
catalog.write_data(bars, skip_disjoint_check=True)
```

### File naming and data organization

The catalog automatically generates filenames based on the timestamp range of the data being written. Files are named using the pattern `{start_timestamp}_{end_timestamp}.parquet` where timestamps are in ISO format.

Data is organized in directories by data type and instrument ID:

```
catalog/
├── data/
│   ├── quote_ticks/
│   │   └── eurusd.sim/
│   │       └── 20240101T000000000000000_20240101T235959999999999.parquet
│   └── trade_ticks/
│       └── btcusd.binance/
│           └── 20240101T000000000000000_20240101T235959999999999.parquet
```

**Rust backend data types (enhanced performance):**

The following data types use optimized Rust implementations:

- `OrderBookDelta`.
- `OrderBookDeltas`.
- `OrderBookDepth10`.
- `QuoteTick`.
- `TradeTick`.
- `Bar`.
- `MarkPriceUpdate`.

:::warning
By default, data that overlaps with existing files will cause an assertion error to maintain data integrity. Use `skip_disjoint_check=True` in `write_data()` to bypass this check when needed.
:::

### Reading data

Use the `query()` method to read data back from the catalog:

```python
from nautilus_trader.model import QuoteTick, TradeTick

# Query quote ticks for a specific instrument and time range
quotes = catalog.query(
    data_cls=QuoteTick,
    identifiers=["EUR/USD.SIM"],
    start="2024-01-01T00:00:00Z",
    end="2024-01-02T00:00:00Z"
)

# Query trade ticks with filtering
trades = catalog.query(
    data_cls=TradeTick,
    identifiers=["BTC/USD.BINANCE"],
    start="2024-01-01",
    end="2024-01-02",
    where="price > 50000"
)
```

### `BacktestDataConfig` - data specification for backtests

The `BacktestDataConfig` class is the primary mechanism for specifying data requirements before a backtest starts. It defines what data should be loaded from the catalog and how it should be filtered and processed during the backtest execution.

#### Core parameters

**Required parameters:**

- `catalog_path`: Path to the data catalog directory.
- `data_cls`: The data type class (e.g., QuoteTick, TradeTick, OrderBookDelta, Bar).

**Optional parameters:**

- `catalog_fs_protocol`: Filesystem protocol ('file', 's3', 'gcs', etc.).
- `catalog_fs_storage_options`: Storage-specific options (credentials, region, etc.).
- `instrument_id`: Specific instrument to load data for.
- `instrument_ids`: List of instruments (alternative to single instrument_id).
- `start_time`: Start time for data filtering (ISO string or UNIX nanoseconds).
- `end_time`: End time for data filtering (ISO string or UNIX nanoseconds).
- `filter_expr`: Additional PyArrow filter expressions.
- `client_id`: Client ID for custom data types.
- `metadata`: Additional metadata for data queries.
- `bar_spec`: Bar specification for bar data (e.g., "1-MINUTE-LAST").
- `bar_types`: List of bar types (alternative to bar_spec).

#### Basic usage examples

**Loading quote ticks:**

```python
from nautilus_trader.config import BacktestDataConfig
from nautilus_trader.model import QuoteTick, InstrumentId

data_config = BacktestDataConfig(
    catalog_path="/path/to/catalog",
    data_cls=QuoteTick,
    instrument_id=InstrumentId.from_str("EUR/USD.SIM"),
    start_time="2024-01-01T00:00:00Z",
    end_time="2024-01-02T00:00:00Z",
)
```

**Loading multiple instruments:**

```python
data_config = BacktestDataConfig(
    catalog_path="/path/to/catalog",
    data_cls=TradeTick,
    instrument_ids=["BTC/USD.BINANCE", "ETH/USD.BINANCE"],
    start_time="2024-01-01T00:00:00Z",
    end_time="2024-01-02T00:00:00Z",
)
```

**Loading Bar Data:**

```python
data_config = BacktestDataConfig(
    catalog_path="/path/to/catalog",
    data_cls=Bar,
    instrument_id=InstrumentId.from_str("AAPL.NASDAQ"),
    bar_spec="5-MINUTE-LAST",
    start_time="2024-01-01",
    end_time="2024-01-31",
)
```

#### Advanced configuration examples

**Cloud Storage with Custom Filtering:**

```python
data_config = BacktestDataConfig(
    catalog_path="s3://my-bucket/nautilus-data/",
    catalog_fs_protocol="s3",
    catalog_fs_storage_options={
        "key": "your-access-key",
        "secret": "your-secret-key",
        "region": "us-east-1"
    },
    data_cls=OrderBookDelta,
    instrument_id=InstrumentId.from_str("BTC/USD.COINBASE"),
    start_time="2024-01-01T09:30:00Z",
    end_time="2024-01-01T16:00:00Z",
    filter_expr="side == 'BUY'",  # Only buy-side deltas
)
```

**Custom Data with Client ID:**

```python
data_config = BacktestDataConfig(
    catalog_path="/path/to/catalog",
    data_cls="my_package.data.NewsEventData",
    client_id="NewsClient",
    metadata={"source": "reuters", "category": "earnings"},
    start_time="2024-01-01",
    end_time="2024-01-31",
)
```

#### Integration with BacktestRunConfig

The `BacktestDataConfig` objects are integrated into the backtesting framework through `BacktestRunConfig`:

```python
from nautilus_trader.config import BacktestRunConfig, BacktestVenueConfig

# Define multiple data configurations
data_configs = [
    BacktestDataConfig(
        catalog_path="/path/to/catalog",
        data_cls=QuoteTick,
        instrument_id="EUR/USD.SIM",
        start_time="2024-01-01",
        end_time="2024-01-02",
    ),
    BacktestDataConfig(
        catalog_path="/path/to/catalog",
        data_cls=TradeTick,
        instrument_id="EUR/USD.SIM",
        start_time="2024-01-01",
        end_time="2024-01-02",
    ),
]

# Create backtest run configuration
run_config = BacktestRunConfig(
    venues=[BacktestVenueConfig(name="SIM", oms_type="HEDGING")],
    data=data_configs,  # List of data configurations
    start="2024-01-01T00:00:00Z",
    end="2024-01-02T00:00:00Z",
)
```

#### Data loading process

When a backtest runs, the `BacktestNode` processes each `BacktestDataConfig`:

1. **Catalog Loading**: Creates a `ParquetDataCatalog` instance from the config.
2. **Query Construction**: Builds query parameters from config attributes.
3. **Data Retrieval**: Executes catalog queries using the appropriate backend.
4. **Instrument Loading**: Loads instrument definitions if needed.
5. **Engine Integration**: Adds data to the backtest engine with proper sorting.

The system automatically handles:

- Instrument ID resolution and validation.
- Data type validation and conversion.
- Memory-efficient streaming for large datasets.
- Error handling and logging.

### DataCatalogConfig - on-the-fly data loading

The `DataCatalogConfig` class provides configuration for on-the-fly data loading scenarios, particularly useful for backtests where the number of possible instruments is vast,
Unlike `BacktestDataConfig` which pre-specifies data for backtests, `DataCatalogConfig` enables flexible catalog access during runtime.
Catalogs defined this way can also be used for requesting historical data.

#### Core parameters

**Required Parameters:**

- `path`: Path to the data catalog directory.

**Optional Parameters:**

- `fs_protocol`: Filesystem protocol ('file', 's3', 'gcs', 'azure', etc.).
- `fs_storage_options`: Protocol-specific storage options.
- `name`: Optional name identifier for the catalog configuration.

#### Basic usage examples

**Local Catalog Configuration:**

```python
from nautilus_trader.persistence.config import DataCatalogConfig

catalog_config = DataCatalogConfig(
    path="/path/to/catalog",
    fs_protocol="file",
    name="local_market_data"
)

# Convert to catalog instance
catalog = catalog_config.as_catalog()
```

**Cloud storage configuration:**

```python
catalog_config = DataCatalogConfig(
    path="s3://my-bucket/market-data/",
    fs_protocol="s3",
    fs_storage_options={
        "key": "your-access-key",
        "secret": "your-secret-key",
        "region": "us-west-2",
        "endpoint_url": "https://s3.us-west-2.amazonaws.com"
    },
    name="cloud_market_data"
)
```

#### Integration with live trading

`DataCatalogConfig` is commonly used in live trading configurations for historical data access:

```python
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.persistence.config import DataCatalogConfig

# Configure catalog for live system
catalog_config = DataCatalogConfig(
    path="/data/nautilus/catalog",
    fs_protocol="file",
    name="historical_data"
)

# Use in trading node configuration
node_config = TradingNodeConfig(
    # ... other configurations
    catalog=catalog_config,  # Enable historical data access
)
```

#### Streaming configuration

For streaming data to catalogs during live trading or backtesting, use `StreamingConfig`:

```python
from nautilus_trader.persistence.config import StreamingConfig, RotationMode
import pandas as pd

streaming_config = StreamingConfig(
    catalog_path="/path/to/streaming/catalog",
    fs_protocol="file",
    flush_interval_ms=1000,  # Flush every second
    replace_existing=False,
    rotation_mode=RotationMode.DAILY,
    rotation_interval=pd.Timedelta(hours=1),
    max_file_size=1024 * 1024 * 100,  # 100MB max file size
)
```

#### Use cases

**Historical Data Analysis:**

- Load historical data during live trading for strategy calculations.
- Access reference data for instrument lookups.
- Retrieve past performance metrics.

**Dynamic data loading:**

- Load data based on runtime conditions.
- Implement custom data loading strategies.
- Support multiple catalog sources.

**Research and development:**

- Interactive data exploration in Jupyter notebooks.
- Ad-hoc analysis and backtesting.
- Data quality validation and monitoring.

### Query system and dual backend architecture

The catalog's query system leverages a sophisticated dual-backend architecture that automatically selects the optimal query engine based on data type and query parameters.

#### Backend selection logic

**Rust backend (high performance):**

- **Supported Types**: OrderBookDelta, OrderBookDeltas, OrderBookDepth10, QuoteTick, TradeTick, Bar, MarkPriceUpdate.
- **Conditions**: Used when `files` parameter is None (automatic file discovery).
- **Benefits**: Optimized performance, memory efficiency, native Arrow integration.

**PyArrow backend (flexible):**

- **Supported Types**: All data types including custom data classes.
- **Conditions**: Used for custom data types or when `files` parameter is specified.
- **Benefits**: Advanced filtering, custom data support, complex query expressions.

#### Query methods and parameters

**Core query parameters:**

```python
catalog.query(
    data_cls=QuoteTick,                    # Data type to query
    identifiers=["EUR/USD.SIM"],           # Instrument identifiers
    start="2024-01-01T00:00:00Z",         # Start time (various formats supported)
    end="2024-01-02T00:00:00Z",           # End time
    where="bid > 1.1000",                 # PyArrow filter expression
    files=None,                           # Specific files (forces PyArrow backend)
)
```

**Time format support:**

- ISO 8601 strings: `"2024-01-01T00:00:00Z"`.
- UNIX nanoseconds: `1704067200000000000` (or ISO format: `"2024-01-01T00:00:00Z"`).
- Pandas Timestamps: `pd.Timestamp("2024-01-01", tz="UTC")`.
- Python datetime objects (timezone-aware recommended).

**Advanced filtering examples:**

```python
# Complex PyArrow expressions
catalog.query(
    data_cls=TradeTick,
    identifiers=["BTC/USD.BINANCE"],
    where="price > 50000 AND size > 1.0",
    start="2024-01-01",
    end="2024-01-02",
)

# Multiple instruments with metadata filtering
catalog.query(
    data_cls=Bar,
    identifiers=["AAPL.NASDAQ", "MSFT.NASDAQ"],
    where="volume > 1000000",
    metadata={"bar_type": "1-MINUTE-LAST"},
)
```

### Catalog operations

The catalog provides several operation functions for maintaining and organizing data files. These operations help optimize storage, improve query performance, and ensure data integrity.

#### Reset file names

Reset parquet file names to match their actual content timestamps. This ensures filename-based filtering works correctly.

**Reset all files in catalog:**

```python
# Reset all parquet files in the catalog
catalog.reset_all_file_names()
```

**Reset specific data type:**

```python
# Reset filenames for all quote tick files
catalog.reset_data_file_names(QuoteTick)

# Reset filenames for specific instrument's trade files
catalog.reset_data_file_names(TradeTick, "BTC/USD.BINANCE")
```

#### Consolidate catalog

Combine multiple small parquet files into larger files to improve query performance and reduce storage overhead.

**Consolidate entire catalog:**

```python
# Consolidate all files in the catalog
catalog.consolidate_catalog()

# Consolidate files within a specific time range
catalog.consolidate_catalog(
    start="2024-01-01T00:00:00Z",
    end="2024-01-02T00:00:00Z",
    ensure_contiguous_files=True
)
```

**Consolidate specific data type:**

```python
# Consolidate all quote tick files
catalog.consolidate_data(QuoteTick)

# Consolidate specific instrument's files
catalog.consolidate_data(
    TradeTick,
    identifier="BTC/USD.BINANCE",
    start="2024-01-01",
    end="2024-01-31"
)
```

#### Consolidate catalog by period

Split data files into fixed time periods for standardized file organization.

**Consolidate entire catalog by period:**

```python
import pandas as pd

# Consolidate all files by 1-day periods
catalog.consolidate_catalog_by_period(
    period=pd.Timedelta(days=1)
)

# Consolidate by 1-hour periods within time range
catalog.consolidate_catalog_by_period(
    period=pd.Timedelta(hours=1),
    start="2024-01-01T00:00:00Z",
    end="2024-01-02T00:00:00Z"
)
```

**Consolidate specific data by period:**

```python
# Consolidate quote data by 4-hour periods
catalog.consolidate_data_by_period(
    data_cls=QuoteTick,
    period=pd.Timedelta(hours=4)
)

# Consolidate specific instrument by 30-minute periods
catalog.consolidate_data_by_period(
    data_cls=TradeTick,
    identifier="EUR/USD.SIM",
    period=pd.Timedelta(minutes=30),
    start="2024-01-01",
    end="2024-01-31"
)
```

#### Delete data range

Remove data within a specified time range for specific data types and instruments. This operation permanently deletes data and handles file intersections intelligently.

**Delete entire catalog range:**

```python
# Delete all data within a time range across the entire catalog
catalog.delete_catalog_range(
    start="2024-01-01T00:00:00Z",
    end="2024-01-02T00:00:00Z"
)

# Delete all data from the beginning up to a specific time
catalog.delete_catalog_range(end="2024-01-01T00:00:00Z")
```

**Delete specific data type:**

```python
# Delete all quote tick data for a specific instrument
catalog.delete_data_range(
    data_cls=QuoteTick,
    identifier="BTC/USD.BINANCE"
)

# Delete trade data within a specific time range
catalog.delete_data_range(
    data_cls=TradeTick,
    identifier="EUR/USD.SIM",
    start="2024-01-01T00:00:00Z",
    end="2024-01-31T23:59:59Z"
)
```

:::warning
Delete operations permanently remove data and cannot be undone. Files that partially overlap the deletion range are split to preserve data outside the range.
:::

### Feather streaming and conversion

The catalog supports streaming data to temporary feather files during backtests, which can then be converted to permanent parquet format for efficient querying.

**Example: option greeks streaming**

```python
from option_trader.greeks import GreeksData
from nautilus_trader.persistence.config import StreamingConfig

# 1. Configure streaming for custom data
streaming = StreamingConfig(
    catalog_path=catalog.path,
    include_types=[GreeksData],
    flush_interval_ms=1000,
)

# 2. Run backtest with streaming enabled
engine_config = BacktestEngineConfig(streaming=streaming)
results = node.run()

# 3. Convert streamed data to permanent catalog
catalog.convert_stream_to_data(
    results[0].instance_id,
    GreeksData,
)

# 4. Query converted data
greeks_data = catalog.query(
    data_cls=GreeksData,
    start="2024-01-01",
    end="2024-01-31",
    where="delta > 0.5",
)
```

### Catalog summary

The NautilusTrader data catalog provides comprehensive market data management:

**Core features**:

- **Dual Backend**: Rust performance + Python flexibility.
- **Multi-Protocol**: Local, S3, GCS, Azure storage.
- **Streaming**: Feather → Parquet conversion pipeline.
- **Operations**: Reset file names, consolidate data, period-based organization.

**Key use cases**:

- **Backtesting**: Pre-configured data loading via BacktestDataConfig.
- **Live Trading**: On-demand data access via DataCatalogConfig.
- **Maintenance**: File consolidation and organization operations.
- **Research**: Interactive querying and analysis.

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

- Reads both the data JSON and metadata JSON files.
- Preserves row group sizes from original metadata.
- Uses ZSTD compression.
- Creates `<input>.parquet`.

### Migration process

The following migration examples both use trades data (you can also migrate the other data types in the same way).
All commands should be run from the root of the `persistence` crate directory.

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

### Best practices

- Always test migrations with a small dataset first.
- Maintain backups of original files.
- Verify data integrity after migration.
- Perform migrations in a staging environment before applying them to production data.

## Custom data

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

### Option greeks example

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
