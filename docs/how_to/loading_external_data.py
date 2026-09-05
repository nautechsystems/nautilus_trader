# %% [markdown]
# # Loading External Data
#
# Load CSV market data into the Parquet data catalog, then run a backtest with
# `BacktestNode`. This is a common workflow when you have historical data from an
# external vendor that is not directly supported by a NautilusTrader adapter.
#
# [View source on GitHub](https://github.com/nautechsystems/nautilus_trader/blob/develop/docs/how_to/loading_external_data.py).

# %% [markdown]
# ## Prerequisites
#
# - Python 3.12+
# - [NautilusTrader](https://pypi.org/project/nautilus_trader/) 2.x installed
#   (`pip install -U --pre nautilus_trader`)
# - pandas (`pip install pandas`), needed only for the histdata path below

# %%
import os
import shutil
from pathlib import Path

from nautilus_trader.backtest import BacktestNode
from nautilus_trader.config import BacktestDataConfig
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.config import BacktestRunConfig
from nautilus_trader.config import BacktestVenueConfig
from nautilus_trader.model import AccountType
from nautilus_trader.model import Currency
from nautilus_trader.model import OmsType
from nautilus_trader.model import Quantity
from nautilus_trader.persistence import ParquetDataCatalog
from nautilus_trader.testkit.providers import TestDataProvider
from nautilus_trader.testkit.providers import TestInstrumentProvider
from nautilus_trader.trading import EmaCrossConfig


# %% [markdown]
# ## Load and wrangle the data
#
# Place CSV tick files (e.g. from [histdata.com](https://www.histdata.com/))
# into `~/Downloads/Data/HISTDATA/`. Set the `NAUTILUS_DATA_DIR` environment
# variable to the parent directory if your data lives elsewhere.
# `TestDataProvider.quotes_from_histdata_csv` converts the rows into Nautilus
# `QuoteTick` objects.
#
# Without a download, the how-to falls back to 20,000 bundled AUD/USD quote
# ticks so it still runs end to end.

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
CATALOG_PATH.mkdir(parents=True)

catalog = ParquetDataCatalog(str(CATALOG_PATH))

# %%
catalog.write_instruments([instrument])
catalog.write_quote_ticks(ticks)

# %%
# Verify instruments written to catalog
catalog.instruments()

# %%
start = ticks[0].ts_event
end = ticks[-1].ts_event + 1

ticks = catalog.query_quote_ticks(identifiers=[instrument.id.value], start=start, end=end)
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
        oms_type=OmsType.HEDGING,
        account_type=AccountType.MARGIN,
        base_currency=Currency.from_str("USD"),
        starting_balances=["1000000 USD"],
    ),
]

data_configs = [
    BacktestDataConfig(
        catalog_path=str(CATALOG_PATH),
        data_type="QuoteTick",
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
