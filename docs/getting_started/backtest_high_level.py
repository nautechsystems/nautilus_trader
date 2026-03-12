# %% [markdown]
# # Backtest (high-level API)
#
# Tutorial for [NautilusTrader](https://nautilustrader.io/docs/latest/) a high-performance algorithmic trading platform and event-driven backtester.
#
# [View source on GitHub](https://github.com/nautechsystems/nautilus_trader/blob/develop/docs/getting_started/backtest_high_level.py).

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
# - Python 3.12+ installed.
# - [NautilusTrader](https://pypi.org/project/nautilus_trader/) latest release installed (`uv pip install nautilus_trader`).

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
# Before starting, download some sample data for backtesting.
#
# For this example we will use FX data from `histdata.com`. Go to https://www.histdata.com/download-free-forex-historical-data/?/ascii/tick-data-quotes/ and select an FX pair, then select one or more months of data to download.
#
# Examples of downloaded files:
#
# - `DAT_ASCII_EURUSD_T_202410.csv` (EUR/USD data for month 2024-10)
# - `DAT_ASCII_EURUSD_T_202411.csv` (EUR/USD data for month 2024-11)
#
# Once you have downloaded the data:
#
# 1. Extract the CSV files and copy them into a folder, for example `~/Downloads/Data/HISTDATA/`.
# 2. Set the `DATA_DIR` variable below to the directory containing the CSV files.

# %%
DATA_DIR = "~/Downloads/Data/HISTDATA/"

# %%
path = Path(DATA_DIR).expanduser()
raw_files = [
    f for f in path.iterdir() if f.is_file() and (f.suffix == ".csv" or f.name.endswith(".csv.gz"))
]
assert raw_files, f"Unable to find any CSV files in directory {path}"
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
# Load the first CSV file into a pandas DataFrame
df = CSVTickDataLoader.load(
    file_path=raw_files[0],
    index_col=0,
    header=None,
    names=["timestamp", "bid_price", "ask_price", "volume"],
    usecols=["timestamp", "bid_price", "ask_price"],
    parse_dates=["timestamp"],
    date_format="%Y%m%d %H%M%S%f",
)

df = df.sort_index()
df.head(2)

# %%
# Process quotes using a wrangler
EURUSD = TestInstrumentProvider.default_fx_ccy("EUR/USD")
wrangler = QuoteTickDataWrangler(EURUSD)

ticks = wrangler.process(df)

# Preview: see first 2 ticks
ticks[0:2]

# %% [markdown]
# See the [Loading data](../concepts/data) guide for more details.
#
# Instantiate a `ParquetDataCatalog` with a storage directory (here we use the current directory).
# Write the instrument and tick data to the catalog.
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
# The catalog provides methods like `.instruments(...)` and `.quote_ticks(...)` to query stored data.
#

# %%
# Get list of all instruments in catalog
catalog.instruments()

# %%
# See 1st instrument from catalog
instrument = catalog.instruments()[0]
instrument

# %%
# Query quote ticks from catalog to determine the data range
all_ticks = catalog.quote_ticks(instrument_ids=[EURUSD.id.value])
print(f"Total ticks in catalog: {len(all_ticks)}")

if all_ticks:
    # Get timestamps from the data
    first_tick_time = pd.Timestamp(all_ticks[0].ts_init, unit="ns", tz="UTC")
    last_tick_time = pd.Timestamp(all_ticks[-1].ts_init, unit="ns", tz="UTC")
    print(f"Data range: {first_tick_time} to {last_tick_time}")

    # Set backtest range to first 2 weeks of data (as ISO strings for BacktestDataConfig)
    start_time = first_tick_time.isoformat()
    end_time = (first_tick_time + pd.Timedelta(days=14)).isoformat()
    print(f"Backtest range: {start_time} to {end_time}")

    # Preview selected data
    start_ns = all_ticks[0].ts_init
    end_ns = dt_to_unix_nanos(first_tick_time + pd.Timedelta(days=14))
    selected_quote_ticks = catalog.quote_ticks(
        instrument_ids=[EURUSD.id.value],
        start=start_ns,
        end=end_ns,
    )
    print(f"Selected ticks for backtest: {len(selected_quote_ticks)}")
    selected_quote_ticks[:2]
else:
    raise ValueError("No ticks found in catalog")

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
        start_time=start_time,
        end_time=end_time,
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
