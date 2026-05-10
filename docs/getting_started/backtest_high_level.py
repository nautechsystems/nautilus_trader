# %% [markdown]
# # Backtest (High-Level API)
#
# Use `BacktestNode` for config-driven backtesting with the Parquet data catalog.
# This is the recommended path for production workflows because the strategies,
# actors, and execution algorithms you build here carry forward to live trading
# with `TradingNode`.
#
# This tutorial loads FX quote tick data, writes it to a catalog, and backtests
# an EMA cross strategy on a simulated FX ECN venue.
#
# [View source on GitHub](https://github.com/nautechsystems/nautilus_trader/blob/develop/docs/getting_started/backtest_high_level.py).

# %% [markdown]
# ## Prerequisites
# - Python 3.12+
# - [NautilusTrader](https://pypi.org/project/nautilus_trader/) latest release installed (`pip install nautilus_trader`)

# %%
import os
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
# ## Download sample data
#
# This example uses FX tick data from [histdata.com](https://www.histdata.com/download-free-forex-historical-data/?/ascii/tick-data-quotes/).
# Select an FX pair and one or more months to download.
#
# Downloaded files look like:
#
# - `DAT_ASCII_EURUSD_T_202410.csv` (EUR/USD for October 2024)
# - `DAT_ASCII_EURUSD_T_202411.csv` (EUR/USD for November 2024)
#
# Extract the CSV files into `~/Downloads/Data/HISTDATA/` (or set the
# `NAUTILUS_DATA_DIR` environment variable to the parent directory containing a
# `HISTDATA` subfolder).

# %%
DATA_DIR = Path(os.environ.get("NAUTILUS_DATA_DIR", "~/Downloads/Data")).expanduser() / "HISTDATA"

# %%
path = DATA_DIR
raw_files = [
    f for f in path.iterdir() if f.is_file() and (f.suffix == ".csv" or f.name.endswith(".csv.gz"))
]
assert raw_files, f"Unable to find any CSV files in directory {path}"
raw_files

# %% [markdown]
# ## Load data into the catalog
#
# Histdata CSV files contain `timestamp, bid_price, ask_price` fields. Load the
# raw data into a DataFrame, then process it into Nautilus `QuoteTick` objects
# with a `QuoteTickDataWrangler`.

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
# ## Query the catalog
#
# The catalog provides methods like `.instruments()` and `.quote_ticks()` to
# query stored data and determine the available time range.

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
# ## Configure the backtest
#
# `BacktestRunConfig` centralizes venue, data, and strategy configuration in one
# object. It is Partialable, so you can build it in stages. This reduces
# boilerplate when running parameter sweeps or grid searches.

# %%
config = BacktestRunConfig(
    engine=BacktestEngineConfig(strategies=strategies),
    data=data_configs,
    venues=venue_configs,
)

# %% [markdown]
# ## Run the backtest
#
# `BacktestNode` processes all data in timestamp order with deterministic
# execution semantics. The architectural patterns (strategies, actors, execution
# algorithms) carry forward to live trading with `TradingNode`.

# %%
node = BacktestNode(configs=[config])

results = node.run()
results
