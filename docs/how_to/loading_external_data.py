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
from nautilus_trader.model import BarType
from nautilus_trader.model import QuoteTick
from nautilus_trader.persistence.catalog import ParquetDataCatalog
from nautilus_trader.persistence.wranglers import QuoteTickDataWrangler
from nautilus_trader.test_kit.providers import CSVTickDataLoader
from nautilus_trader.test_kit.providers import TestInstrumentProvider

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

ticks = catalog.quote_ticks(instrument_ids=[EURUSD.id.value], start=start, end=end)
ticks[:10]

# %% [markdown]
# ## Configure and run the backtest
#
# Set up venue, data, and strategy configs, then run through `BacktestNode`.
# The strategies and actors you build here carry forward to live trading
# with `TradingNode`.

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

strategies = [
    ImportableStrategyConfig(
        strategy_path="nautilus_trader.examples.strategies.ema_cross:EMACross",
        config_path="nautilus_trader.examples.strategies.ema_cross:EMACrossConfig",
        config={
            "instrument_id": instrument.id,
            "bar_type": BarType.from_str(f"{instrument.id.value}-15-MINUTE-BID-INTERNAL"),
            "fast_ema_period": 10,
            "slow_ema_period": 20,
            "trade_size": Decimal(1_000_000),
        },
    ),
]

config = BacktestRunConfig(
    engine=BacktestEngineConfig(strategies=strategies),
    data=data_configs,
    venues=venue_configs,
)

# %%
node = BacktestNode(configs=[config])

[result] = node.run()

# %%
result
