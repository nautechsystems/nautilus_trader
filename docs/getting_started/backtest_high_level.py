# %% [markdown]
# # Backtest (High-Level API)
#
# Use `BacktestNode` for config-driven backtesting with the Parquet data catalog.
# This is the recommended path for production workflows because the strategies,
# actors, and execution algorithms you build here carry forward to live trading
# with `LiveNode`.
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
from pathlib import Path

import pandas as pd

from nautilus_trader.backtest import BacktestNode
from nautilus_trader.config import BacktestDataConfig
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.config import BacktestRunConfig
from nautilus_trader.config import BacktestVenueConfig
from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.model import AccountType
from nautilus_trader.model import BookType
from nautilus_trader.model import Currency
from nautilus_trader.model import OmsType
from nautilus_trader.model import Quantity
from nautilus_trader.persistence import ParquetDataCatalog
from nautilus_trader.test_kit.providers import TestDataProvider
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.trading import EmaCrossConfig


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
# Histdata CSV files contain `timestamp, bid_price, ask_price` fields.
# `TestDataProvider.quotes_from_histdata_csv` parses them into Nautilus
# `QuoteTick` objects with a default notional size.

# %%
# Create a EUR/USD instrument on the SIM venue and parse the CSV into quote ticks
EURUSD = TestInstrumentProvider.default_fx_ccy("EUR/USD")
ticks = TestDataProvider.quotes_from_histdata_csv(EURUSD, raw_files[0])

# Preview: see first 2 ticks
ticks[0:2]

# %% [markdown]
# See the [Loading data](../concepts/data/) guide for more details.
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
catalog = ParquetDataCatalog(str(CATALOG_PATH))

# Write instrument to the catalog
catalog.write_instruments([EURUSD])

# Write ticks to the catalog
catalog.write_quote_ticks(ticks)

# %% [markdown]
# ## Query the catalog
#
# The catalog provides methods like `.instruments()` and `.query_quote_ticks()`
# to query stored data and determine the available time range.

# %%
# Get list of all instruments in catalog
catalog.instruments()

# %%
# See 1st instrument from catalog
instrument = catalog.instruments()[0]
instrument

# %%
# Query quote ticks from catalog to determine the data range
all_ticks = catalog.query_quote_ticks(identifiers=[EURUSD.id.value])
print(f"Total ticks in catalog: {len(all_ticks)}")

if all_ticks:
    # Get timestamps from the data
    first_tick_time = pd.Timestamp(all_ticks[0].ts_init, unit="ns", tz="UTC")
    last_tick_time = pd.Timestamp(all_ticks[-1].ts_init, unit="ns", tz="UTC")
    print(f"Data range: {first_tick_time} to {last_tick_time}")

    # Set backtest range to first 2 weeks of data (as UNIX nanoseconds)
    start_ns = all_ticks[0].ts_init
    end_ns = dt_to_unix_nanos(first_tick_time + pd.Timedelta(days=14))
    print(f"Backtest range: {first_tick_time} to {first_tick_time + pd.Timedelta(days=14)}")

    # Preview selected data
    selected_quote_ticks = catalog.query_quote_ticks(
        identifiers=[EURUSD.id.value],
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
        oms_type=OmsType.HEDGING,
        account_type=AccountType.MARGIN,
        book_type=BookType.L1_MBP,
        base_currency=Currency.from_str("USD"),
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
        data_type="QuoteTick",
        catalog_path=str(CATALOG_PATH),
        instrument_id=instrument.id,
        start_time=start_ns,
        end_time=end_ns,
    ),
]

# %% [markdown]
# ## Configure the backtest
#
# `BacktestRunConfig` centralizes venue and data configuration in one object.

# %%
config = BacktestRunConfig(
    venues=venue_configs,
    data=data_configs,
    engine=BacktestEngineConfig(),
)

# %% [markdown]
# ## Add the strategy
#
# Build the node, then attach a strategy to the run configuration. Here we add
# the built-in `EmaCross` example strategy, which subscribes to the quote ticks
# and trades the crossover of a fast and slow EMA on the mid price. To run your
# own strategy, make it importable and use `node.add_strategy_from_config()`
# with an `ImportableStrategyConfig` instead.

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

# %% [markdown]
# ## Run the backtest
#
# `BacktestNode` processes all data in timestamp order with deterministic
# execution semantics. The architectural patterns (strategies, actors, execution
# algorithms) carry forward to live trading with `LiveNode`.

# %%
results = node.run()
results
