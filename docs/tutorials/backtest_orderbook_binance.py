# %% [markdown]
# # Backtest with Order Book Depth Data (Binance)
#
# Replay Binance T_DEPTH order book deltas through `BacktestNode` and run an
# imbalance strategy that fires fill-or-kill (FOK) limit orders when one side
# of the book is much thicker than the other. The same pattern works against
# any venue's L2 delta feed.
#
# [View source on GitHub](https://github.com/nautechsystems/nautilus_trader/blob/develop/docs/tutorials/backtest_orderbook_binance.py).

# %% [markdown]
# ## Introduction
#
# Top-of-book imbalance is a microstructure signal: when the smaller resting
# side at the BBO drops well below the larger side, the book is leaning. The
# `OrderBookImbalance` strategy ships in `nautilus_trader.examples` and works
# in two stages on every order book update:
#
# - Compute `min(bid_size, ask_size) / max(bid_size, ask_size)`. Higher means
#   balanced; lower means leaning.
# - When the larger side is at least `trigger_min_size` and the ratio is below
#   `trigger_imbalance_ratio`, fire a single FOK limit order against the
#   thicker side. A trigger cooldown of `min_seconds_between_triggers`
#   prevents the strategy from re-firing on every micro-update.
#
# The strategy is intentionally simple and has no edge.
#
# ```mermaid
# flowchart LR
#     subgraph Inputs ["Data engine"]
#         S["Snap CSV (initial L2 state)"]
#         U["Update CSV (L2 deltas)"]
#     end
#
#     subgraph Engine ["BacktestEngine"]
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
#     S --> W --> B
#     U --> W
#     B --> C
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
# - Binance T_DEPTH CSVs for the day you want to replay. The bundled tutorial
#   uses BTCUSDT 2022-11-01 from
#   [data.binance.vision](https://data.binance.vision). Place them under the
#   directory in `NAUTILUS_DATA_DIR/Binance/`.

# %%
import os
import shutil
from decimal import Decimal
from pathlib import Path

import pandas as pd

from nautilus_trader.adapters.binance.loaders import BinanceOrderBookDeltaDataLoader
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
#
# Each row of `_depth_snap.csv` and `_depth_update.csv` is a single L2 level
# event. The Binance loader maps them to NautilusTrader `OrderBookDelta`
# objects with `update_type="snap"` for snapshots and `set` / `delete` for
# updates. The full update file for BTCUSDT 2022-11-01 is ~12 GB
# (~110 million rows), so the tutorial caps the read at 1,000,000 rows.

# %%
DATA_DIR = Path(os.environ.get("NAUTILUS_DATA_DIR", "~/Downloads/Data")).expanduser() / "Binance"

# %%
data_path = DATA_DIR
raw_files = [f for f in data_path.iterdir() if f.is_file()]
assert raw_files, f"Unable to find any data files in directory {data_path}"
raw_files

# %%
# Initial L2 snapshot of the book at session open.
path_snap = data_path / "BTCUSDT_T_DEPTH_2022-11-01_depth_snap.csv"
df_snap = BinanceOrderBookDeltaDataLoader.load(path_snap)
df_snap.head()

# %%
# Per-level deltas for the day; capped to 1M rows for a reasonable run time.
path_update = data_path / "BTCUSDT_T_DEPTH_2022-11-01_depth_update.csv"
nrows = 1_000_000
df_update = BinanceOrderBookDeltaDataLoader.load(path_update, nrows=nrows)
df_update.head()

# %% [markdown]
# ### Process deltas using a wrangler
#
# `OrderBookDeltaDataWrangler` tags each level event with the instrument ID
# and emits an `OrderBookDelta` ready for the engine. Sort by `ts_init` so the
# data engine sees deltas in true publication order regardless of how the snap
# and update files interleave.

# %%
BTCUSDT_BINANCE = TestInstrumentProvider.btcusdt_binance()
wrangler = OrderBookDeltaDataWrangler(BTCUSDT_BINANCE)

deltas = wrangler.process(df_snap)
deltas += wrangler.process(df_update)
deltas.sort(key=lambda x: x.ts_init)
deltas[:10]

# %% [markdown]
# ### Set up the data catalog
#
# Persist the instrument and deltas to a fresh `ParquetDataCatalog` so the
# `BacktestNode` can lazy-load by time range. Re-running the tutorial wipes
# any prior catalog at the same path.

# %%
CATALOG_PATH = Path.cwd() / "catalog"
if CATALOG_PATH.exists():
    shutil.rmtree(CATALOG_PATH)
CATALOG_PATH.mkdir()

catalog = ParquetDataCatalog(CATALOG_PATH)

# %%
catalog.write_data([BTCUSDT_BINANCE])
catalog.write_data(deltas)

# %%
catalog.instruments()

# %%
start = dt_to_unix_nanos(pd.Timestamp("2022-11-01", tz="UTC"))
end = dt_to_unix_nanos(pd.Timestamp("2022-11-04", tz="UTC"))

deltas = catalog.order_book_deltas(start=start, end=end)
print(len(deltas))
deltas[:10]

# %% [markdown]
# ## Configure the backtest
#
# `BacktestNode` ingests data from the catalog and builds a `BacktestEngine`
# per `BacktestRunConfig`. The venue book type must match the data: deltas
# carry full L2 information so we use `L2_MBP`.

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
        name="BINANCE",
        oms_type="NETTING",
        account_type="CASH",
        base_currency=None,
        starting_balances=["20 BTC", "100000 USDT"],
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
engine.trader.generate_account_report(Venue("BINANCE"))

# %% [markdown]
# ## What the run produces
#
# With one million updates the data spans roughly the first eleven minutes of
# the trading day after the initial snapshot is rebuilt. The renderer below
# uses three million updates (~25 minutes) so the panels show enough trigger
# events to be informative; the strategy fires the same way on the smaller
# default window.
#
# Across the active update window the strategy submits 47 FOK limit orders
# and accumulates a net 14 BTC short. Every trigger lands on the bid side,
# implying ask size dominated bid size for nearly every imbalance event in
# the recorded window.
#
# ![Top of book during the active window with FOK fills](./assets/backtest_orderbook_binance/panel_a_top_book.png)
#
# **Figure 1.** *BTCUSDT mid, best bid, and best ask during the FOK trigger
# window. Triangles down are short entries at the bid; the cross is the
# closing fill. The strategy is on the bid side throughout.*
#
# ![Imbalance ratio distribution](./assets/backtest_orderbook_binance/panel_b_imbalance_dist.png)
#
# **Figure 2.** *`smaller / larger` ratio across all sampled top-of-book
# snapshots, with the 0.20 trigger threshold marked. The mass left of the
# threshold is the addressable trigger region.*
#
# ![Top of book size and mid](./assets/backtest_orderbook_binance/panel_c_size_landscape.png)
#
# **Figure 3.** *Mid price (top) and best bid/ask size in BTC (bottom) across
# the active update window. Top-of-book sizes oscillate over a wide range
# while the mid drifts in a narrow band.*
#
# ![Net position trajectory](./assets/backtest_orderbook_binance/panel_d_position.png)
#
# **Figure 4.** *Cumulative signed BTC across the FOK fill sequence. Each
# marker is a fill; orange is a sell, blue is a buy. The strategy ramps into
# a -14 BTC short over 25 minutes.*

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
#     python3 docs/tutorials/assets/backtest_orderbook_binance/render_panels.py
# ```
#
# Set `NAUTILUS_DATA_DIR` to wherever your `Binance/` data directory lives.

# %% [markdown]
# ## Next steps
#
# - **Tighter trigger**. Drop `trigger_imbalance_ratio` to 0.10 to require a
#   ten-to-one lean before firing. Expect far fewer entries and lower hit
#   rate.
# - **Longer window**. Bump `nrows` to ten or twenty million to replay
#   several hours and see the strategy stress against more diverse sessions.
# - **Quote ticks instead of deltas**. Set `use_quote_ticks=True` in the
#   strategy config and feed the engine a quote-tick dataset for an L1 view
#   that costs less to source.
