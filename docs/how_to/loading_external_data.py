# %% [markdown]
# # Loading External Data
#
# Load CSV market data into the Parquet data catalog, then run a backtest with
# `BacktestNode`. This is a common workflow when you have historical data from an
# external vendor that is not directly supported by a NautilusTrader adapter.
#
# [View source on GitHub](https://github.com/nautechsystems/nautilus_trader/blob/develop/docs/how_to/loading_external_data.py).

# %%
import os
import shutil
from pathlib import Path

import pandas as pd

from nautilus_trader.backtest import BacktestNode
from nautilus_trader.config import BacktestDataConfig
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.config import BacktestRunConfig
from nautilus_trader.config import BacktestVenueConfig
from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.model import Quantity
from nautilus_trader.model import QuoteTick
from nautilus_trader.persistence import ParquetDataCatalog
from nautilus_trader.persistence.wranglers import QuoteTickDataWrangler
from nautilus_trader.testkit.providers import CSVTickDataLoader
from nautilus_trader.testkit.providers import TestInstrumentProvider
from nautilus_trader.trading import EmaCrossConfig


# %% [markdown]
# ## Load and wrangle the data
#
# Place CSV tick files (e.g. from [histdata.com](https://www.histdata.com/))
# into `~/Downloads/Data/HISTDATA/`. Set the `NAUTILUS_DATA_DIR` environment
# variable to the parent directory if your data lives elsewhere.
# `CSVTickDataLoader` reads the raw CSV into a DataFrame, and
# `QuoteTickDataWrangler` converts it into Nautilus `QuoteTick` objects.

# %%
DATA_DIR = Path(os.environ.get("NAUTILUS_DATA_DIR", "~/Downloads/Data")).expanduser() / "HISTDATA"

# %%
path = DATA_DIR
raw_files = [
    f for f in path.iterdir() if f.is_file() and (f.suffix == ".csv" or f.name.endswith(".csv.gz"))
]
assert raw_files, f"Unable to find any data files in directory {path}"
raw_files

# %%
# Load the first data file into a pandas DataFrame
df = CSVTickDataLoader.load(raw_files[0], index_col=0, datetime_format="%Y%m%d %H%M%S%f")
df = df.iloc[:, :2]
df.columns = ["bid_price", "ask_price"]

# Process quotes using a wrangler
EURUSD = TestInstrumentProvider.default_fx_ccy("EUR/USD")
wrangler = QuoteTickDataWrangler(EURUSD)

ticks = wrangler.process(df)

# %% [markdown]
# ## Write to the data catalog
#
# Create a `ParquetDataCatalog` and write the instrument definition and tick
# data. The catalog stores data in Parquet format for efficient querying across
# backtest runs.

# %%
CATALOG_PATH = Path.cwd() / "catalog"

# Clear if it already exists, then create fresh
if CATALOG_PATH.exists():
    shutil.rmtree(CATALOG_PATH)
CATALOG_PATH.mkdir()

catalog = ParquetDataCatalog(CATALOG_PATH)

# %%
catalog.write_data([EURUSD])
catalog.write_data(ticks)

# %%
# Verify instruments written to catalog
catalog.instruments()

# %%
start = dt_to_unix_nanos(pd.Timestamp("2020-01-03", tz="UTC"))
end = dt_to_unix_nanos(pd.Timestamp("2020-01-04", tz="UTC"))

ticks = catalog.quotes(instrument_ids=[EURUSD.id.value], start=start, end=end)
ticks[:10]

# %% [markdown]
# ## Configure and run the backtest
#
# Set up venue and data configs, build the node, then register the built-in
# `EmaCross` strategy. The same node and strategy pattern carries forward to
# live trading with `LiveNode`.

# %%
instrument = catalog.instruments()[0]

venue_configs = [
    BacktestVenueConfig(
        name="SIM",
        oms_type="HEDGING",
        account_type="MARGIN",
        base_currency="USD",
        starting_balances=["1000000 USD"],
    ),
]

data_configs = [
    BacktestDataConfig(
        catalog_path=str(catalog.path),
        data_cls=QuoteTick,
        instrument_id=instrument.id,
        start_time=start,
        end_time=end,
    ),
]

config = BacktestRunConfig(
    engine=BacktestEngineConfig(),
    data=data_configs,
    venues=venue_configs,
)

# %%
node = BacktestNode(configs=[config])
node.build()
node.add_builtin_strategy(
    config.id,
    "EmaCross",
    EmaCrossConfig(
        instrument_id=instrument.id,
        trade_size=Quantity.from_int(1_000_000),
        fast_period=10,
        slow_period=20,
    ),
)

[result] = node.run()

# %%
result
