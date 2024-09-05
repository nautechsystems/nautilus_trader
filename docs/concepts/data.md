# Data

The NautilusTrader platform defines a range of built-in data types crafted specifically to represent 
a trading domain:

- `OrderBookDelta` (L1/L2/L3): Most granular order book updates
- `OrderBookDeltas` (L1/L2/L3): Bundles multiple order book deltas
- `OrderBookDepth10`: Aggregated order book snapshot (10 levels per side)
- `QuoteTick`: Top-of-book best bid and ask prices and sizes
- `TradeTick`: A single trade/match event between counterparties
- `Bar`: OHLCV bar data, aggregated using a specific *aggregation method*
- `Instrument`: General base class for a tradable instrument
- `InstrumentStatus`: An instrument level status event
- `InstrumentClose`: An instrument closing price

Each of these data types inherits from `Data`, which defines two fields:
- `ts_event`: UNIX timestamp (nanoseconds) when the data event occurred
- `ts_init`: UNIX timestamp (nanoseconds) when the object was initialized

This inheritance ensures chronological data ordering (vital for backtesting), while also enhancing analytics.

Consistency is key; data flows through the platform in exactly the same way for all system [environment contexts](/concepts/architecture.md#environment-contexts) (`backtest`, `sandbox`, `live`)
primarily through the `MessageBus` to the `DataEngine` and onto subscribed or registered handlers.

For those seeking customization, the platform supports user-defined data types. Refer to the advanced [Custom data guide](advanced/custom_data.md) for further details.

## Loading data

NautilusTrader facilitates data loading and conversion for three main use cases:
- Populating the `BacktestEngine` directly to run backtests
- Persisting the Nautilus-specific Parquet format for the data catalog via `ParquetDataCatalog.write_data(...)` to be later used with a `BacktestNode`
- For research purposes (to ensure data is consistent between research and backtesting)

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

**Process flow:**
1. Raw data (e.g., CSV) is input into the pipeline
2. DataLoader processes the raw data and converts it into a `pd.DataFrame`
3. DataWrangler further processes the `pd.DataFrame` to generate a list of Nautilus objects
4. The Nautilus `list[Data]` is the output of the data loading process

This diagram illustrates how raw data is transformed into Nautilus data structures.
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
- `BinanceOrderBookDeltaDataLoader.load(...)` which reads CSV files provided by Binance from disk, and returns a `pd.DataFrame`
- `OrderBookDeltaDataWrangler.process(...)` which takes the `pd.DataFrame` and returns `list[OrderBookDelta]`

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
- It performs much better than CSV/JSON/HDF5/etc in terms of compression ratio (storage size) and read performance
- It does not require any separate running components (for example a database)
- It is quick and simple to get up and running with

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

The following example shows the above list of Binance `OrderBookDelta` objects being written.
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
