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
# # Backtest: Bybit OrderBook data
#
# Tutorial for [NautilusTrader](https://nautilustrader.io/docs/) a high-performance algorithmic trading platform and event driven backtester.
#
# [View source on GitHub](https://github.com/nautechsystems/nautilus_trader/blob/develop/docs/tutorials/backtest_binance_orderbook.ipynb).
#
# :::info
# We are currently working on this tutorial.
# :::

# %% [markdown]
# ## Overview
#
# This tutorial runs through how to set up the data catalog and a `BacktestNode` to backtest an `OrderBookImbalance` strategy or order book data. This example requires order book depth data provided by Bybit.
#

# %% [markdown]
# ## Prerequisites
#
# - Python 3.11+ installed
# - [JupyterLab](https://jupyter.org/) or similar installed (`pip install -U jupyterlab`)
# - [NautilusTrader](https://pypi.org/project/nautilus_trader/) latest release installed (`pip install -U nautilus_trader`)

# %% [markdown]
# ## Imports
#
# We'll start with all of our imports for the remainder of this guide:

# %%
import os
import shutil
from decimal import Decimal
from pathlib import Path

import pandas as pd

from nautilus_trader.adapters.bybit.loaders import BybitOrderBookDeltaDataLoader
from nautilus_trader.backtest.node import BacktestDataConfig
from nautilus_trader.backtest.node import BacktestEngineConfig
from nautilus_trader.backtest.node import BacktestNode
from nautilus_trader.backtest.node import BacktestRunConfig
from nautilus_trader.backtest.node import BacktestVenueConfig
from nautilus_trader.config import ImportableStrategyConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.model import OrderBookDelta
from nautilus_trader.persistence.catalog import ParquetDataCatalog
from nautilus_trader.persistence.wranglers import OrderBookDeltaDataWrangler
from nautilus_trader.test_kit.providers import TestInstrumentProvider


# %% [markdown]
# ## Loading data

# %%
# Path to your data directory, using user /Downloads as an example
DATA_DIR = "~/Downloads"

# %%
data_path = Path(DATA_DIR).expanduser() / "Data" / "Bybit"
raw_files = list(data_path.iterdir())
assert raw_files, f"Unable to find any histdata files in directory {data_path}"
raw_files

# %%
# We'll use orderbook depth 500 data provided by Bybit with limit of 1000000 rows
path_update = data_path / "2024-12-01_XRPUSDT_ob500.data.zip"
nrows = 1_000_000
df_raw = BybitOrderBookDeltaDataLoader.load(path_update, nrows=nrows)
df_raw.head()

# %% [markdown]
# ### Process deltas using a wrangler

# %%
XRPUSDT_BYBIT = TestInstrumentProvider.xrpusdt_linear_bybit()
wrangler = OrderBookDeltaDataWrangler(XRPUSDT_BYBIT)

deltas = wrangler.process(df_raw)
deltas.sort(key=lambda x: x.ts_init)  # Ensure data is non-decreasing by `ts_init`
deltas[:10]

# %% [markdown]
# ### Set up data catalog

# %%
CATALOG_PATH = os.getcwd() + "/catalog"

# Clear if it already exists, then create fresh
if os.path.exists(CATALOG_PATH):
    shutil.rmtree(CATALOG_PATH)
os.mkdir(CATALOG_PATH)

# Create a catalog instance
catalog = ParquetDataCatalog(CATALOG_PATH)

# %%
# Write instrument and ticks to catalog
catalog.write_data([XRPUSDT_BYBIT])
catalog.write_data(deltas)

# %%
# Confirm the instrument was written
catalog.instruments()

# %%
# Explore the available data in the catalog
start = dt_to_unix_nanos(pd.Timestamp("2022-11-01", tz="UTC"))
end = dt_to_unix_nanos(pd.Timestamp("2022-11-04", tz="UTC"))

deltas = catalog.order_book_deltas(start=start, end=end)
print(len(deltas))
deltas[:10]

# %% [markdown]
# ## Configure backtest

# %%
instrument = catalog.instruments()[0]
book_type = "L2_MBP"  # Ensure data book type matches venue book type

data_configs = [
    BacktestDataConfig(
        catalog_path=CATALOG_PATH,
        data_cls=OrderBookDelta,
        instrument_id=instrument.id,
        # start_time=start,  # Run across all data
        # end_time=end,  # Run across all data
    ),
]

venues_configs = [
    BacktestVenueConfig(
        name="BYBIT",
        oms_type="NETTING",
        account_type="CASH",
        base_currency=None,
        starting_balances=["200000 XRP", "100000 USDT"],
        book_type=book_type,  # <-- Venues book type
    ),
]

strategies = [
    ImportableStrategyConfig(
        strategy_path="nautilus_trader.examples.strategies.orderbook_imbalance:OrderBookImbalance",
        config_path="nautilus_trader.examples.strategies.orderbook_imbalance:OrderBookImbalanceConfig",
        config={
            "instrument_id": instrument.id,
            "book_type": book_type,
            "max_trade_size": Decimal("1.000"),
            "min_seconds_between_triggers": 1.0,
        },
    ),
]

# NautilusTrader currently exceeds the rate limit for Jupyter notebook logging (stdout output),
# this is why the `log_level` is set to "ERROR". If you lower this level to see
# more logging then the notebook will hang during cell execution. A fix is currently
# being investigated which involves either raising the configured rate limits for
# Jupyter, or throttling the log flushing from Nautilus.
# https://github.com/jupyterlab/jupyterlab/issues/12845
# https://github.com/deshaw/jupyterlab-limit-output
config = BacktestRunConfig(
    engine=BacktestEngineConfig(
        strategies=strategies,
        logging=LoggingConfig(log_level="ERROR"),
    ),
    data=data_configs,
    venues=venues_configs,
)

config

# %% [markdown]
# ## Run the backtest

# %%
node = BacktestNode(configs=[config])

result = node.run()

# %%
result

# %%
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.model import Venue


engine: BacktestEngine = node.get_engine(config.id)

engine.trader.generate_order_fills_report()

# %%
engine.trader.generate_positions_report()

# %%
engine.trader.generate_account_report(Venue("BYBIT"))

# %%
