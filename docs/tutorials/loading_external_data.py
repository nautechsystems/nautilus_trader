# %% [markdown]
# # Loading External Data
#
# This tutorial demonstrates how to load external data into the `ParquetDataCatalog`, and then use this to run a one-shot backtest using a `BacktestNode`.

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
from nautilus_trader.model import BarType
from nautilus_trader.model import QuoteTick
from nautilus_trader.persistence.catalog import ParquetDataCatalog
from nautilus_trader.persistence.wranglers import QuoteTickDataWrangler
from nautilus_trader.test_kit.providers import CSVTickDataLoader
from nautilus_trader.test_kit.providers import TestInstrumentProvider

# %%
DATA_DIR = "~/Downloads/Data/"

# %%
path = Path(DATA_DIR).expanduser() / "HISTDATA"
raw_files = [
    f for f in path.iterdir() if f.is_file() and (f.suffix == ".csv" or f.name.endswith(".csv.gz"))
]
assert raw_files, f"Unable to find any data files in directory {path}"
raw_files

# %%
# Load the first data file into a pandas DataFrame
df = CSVTickDataLoader.load(raw_files[0], index_col=0, datetime_format="%Y%m%d %H%M%S%f")
df.columns = ["bid_price", "ask_price"]

# Process quotes using a wrangler
EURUSD = TestInstrumentProvider.default_fx_ccy("EUR/USD")
wrangler = QuoteTickDataWrangler(EURUSD)

ticks = wrangler.process(df)

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
