# Data

The NautilusTrader platform defines a range of built-in data types crafted specifically to represent 
a trading domain:

- `OrderBookDelta` (L1/L2/L3) - Most granular order book updates
- `OrderBookDeltas` (L1/L2/L3) - Bundles multiple order book deltas
- `QuoteTick` - Top-of-book best bid and ask prices and sizes
- `TradeTick` - A single trade/match event between counterparties
- `Bar` - OHLCV data aggregated using a specific method
- `Ticker` - General base class for a symbol ticker
- `Instrument` - General base class for a tradable instrument
- `VenueStatus` - A venue level status event
- `InstrumentStatus` - An instrument level status event
- `InstrumentClose` - An instrument closing price

Each of these data types inherits from `Data`, which defines two fields:
- `ts_event` - The UNIX timestamp (nanoseconds) when the data event occurred
- `ts_init` - The UNIX timestamp (nanoseconds) when the object was initialized

This inheritance ensures chronological data ordering, vital for backtesting, while also enhancing analytics.

Consistency is key; data flows through the platform in exactly the same way between all system contexts (backtest, sandbox and live),
primarily through the `MessageBus` to the `DataEngine` and onto subscribed or registered handlers.

For those seeking customization, the platform supports user-defined data types. Refer to the [advanced custom guide](/docs/concepts/advanced/custom_data.md) for more details.

## Loading data

NautilusTrader facilitates data loading and conversion for three main use cases:
- Populating the `BacktestEngine` directly
- Persisting the Nautilus-specific Parquet format via `ParquetDataCatalog.write_data(...)` to be used with a `BacktestNode`
- Research purposes

Regardless of the destination, the process remains the same: converting diverse external data formats into Nautilus data structures.
To achieve this two components are necessary:
- A data loader which can read the data and return a `pd.DataFrame` with the correct schema for the desired Nautilus object
- A data wrangler which takes this `pd.DataFrame` and returns a `list[Data]` of Nautilus objects

`raw data (e.g. CSV)` -> `*DataLoader` -> `pd.DataFrame` -> `*DataWrangler` -> Nautilus `list[Data]`

Conceretely, this would involve for example:
- `BinanceOrderBookDeltaDataLoader.load(...)` which reads CSV files provided by Binance from disk, and returns a `pd.DataFrame`
- `OrderBookDeltaDataWrangler.process(...)` which takes the `pd.DataFrame` and returns `list[OrderBookDelta]`

The following example shows how to accomplish the above in Python:
```python
import os

from nautilus_trader import PACKAGE_ROOT
from nautilus_trader.persistence.loaders import BinanceOrderBookDeltaDataLoader
from nautilus_trader.persistence.wranglers import OrderBookDeltaDataWrangler
from nautilus_trader.test_kit.providers import TestInstrumentProvider


# Load raw data
data_path = os.path.join(PACKAGE_ROOT, "tests/test_data/binance-btcusdt-depth-snap.csv")
df = BinanceOrderBookDeltaDataLoader.load(data_path)

# Setup a wrangler
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

The Arrow schemas used for the Parquet format are either single sourced in the core `persistence` Rust library, or available
from the `/serialization/arrow/schema.py` module.

```{note}
2023-10-14: The current plan is to eventually phase out the Python schemas module, so that all schemas are single sourced in the Rust core.
```

**This doc is an evolving work in progress and will continue to describe the data catalog more fully...**
