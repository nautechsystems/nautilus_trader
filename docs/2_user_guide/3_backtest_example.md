---
jupyter:
  jupytext:
    formats: ipynb,md
    text_representation:
      extension: .md
      format_name: markdown
      format_version: '1.3'
      jupytext_version: 1.11.5
  kernelspec:
    display_name: Python (nautilus_trader)
    language: python
    name: nautilus_trader
---

# Complete Backtest Example

This notebook runs through a complete backtest example using raw data (external to nautilus) to a parameterised run 

<!-- #region tags=[] -->
## Getting some raw data

Before we start the notebook - as a once off we need to download some sample data for backtesting

For this notebook we will use Forex data from `histdata.com`, simply go to https://www.histdata.com/download-free-forex-historical-data/?/ascii/tick-data-quotes/ and select a Forex pair and one or more months of data to download.

Once you have downloaded the data, set the variable `DATA_DIR` below to the directory containing the data. By default it will use the users `Downloads` directory.
<!-- #endregion -->

```python
DATA_DIR = "~/Downloads/"
```

Run the cell below; you should see the files that you downloaded

```python
import fsspec
fs = fsspec.filesystem('file')
raw_files = fs.glob(f"{DATA_DIR}/HISTDATA*")
assert raw_files, f"Unable to find any histdata files in directory {DATA_DIR}"
raw_files
```

<!-- #region tags=[] -->
## The Data Catalog

Next we will load this raw data into the data catalog. The data catalog is a central store for Nautilus data, persisted in the [Parquet](https://parquet.apache.org) file format.

We have chosen parquet as the storage format for the following reasons:
- It performs much better than CSV/JSON/HDF5/etc in terms of compression (storage size) and read performance.
- It does not require any separate running components (for example a database).
- It is quick and simple for someone to get up and running with.
<!-- #endregion -->

## Loading data into the catalog

We can load data from various sources into the data catalog using helper methods in the `nautilus_trader.persistence.external.readers` module. The module contains methods for reading various data formats (csv, json, txt), minimising the amount of code required to get data loaded correctly into the data catalog.

The Forex data from `histdata` is stored in csv/text format, with fields `timestamp, bid_price, ask_price`. To load the data into the catalog, we simply write a function that converts each row into a Nautilus object (in this case, a `QuoteTick`). For this example, we will use the `TextReader` helper, which allows reading and applying a parsing function line by line.

Then, we simply instantiate a data catalog (passing in a directory where to store the data, by default we will just use the current directory) and pass our parsing function wrapping in the Reader class to `process_files`. We also need to know about which instrument this data is for; in this example, we will simply use one of the Nautilus test helpers to create a Forex instrument.

It should only take a couple of minutes to load the data (depending on how many months).

```python
import datetime
import pandas as pd

from nautilus_trader.persistence.catalog import DataCatalog
from nautilus_trader.persistence.external.core import process_files, write_objects
from nautilus_trader.persistence.external.readers import TextReader

from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.objects import Price, Quantity
from nautilus_trader.core.datetime import dt_to_unix_nanos


from nautilus_trader.backtest.data.providers import TestInstrumentProvider
```

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

We'll set up a catalog in the current working directory

```python
import os, shutil
CATALOG_PATH = os.getcwd() + "/catalog"

# Clear if it already exists, then create fresh
if os.path.exists(CATALOG_PATH):
    shutil.rmtree(CATALOG_PATH)
os.mkdir(CATALOG_PATH)
```

```python
AUDUSD = TestInstrumentProvider.default_fx_ccy("AUD/USD")

catalog = DataCatalog(CATALOG_PATH)

process_files(
    glob_path=f"{DATA_DIR}/HISTDATA*.zip",
    reader=TextReader(line_parser=parser),
    catalog=catalog,
)

# Also manually write the AUDUSD instrument to the catalog
write_objects(catalog, [AUDUSD])
```

## Using the Data Catalog 

Once data has been loaded into the catalog, the `catalog` instance can be used for loading data into the backtest engine, or simple for research purposes. It contains various methods to pull data from the catalog, like `quote_ticks` (show below))

```python
catalog.instruments()
```

```python
start = dt_to_unix_nanos(pd.Timestamp('2020-01-01', tz='UTC'))
end =  dt_to_unix_nanos(pd.Timestamp('2020-01-02', tz='UTC'))

catalog.quote_ticks(start=start, end=end)
```

## Configuring backtests

Nautilus has a top level object `BacktestRunConfig` that allows configuring a backtest in one place. It is a `Partialable` object (which means it can be configured in stages); the benefits of which are reduced boilerplate code when creating multiple backtest runs (for example when doing some sort of grid search over parameters).

### Staring with a Venue

We can start partially configuring the config with just a Venue:

```python
from nautilus_trader.backtest.config import BacktestRunConfig, BacktestVenueConfig, BacktestDataConfig, BacktestEngineConfig
from nautilus_trader.model.currencies import USD

# Create a `base` config object to be shared with all backtests
base = BacktestRunConfig(
    venues=[
        BacktestVenueConfig(
            name="SIM",
            oms_type="HEDGING",
            account_type="MARGIN",
            base_currency="USD",
            starting_balances=["1000000 USD"],
        )
    ]
)
base
```

### Adding Data

Notice many of the fields are `None` - we can continue to configure the backtest via `update`.

The `data_config` arg allows adding multiple data types (`quotes`, `trades`, `generic_data`), but for this example, we will simply load the quote ticks we added earlier.

```python
instrument = catalog.instruments(as_nautilus=True)[0]

data_config=[
    BacktestDataConfig(
        catalog_path=CATALOG_PATH,
        data_type=QuoteTick,
        instrument_id=instrument.id.value,
        start_time=1580398089820000000,
        end_time=1580504394501000000,
    )
]

config = base.update(
    data=data_config,
    engine=BacktestEngineConfig()
)

config
```

### Finally, add Strategy instances

We can perform a grid-search of some parameters by using the `replace` method, which returns a new copy of the config. We use the `ImportableStrategyConfig` object to tell nautilus where the `TradingStrategy` class exists, and add some config 

```python
from decimal import Decimal
from nautilus_trader.trading.config import ImportableStrategyConfig
from nautilus_trader.examples.strategies.ema_cross import EMACrossConfig


PARAM_SET = [
    {"fast_ema": 5, "slow_ema": 20},
    {"fast_ema": 10, "slow_ema": 50},
    {"fast_ema": 30, "slow_ema": 100},
]

configs = []
for params in PARAM_SET:
    strategies = [
        ImportableStrategyConfig(
            path="examples.strategies.ema_cross_simple:EMACross",
            config=EMACrossConfig(
                instrument_id=instrument.id.value,
                bar_type='AUD/USD.SIM-15-MINUTE-BID-INTERNAL',
                trade_size=Decimal(1_000_000),
                **params
            ),
        ),
    ]
    # Create the final config
    new = config.replace(strategies=strategies)
    
    configs.append(new)
```

### This gives us 3 parameter sets to backtest

```python
print("\n\n".join(map(str, configs)))
```

# Run the backtest

Finally, we can create a BacktestNode and run the backtest

```python
from nautilus_trader.backtest.node import BacktestNode
node = BacktestNode()
```

```python
task = node.build_graph(run_configs=configs)
task
```

```python
# Visualising the graph requires graphviz - `%pip install graphviz` in a notebook cell to install it

# task.visualize(rankdir='LR') 
```

^ Notice because our configs share the same data that only one instance of `load` is required


### Start up a local dask cluster to execute the graph

```python
# Create a local dask client - not a requirement, but allows parallelising the runs
from distributed import Client
client = Client(n_workers=2)
client
```

### Run the backtests!

```python tags=[]
results = task.compute()
```

### Compare the results

```python
results.plot_balances()
```

```python

```
