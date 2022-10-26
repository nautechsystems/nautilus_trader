# Complete Backtest Example

This notebook runs through a complete backtest example using raw data (external to Nautilus) to a single backtest run.

## Imports

We'll start with all of our imports for the remainder of this guide:

```python
import datetime
import os
import shutil
from decimal import Decimal

import fsspec
import pandas as pd
from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.objects import Price, Quantity

from nautilus_trader.backtest.data.providers import TestInstrumentProvider
from nautilus_trader.backtest.node import BacktestNode, BacktestVenueConfig, BacktestDataConfig, BacktestRunConfig, BacktestEngineConfig
from nautilus_trader.config.common import ImportableStrategyConfig
from nautilus_trader.persistence.catalog import ParquetDataCatalog
from nautilus_trader.persistence.external.core import process_files, write_objects
from nautilus_trader.persistence.external.readers import TextReader
```

## Getting some raw data

As a once off before we start the notebook - we need to download some sample data for backtesting.

For this example we will use FX data from `histdata.com`. Simply go to https://www.histdata.com/download-free-forex-historical-data/?/ascii/tick-data-quotes/ and select an FX pair, then select one or more months of data to download.

Once you have downloaded the data, set the variable `DATA_DIR` below to the directory containing the data. By default, it will use the users `Downloads` directory.
<!-- #endregion -->

```python
DATA_DIR = "~/Downloads/"
```

Run the cell below; you should see the files that you downloaded:

```python
fs = fsspec.filesystem('file')
raw_files = fs.glob(f"{DATA_DIR}/HISTDATA*")
assert raw_files, f"Unable to find any histdata files in directory {DATA_DIR}"
raw_files
```

## The Data Catalog

Next we will load this raw data into the data catalog. The data catalog is a central store for Nautilus data, persisted in the [Parquet](https://parquet.apache.org) file format.

We have chosen parquet as the storage format for the following reasons:
- It performs much better than CSV/JSON/HDF5/etc in terms of compression ratio (storage size) and read performance
- It does not require any separate running components (for example a database)
- It is quick and simple to get up and running with

## Loading data into the catalog

We can load data from various sources into the data catalog using helper methods in the `nautilus_trader.persistence.external.readers` module. The module contains methods for reading various data formats (CSV, JSON, text), minimising the amount of code required to get data loaded correctly into the data catalog.

The FX data from `histdata` is stored in CSV/text format, with fields `timestamp, bid_price, ask_price`. To load the data into the catalog, we simply write a function that converts each row into a Nautilus object (in this case, a `QuoteTick`). For this example, we will use the `TextReader` helper, which allows reading and applying a parsing function line by line.

Then, we simply instantiate a `ParquetDataCatalog` (passing in a directory where to store the data, by default we will just use the current directory) and pass our parsing function wrapping in the Reader class to `process_files`. We also need to know about which instrument this data is for; in this example, we will simply use one of the Nautilus test helpers to create a FX instrument.

It should only take a couple of minutes to load the data (depending on how many months).


```python
def parser(line):
    ts, bid, ask, idx = line.split(b",")
    dt = pd.Timestamp(datetime.datetime.strptime(ts.decode(), "%Y%m%d %H%M%S%f"), tz='UTC')
    yield QuoteTick(
        instrument_id=AUDUSD.id,
        bid=Price.from_str(bid.decode()),
        ask=Price.from_str(ask.decode()),
        bid_size=Quantity.from_int(100_000),
        ask_size=Quantity.from_int(100_000),
        ts_event=dt_to_unix_nanos(dt),
        ts_init=dt_to_unix_nanos(dt),
    )
```

We'll set up a catalog in the current working directory.

```python
CATALOG_PATH = os.getcwd() + "/catalog"

# Clear if it already exists, then create fresh
if os.path.exists(CATALOG_PATH):
    shutil.rmtree(CATALOG_PATH)
os.mkdir(CATALOG_PATH)
```

```python
AUDUSD = TestInstrumentProvider.default_fx_ccy("AUD/USD")

catalog = ParquetDataCatalog(CATALOG_PATH)

process_files(
    glob_path=f"{DATA_DIR}/HISTDATA*.zip",
    reader=TextReader(line_parser=parser),
    catalog=catalog,
)

# Also manually write the AUD/USD instrument to the catalog
write_objects(catalog, [AUDUSD])
```

## Using the Data Catalog 

Once data has been loaded into the catalog, the `catalog` instance can be used for loading data for backtests, or simple for research purposes. It contains various methods to pull data from the catalog, like `quote_ticks` (show below).

```python
catalog.instruments()
```

```python
import pandas as pd
from nautilus_trader.core.datetime import dt_to_unix_nanos


start = dt_to_unix_nanos(pd.Timestamp('2020-01-01', tz='UTC'))
end =  dt_to_unix_nanos(pd.Timestamp('2020-01-02', tz='UTC'))

catalog.quote_ticks(start=start, end=end)
```

## Configuring backtests

Nautilus uses a `BacktestRunConfig` object, which allows configuring a backtest in one place. It is a `Partialable` object (which means it can be configured in stages); the benefits of which are reduced boilerplate code when creating multiple backtest runs (for example when doing some sort of grid search over parameters).

### Adding data and venues

```python
instrument = catalog.instruments(as_nautilus=True)[0]

venues_config=[
    BacktestVenueConfig(
        name="SIM",
        oms_type="HEDGING",
        account_type="MARGIN",
        base_currency="USD",
        starting_balances=["1000000 USD"],
    )
]

data_config=[
    BacktestDataConfig(
        catalog_path=str(ParquetDataCatalog.from_env().path),
        data_cls=QuoteTick,
        instrument_id=instrument.id.value,
        start_time=1580398089820000000,
        end_time=1580504394501000000,
    )
]

strategies = [
    ImportableStrategyConfig(
        strategy_path="nautilus_trader.examples.strategies.ema_cross:EMACross",
        config_path="nautilus_trader.examples.strategies.ema_cross:EMACrossConfig",
        config=EMACrossConfig(
            instrument_id=instrument.id.value,
            bar_type="EUR/USD.SIM-15-MINUTE-BID-INTERNAL",
            fast_ema=10,
            slow_ema=20,
            trade_size=Decimal(1_000_000),
        ),
    ),
]

config = BacktestRunConfig(
    engine=BacktestEngineConfig(strategies=strategies),
    data=data_config,
    venues=venues_config,
)

```

## Run the backtest!

```python
node = BacktestNode(configs=[config])

results = node.run()
```
