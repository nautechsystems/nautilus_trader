# %% [markdown]
# # Backtest with Order Book Depth Data (Bybit)
#
# Replay Bybit `ob500` order book deltas through `BacktestNode` and run the
# `OrderBookImbalance` strategy. Same shape as the
# [Binance variant](https://github.com/nautechsystems/nautilus_trader/blob/develop/docs/tutorials/backtest_orderbook_binance.py),
# different loader and different instrument.
#
# [View source on GitHub](https://github.com/nautechsystems/nautilus_trader/blob/develop/docs/tutorials/backtest_orderbook_bybit.py).

# %% [markdown]
# ## Introduction
#
# Bybit publishes a single per-symbol L2 deltas archive at depth 500. The
# `BybitOrderBookDeltaDataLoader` reads the daily ZIP directly into a
# DataFrame. The strategy is the same `OrderBookImbalance` as in the Binance
# tutorial: when the smaller side of the BBO drops below
# `trigger_imbalance_ratio` of the larger, fire a single FOK limit order on
# the thicker side.
#
# `OrderBookImbalance` is a teaching strategy and has no edge.
#
# ```mermaid
# flowchart LR
#     subgraph Inputs ["Data engine"]
#         Z["ob500 ZIP archive"]
#     end
#
#     subgraph Engine ["BacktestEngine"]
#         L["BybitOrderBookDeltaDataLoader"]
#         W["OrderBookDeltaDataWrangler"]
#         B["Per-instrument OrderBook"]
#         C["Cache.order_book"]
#     end
#
#     subgraph Strategy ["OrderBookImbalance"]
#         R{{"larger >= trigger_min_size<br/>AND smaller/larger < ratio<br/>AND cooldown elapsed"}}
#         D{{"bid_size > ask_size?"}}
#         BUY["Submit FOK BUY at best ask"]
#         SELL["Submit FOK SELL at best bid"]
#     end
#
#     Z --> L --> W --> B --> C
#     C --> R
#     R -->|yes| D
#     D -->|yes| BUY
#     D -->|no| SELL
# ```

# %% [markdown]
# ## Prerequisites
#
# - Python 3.12+
# - [NautilusTrader](https://pypi.org/project/nautilus_trader/) installed
#   (`pip install nautilus_trader`)
# - A daily Bybit `ob500` ZIP, e.g.
#   `2024-12-01_XRPUSDT_ob500.data.zip` from
#   [public.bybit.com](https://public.bybit.com).

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
DATA_DIR = Path(os.environ.get("NAUTILUS_DATA_DIR", "~/Downloads/Data")).expanduser() / "Bybit"

# %%
data_path = DATA_DIR
raw_files = [f for f in data_path.iterdir() if f.is_file()]
assert raw_files, f"Unable to find any data files in directory {data_path}"
raw_files

# %%
# Read the first 1M deltas; the full file is larger.
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
deltas.sort(key=lambda x: x.ts_init)
deltas[:10]

# %% [markdown]
# ### Set up the data catalog

# %%
CATALOG_PATH = Path.cwd() / "catalog"
if CATALOG_PATH.exists():
    shutil.rmtree(CATALOG_PATH)
CATALOG_PATH.mkdir()

catalog = ParquetDataCatalog(CATALOG_PATH)

# %%
catalog.write_data([XRPUSDT_BYBIT])
catalog.write_data(deltas)

# %%
catalog.instruments()

# %%
start = dt_to_unix_nanos(pd.Timestamp("2022-11-01", tz="UTC"))
end = dt_to_unix_nanos(pd.Timestamp("2024-12-04", tz="UTC"))

deltas = catalog.order_book_deltas(start=start, end=end)
print(len(deltas))
deltas[:10]

# %% [markdown]
# ## Configure the backtest

# %%
instrument = catalog.instruments()[0]
book_type = "L2_MBP"

data_configs = [
    BacktestDataConfig(
        catalog_path=str(CATALOG_PATH),
        data_cls=OrderBookDelta,
        instrument_id=instrument.id,
    ),
]

venues_configs = [
    BacktestVenueConfig(
        name="BYBIT",
        oms_type="NETTING",
        account_type="MARGIN",
        base_currency=None,
        starting_balances=["200000 XRP", "100000 USDT"],
        book_type=book_type,
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

# %% [markdown]
# ## What the run produces
#
# The Bybit `ob500` archive sometimes starts a minute before the file's
# nominal date, so the first trades land just before midnight UTC and the
# rest inside the file's day. With a 1M delta cap, the active window is
# roughly the first minute. The strategy fires 43 FOK orders during that
# window.
#
# ![Top of book during the active minute with FOK fills](./assets/backtest_orderbook_bybit/panel_a_top_book.png)
#
# **Figure 1.** *XRPUSDT mid, best bid, and best ask during the trigger
# window. Triangles are entries (up = long, down = short), crosses are
# closing fills.*
#
# ![Imbalance ratio distribution](./assets/backtest_orderbook_bybit/panel_b_imbalance_dist.png)
#
# **Figure 2.** *`smaller / larger` BBO size ratio across all sampled
# top-of-book snapshots, with the 0.20 trigger threshold marked.*
#
# ![Top of book size and mid](./assets/backtest_orderbook_bybit/panel_c_size_landscape.png)
#
# **Figure 3.** *Mid price (top) and best bid/ask size in XRP (bottom)
# across the active window.*
#
# ![Net XRP position trajectory](./assets/backtest_orderbook_bybit/panel_d_position.png)
#
# **Figure 4.** *Cumulative signed XRP position across the FOK fill
# sequence. Each marker is a fill: blue is a buy, orange is a sell.*

# %% [markdown]
# ### Regenerate the panels
#
# A self-contained renderer re-runs the backtest with a sampling actor that
# captures top of book once per second, then writes PNG panels to the asset
# directory using the shared `nautilus_dark` tearsheet theme.
#
# ```bash
# uv sync --extra visualization
# NAUTILUS_DATA_DIR=tests/test_data/local \
#     python3 docs/tutorials/assets/backtest_orderbook_bybit/render_panels.py
# ```

# %% [markdown]
# ## Next steps
#
# - **Tighter trigger**. Drop `trigger_imbalance_ratio` to 0.10 to require a
#   ten-to-one lean.
# - **Longer window**. Bump `nrows` to ten or twenty million for a multi-hour
#   replay.
# - **Cross-venue replay**. Run the same strategy in two engines (one Bybit,
#   one Binance) and compare imbalance distributions.
