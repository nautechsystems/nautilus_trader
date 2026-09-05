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
# - [NautilusTrader](https://pypi.org/project/nautilus_trader/) 2.x installed
#   (`pip install -U --pre nautilus_trader`). The `--pre` flag is required while 2.x
#   ships as `2.0.0rcN`.
# - pandas (`pip install pandas`). The wheel declares no runtime dependencies.

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
from nautilus_trader.testkit.providers import TestDataProvider
from nautilus_trader.testkit.providers import TestInstrumentProvider
from nautilus_trader.trading import EmaCrossConfig


# %% [markdown]
# ## Load the sample data
#
# The tutorial runs with no download: `TestDataProvider` ships AUD/USD quote
# ticks, read from the local `test_data/` directory in a source checkout and
# from GitHub otherwise. We take the first 20,000 to keep the run short.
#
# To replay a longer history, download FX tick data from
# [histdata.com](https://www.histdata.com/download-free-forex-historical-data/?/ascii/tick-data-quotes/)
# and extract the CSV files into `~/Downloads/Data/HISTDATA/` (or set the
# `NAUTILUS_DATA_DIR` environment variable to the parent directory containing a
# `HISTDATA` subfolder). Downloaded files look like
# `DAT_ASCII_EURUSD_T_202410.csv` (EUR/USD for October 2024). The cell below
# picks them up automatically. A full month of tick data runs to millions of
# rows, so expect the catalog write to take several minutes.

# %%
DATA_DIR = Path(os.environ.get("NAUTILUS_DATA_DIR", "~/Downloads/Data")).expanduser() / "HISTDATA"

raw_files = (
    sorted(
        f
        for f in DATA_DIR.iterdir()
        if f.is_file() and (f.suffix == ".csv" or f.name.endswith(".csv.gz"))
    )
    if DATA_DIR.is_dir()
    else []
)
raw_files

# %% [markdown]
# ## Load data into the catalog
#
# Both loaders parse vendor rows into Nautilus `QuoteTick` objects with a
# default notional size. Histdata CSV files contain
# `timestamp, bid_price, ask_price` fields; the bundled TrueFX sample contains
# `timestamp, bid, ask`.

# %%
if raw_files:
    instrument = TestInstrumentProvider.default_fx_ccy("EUR/USD")
    ticks = TestDataProvider.quotes_from_histdata_csv(instrument, raw_files[0])
else:
    instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD")
    ticks = TestDataProvider.quotes_from_truefx_csv(
        instrument,
        "truefx/audusd-ticks.csv",
        max_rows=20_000,
    )

# Vendor exports are not always monotonic; the catalog requires ascending timestamps
ticks.sort(key=lambda tick: tick.ts_init)

print(f"Loaded {len(ticks)} quote ticks for {instrument.id}")

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
catalog.write_instruments([instrument])

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
catalog.instruments()[0]

# %%
# Query quote ticks from catalog to determine the data range
all_ticks = catalog.query_quote_ticks(identifiers=[instrument.id.value])
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
        identifiers=[instrument.id.value],
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
