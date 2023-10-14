# Backtest (high-level API)

This tutorial runs through the following: 
- How to load raw data (external to Nautilus) into the data catalog
- How to setup configuration objects for a `BacktestNode`
- How to run backtests with a  `BacktestNode`

## Imports

We'll start with all of our imports for the remainder of this guide:

```python
import datetime
import os
import shutil
from decimal import Decimal
from pathlib import Path

import fsspec
import pandas as pd

from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.objects import Price, Quantity
from nautilus_trader.backtest.node import BacktestNode, BacktestVenueConfig, BacktestDataConfig, BacktestRunConfig, BacktestEngineConfig
from nautilus_trader.config.common import ImportableStrategyConfig
from nautilus_trader.persistence.catalog import ParquetDataCatalog
from nautilus_trader.persistence.wranglers import QuoteTickDataWrangler
from nautilus_trader.test_kit.providers import CSVTickDataLoader
from nautilus_trader.test_kit.providers import TestInstrumentProvider
```

## Getting raw data

As a once off before we start the notebook - we need to download some sample data for backtesting.

For this example we will use FX data from `histdata.com`. Simply go to https://www.histdata.com/download-free-forex-historical-data/?/ascii/tick-data-quotes/ and select an FX pair, then select one or more months of data to download.

Once you have downloaded the data, set the variable `DATA_DIR` below to the directory containing the data. By default, it will use the users `Downloads` directory.
<!-- #endregion -->

```python
DATA_DIR = "~/Downloads/"
```

Then place the data archive into a `/"HISTDATA"` directory and run the cell below; you should see the files that you downloaded:

```python
path = Path(DATA_DIR).expanduser() / "HISTDATA"
raw_files = list(path.iterdir())
assert raw_files, f"Unable to find any histdata files in directory {path}"
raw_files
```

## Loading data into the Data Catalog

The FX data from `histdata` is stored in CSV/text format, with fields `timestamp, bid_price, ask_price`.
Firstly, we need to load this raw data into a `pandas.DataFrame` which has a compatible schema for Nautilus quote ticks.

Then we can create Nautilus `QuoteTick` objects by processing the DataFrame with a `QuoteTickDataWrangler`.

```python
# Here we just take the first data file found and load into a pandas DataFrame
df = CSVTickDataLoader.load(raw_files[0], index_col=0, format="%Y%m%d %H%M%S%f")
df.columns = ["bid_price", "ask_price"]

# Process quote ticks using a wrangler
EURUSD = TestInstrumentProvider.default_fx_ccy("EUR/USD")
wrangler = QuoteTickDataWrangler(EURUSD)

ticks = wrangler.process(df)
```

Next, we simply instantiate a `ParquetDataCatalog` (passing in a directory where to store the data, by default we will just use the current directory).
We can then write the instrument and tick data to the catalog, it should only take a couple of minutes to load the data (depending on how many months).

```python
CATALOG_PATH = os.getcwd() + "/catalog"

# Clear if it already exists, then create fresh
if os.path.exists(CATALOG_PATH):
    shutil.rmtree(CATALOG_PATH)
os.mkdir(CATALOG_PATH)

# Create a catalog instance
catalog = ParquetDataCatalog(CATALOG_PATH)
```

```python
# Write instrument and ticks to catalog (this currently takes a minute - investigating)
catalog.write_data([EURUSD])
catalog.write_data(ticks)
```

## Using the Data Catalog 

Once data has been loaded into the catalog, the `catalog` instance can be used for loading data for backtests, or simply for research purposes. 
It contains various methods to pull data from the catalog, such as `.instruments(...)` and `quote_ticks(...)` (shown below).

```python
catalog.instruments()
```

```python
start = dt_to_unix_nanos(pd.Timestamp("2020-01-03", tz="UTC"))
end =  dt_to_unix_nanos(pd.Timestamp("2020-01-04", tz="UTC"))

catalog.quote_ticks(instrument_ids=[EURUSD.id.value], start=start, end=end)
```

## Configuring backtests

Nautilus uses a `BacktestRunConfig` object, which allows configuring a backtest in one place. It is a `Partialable` object (which means it can be configured in stages); the benefits of which are reduced boilerplate code when creating multiple backtest runs (for example when doing some sort of grid search over parameters).

### Adding data and venues

```python
instrument = catalog.instruments(as_nautilus=True)[0]

venue_configs = [
    BacktestVenueConfig(
        name="SIM",
        oms_type="HEDGING",
        account_type="MARGIN",
        base_currency="USD",
        starting_balances=["1_000_000 USD"],
    ),
]

data_configs = [
    BacktestDataConfig(
        catalog_path=str(ParquetDataCatalog.from_env().path),
        data_cls=QuoteTick,
        instrument_id=instrument.id.value,
        start_time=start,
        end_time=end,
    ),
]

strategies = [
    ImportableStrategyConfig(
        strategy_path="nautilus_trader.examples.strategies.ema_cross:EMACross",
        config_path="nautilus_trader.examples.strategies.ema_cross:EMACrossConfig",
        config=dict(
            instrument_id=instrument.id.value,
            bar_type="EUR/USD.SIM-15-MINUTE-BID-INTERNAL",
            fast_ema_period=10,
            slow_ema_period=20,
            trade_size=Decimal(1_000_000),
        ),
    ),
]

config = BacktestRunConfig(
    engine=BacktestEngineConfig(strategies=strategies),
    data=data_configs,
    venues=venue_configs,
)

```

## Run the backtest!

```python
node = BacktestNode(configs=[config])

results = node.run()
results
```
