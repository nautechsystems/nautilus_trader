# ---
# jupyter:
#   jupytext:
#     formats: py:percent
#     text_representation:
#       extension: .py
#       format_name: percent
#       format_version: '1.3'
#       jupytext_version: 1.17.3
#   kernelspec:
#     display_name: Python 3 (ipykernel)
#     language: python
#     name: python3
# ---

# %% [markdown]
# Note: Use the jupytext python package to be able to open this python file in jupyter as a notebook.
# Also run `jupytext-config set-default-viewer` to open jupytext python files as notebooks by default.

# %% [markdown]
# # Backtest (high-level API)
#
# Tutorial for [NautilusTrader](https://nautilustrader.io/docs/) a high-performance algorithmic trading platform and event driven backtester.
#
# [View source on GitHub](https://github.com/nautechsystems/nautilus_trader/blob/develop/docs/getting_started/backtest_high_level.ipynb).

# %% [markdown]
# ## Overview
#
# This tutorial walks through how to use a `BacktestNode` to backtest a simple EMA cross strategy
# on a simulated FX ECN venue using historical quote tick data.
#
# The following points will be covered:
# - Load raw data (external to Nautilus) into the data catalog.
# - Set up configuration objects for a `BacktestNode`.
# - Run backtests with a `BacktestNode`.
#

# %% [markdown]
# ## Prerequisites
# - Python 3.11+ installed.
# - [JupyterLab](https://jupyter.org/) or similar installed (`pip install -U jupyterlab`).
# - [NautilusTrader](https://pypi.org/project/nautilus_trader/) latest release installed (`pip install -U nautilus_trader`).
#

# %% [markdown]
# ## Imports
#
# We'll start with all of our imports for the remainder of this tutorial.

# %%
import shutil
from decimal import Decimal
from pathlib import Path

import pandas as pd

from nautilus_trader.backtest.node import BacktestDataConfig
from nautilus_trader.backtest.node import BacktestEngineConfig
from nautilus_trader.backtest.node import BacktestNode
from nautilus_trader.backtest.node import BacktestRunConfig
from nautilus_trader.backtest.node import BacktestVenueConfig
from nautilus_trader.config import ImportableStrategyConfig
from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.model import QuoteTick
from nautilus_trader.persistence.catalog import ParquetDataCatalog
from nautilus_trader.persistence.wranglers import QuoteTickDataWrangler
from nautilus_trader.test_kit.providers import CSVTickDataLoader
from nautilus_trader.test_kit.providers import TestInstrumentProvider


# %% [markdown]
# As a one-off before we start the notebook, we need to download some sample data for backtesting.
#
# For this example we will use FX data from `histdata.com`. Simply go to https://www.histdata.com/download-free-forex-historical-data/?/ascii/tick-data-quotes/ and select an FX pair, then select one or more months of data to download.
#
# Examples of downloaded files:
#
# - `DAT_ASCII_EURUSD_T_202410.csv` (EUR\USD data for month 2024-10)
# - `DAT_ASCII_EURUSD_T_202411.csv` (EUR\USD data for month 2024-11)
#
# Once you have downloaded the data:
#
# 1. Copy files like the ones above into one folder—for example `~/Downloads/Data/` (by default, it will use the user's `Downloads/Data/` directory).
# 2. Set the `DATA_DIR` variable below to the directory containing the data.
#

# %%
DATA_DIR = "~/Downloads/Data/"

# %%
path = Path(DATA_DIR).expanduser()
raw_files = list(path.iterdir())
assert raw_files, f"Unable to find any histdata files in directory {path}"
raw_files

# %% [markdown]
# ## Loading data into the Parquet data catalog
#
# Histdata stores the FX data in CSV/text format with fields `timestamp, bid_price, ask_price`.
# First, load this raw data into a `pandas.DataFrame` with a schema compatible with Nautilus quotes.
#
# Then create Nautilus `QuoteTick` objects by processing the DataFrame with a `QuoteTickDataWrangler`.
#

# %%
# Here we just take the first data file found and load into a pandas DataFrame
df = CSVTickDataLoader.load(
    file_path=raw_files[0],  # Input 1st CSV file
    index_col=0,  # Use 1st column in data as index for dataframe
    header=None,  # There are no column names in CSV files
    names=["timestamp", "bid_price", "ask_price", "volume"],  # Specify names to individual columns
    usecols=[
        "timestamp",
        "bid_price",
        "ask_price",
    ],  # Read only these columns from CSV file into dataframe
    parse_dates=True,  # Specify columns containing date/time
    date_format="%Y%m%d %H%M%S%f",  # Format for parsing datetime
)

# Let's make sure data are sorted by timestamp
df = df.sort_index()

# Preview of loaded dataframe
df.head(2)

# %%
# Process quotes using a wrangler
EURUSD = TestInstrumentProvider.default_fx_ccy("EUR/USD")
wrangler = QuoteTickDataWrangler(EURUSD)

ticks = wrangler.process(df)

# Preview: see first 2 ticks
ticks[0:2]

# %% [markdown]
# See the [Loading data](../concepts/data) guide for further details.
#
# Next, instantiate a `ParquetDataCatalog` (pass in a directory to store the data; by default we use the current directory).
# Write the instrument and tick data to the catalog. Loading the data should only take a couple of minutes, depending on how many months you include.
#

# %%
CATALOG_PATH = Path.cwd() / "catalog"

# Clear if it already exists, then create fresh
if CATALOG_PATH.exists():
    shutil.rmtree(CATALOG_PATH)
CATALOG_PATH.mkdir(parents=True)

# Create a catalog instance
catalog = ParquetDataCatalog(CATALOG_PATH)

# Write instrument to the catalog
catalog.write_data([EURUSD])

# Write ticks to catalog
catalog.write_data(ticks)

# %% [markdown]
# ## Using the Data Catalog
#
# After you load data into the catalog, use the `catalog` instance to load data for backtests or research.
# It contains various methods to pull data from the catalog, such as `.instruments(...)` and `quote_ticks(...)` (shown below).
#

# %%
# Get list of all instruments in catalog
catalog.instruments()

# %%
# See 1st instrument from catalog
instrument = catalog.instruments()[0]
instrument

# %%
# Query quote-ticks from catalog
start = dt_to_unix_nanos(pd.Timestamp("2024-10-01", tz="UTC"))
end = dt_to_unix_nanos(pd.Timestamp("2024-10-15", tz="UTC"))
selected_quote_ticks = catalog.quote_ticks(instrument_ids=[EURUSD.id.value], start=start, end=end)

# Preview first
selected_quote_ticks[:2]

# %% [markdown]
# ## Add venues

# %%
venue_configs = [
    BacktestVenueConfig(
        name="SIM",
        oms_type="HEDGING",
        account_type="MARGIN",
        base_currency="USD",
        starting_balances=["1_000_000 USD"],
    ),
]

# %% [markdown]
# ## Add data

# %%
str(CATALOG_PATH)

# %%
data_configs = [
    BacktestDataConfig(
        catalog_path=str(CATALOG_PATH),
        data_cls=QuoteTick,
        instrument_id=instrument.id,
        start_time=start,
        end_time=end,
    ),
]

# %% [markdown]
# ## Add strategies

# %%
strategies = [
    ImportableStrategyConfig(
        strategy_path="nautilus_trader.examples.strategies.ema_cross:EMACross",
        config_path="nautilus_trader.examples.strategies.ema_cross:EMACrossConfig",
        config={
            "instrument_id": instrument.id,
            "bar_type": "EUR/USD.SIM-15-MINUTE-BID-INTERNAL",
            "fast_ema_period": 10,
            "slow_ema_period": 20,
            "trade_size": Decimal(1_000_000),
        },
    ),
]

# %% [markdown]
# ## Configure backtest
#
# Nautilus uses a `BacktestRunConfig` object to centralize backtest configuration.
# The `BacktestRunConfig` is Partialable, so you can configure it in stages.
# This design reduces boilerplate when you create multiple backtest runs (for example when performing a parameter grid search).
#

# %%
config = BacktestRunConfig(
    engine=BacktestEngineConfig(strategies=strategies),
    data=data_configs,
    venues=venue_configs,
)

# %% [markdown]
# ## Run backtest
#
# Now we can run the backtest node, which will simulate trading across the entire data stream.

# %%
node = BacktestNode(configs=[config])

results = node.run()
results
