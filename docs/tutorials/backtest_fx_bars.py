# %% [markdown]
# # Backtest with FX Bar Data
#
# Run an EMA cross strategy on USD/JPY 1-minute bid/ask bars with FX rollover
# interest and a probabilistic fill model. The data ships with the
# NautilusTrader test kit, so this tutorial runs without any external download.
#
# [View source on GitHub](https://github.com/nautechsystems/nautilus_trader/blob/develop/docs/tutorials/backtest_fx_bars.py).

# %% [markdown]
# ## Introduction
#
# The strategy is `EMACross`, a teaching example that compares a fast EMA
# against a slow EMA on bar closes:
#
# - **Fast EMA crosses above slow EMA**: any short position is closed and a new
#   long is opened.
# - **Fast EMA crosses below slow EMA**: any long position is closed and a new
#   short is opened.
#
# The venue is a simulated FX ECN with a `MARGIN` account, `HEDGING` OMS, and
# multi-currency starting balances of 1,000,000 USD and 10,000,000 JPY. A
# `FillModel` introduces a 50% probability of one-tick slippage, and the
# `FXRolloverInterestModule` applies daily rollover at the relevant short-term
# interest differential.
#
# `EMACross` is a teaching strategy and has no edge.
#
# ```mermaid
# flowchart LR
#     subgraph Inputs ["Data streams"]
#         B["1-minute BID bar (FXCM)"]
#         A["1-minute ASK bar (FXCM)"]
#     end
#
#     subgraph Wrangler ["QuoteTickDataWrangler"]
#         Q["QuoteTick stream"]
#     end
#
#     subgraph Engine ["Backtest engine"]
#         AGG["5-minute BID INTERNAL aggregator"]
#         BAR["Bar close"]
#         F1(("EMA(10)"))
#         F2(("EMA(20)"))
#     end
#
#     subgraph Decision ["Crossover decision"]
#         X{{"fast >= slow"}}
#         Y{{"fast < slow"}}
#     end
#
#     subgraph Orders ["Orders"]
#         L["Close shorts -> BUY market"]
#         S["Close longs  -> SELL market"]
#     end
#
#     B --> Q
#     A --> Q
#     Q --> AGG --> BAR
#     BAR --> F1 --> X
#     BAR --> F2 --> X
#     F1 --> Y
#     F2 --> Y
#     X -->|cross up| L
#     Y -->|cross down| S
# ```

# %% [markdown]
# ## Prerequisites
#
# - Python 3.12+
# - [NautilusTrader](https://pypi.org/project/nautilus_trader/) installed
#   (`pip install nautilus_trader`). The `visualization` extra is only needed
#   if you also want to regenerate the panels at the end of the tutorial.

# %%
from decimal import Decimal

from nautilus_trader.backtest.config import BacktestEngineConfig
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.backtest.modules import FXRolloverInterestConfig
from nautilus_trader.backtest.modules import FXRolloverInterestModule
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import RiskEngineConfig
from nautilus_trader.examples.strategies.ema_cross import EMACross
from nautilus_trader.examples.strategies.ema_cross import EMACrossConfig
from nautilus_trader.model import BarType
from nautilus_trader.model import Money
from nautilus_trader.model import Venue
from nautilus_trader.model.currencies import JPY
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.persistence.wranglers import QuoteTickDataWrangler
from nautilus_trader.test_kit.providers import TestDataProvider
from nautilus_trader.test_kit.providers import TestInstrumentProvider

# %% [markdown]
# ## Engine setup
#
# Pre-trade risk checks are bypassed so the strategy's market orders flow
# straight through to the matching engine.

# %%
config = BacktestEngineConfig(
    trader_id="BACKTESTER-001",
    logging=LoggingConfig(log_level="ERROR"),
    risk_engine=RiskEngineConfig(bypass=True),
)
engine = BacktestEngine(config=config)

# %% [markdown]
# ## Simulation modules
#
# `FXRolloverInterestModule` charges or credits rollover interest on open
# positions at the configured cutover time, using the bundled
# `short-term-interest.csv` rates from the OECD short-term interest series.
# Without it a backtest spanning many sessions ignores carry.

# %%
provider = TestDataProvider()
rollover_config = FXRolloverInterestConfig(provider.read_csv("short-term-interest.csv"))
fx_rollover_interest = FXRolloverInterestModule(config=rollover_config)

# %% [markdown]
# ## Fill model
#
# Limit orders fill on a 20% probability per tick when their price is reached,
# and any market or marketable order draws a one-tick slip on a 50% coin flip.
# The seed makes the run reproducible.

# %%
fill_model = FillModel(
    prob_fill_on_limit=0.2,
    prob_slippage=0.5,
    random_seed=42,
)

# %% [markdown]
# ## Venue
#
# `OmsType.HEDGING` lets the strategy carry concurrent long and short positions
# in the same instrument and have the venue assign position IDs. The account is
# multi-currency so PnL on USD/JPY accrues in JPY rather than being converted
# on every fill.

# %%
SIM = Venue("SIM")
engine.add_venue(
    venue=SIM,
    oms_type=OmsType.HEDGING,
    account_type=AccountType.MARGIN,
    base_currency=None,
    starting_balances=[Money(1_000_000, USD), Money(10_000_000, JPY)],
    fill_model=fill_model,
    modules=[fx_rollover_interest],
)

# %% [markdown]
# ## Instrument and data
#
# `QuoteTickDataWrangler.process_bar_data` synthesises one quote tick at the
# open and one at the close of each minute bar from the bundled FXCM bid and
# ask CSVs, giving the engine a quote tick stream ahead of bar aggregation.
# The strategy declares `5-MINUTE-BID-INTERNAL`, so the engine builds 5-minute
# BID bars from the quote stream internally.

# %%
USDJPY_SIM = TestInstrumentProvider.default_fx_ccy("USD/JPY", SIM)
engine.add_instrument(USDJPY_SIM)

wrangler = QuoteTickDataWrangler(instrument=USDJPY_SIM)
ticks = wrangler.process_bar_data(
    bid_data=provider.read_csv_bars("fxcm/usdjpy-m1-bid-2013.csv"),
    ask_data=provider.read_csv_bars("fxcm/usdjpy-m1-ask-2013.csv"),
)
engine.add_data(ticks)

# %% [markdown]
# ## Strategy
#
# Trade size is one million USD per order. EMACross cancels and replaces the
# position on every crossover, so the strategy is in some position for nearly
# the whole month.

# %%
strategy_config = EMACrossConfig(
    instrument_id=USDJPY_SIM.id,
    bar_type=BarType.from_str("USD/JPY.SIM-5-MINUTE-BID-INTERNAL"),
    fast_ema_period=10,
    slow_ema_period=20,
    trade_size=Decimal(1_000_000),
)
strategy = EMACross(config=strategy_config)
engine.add_strategy(strategy=strategy)

# %% [markdown]
# ## Run
#
# The engine processes every quote tick and bar in timestamp order, then
# returns when the data is exhausted.

# %%
engine.run()

# %% [markdown]
# ## Reports
#
# `engine.trader.generate_*` returns DataFrames covering the account state, the
# fills, and the closed positions.

# %%
engine.trader.generate_account_report(SIM)

# %%
engine.trader.generate_order_fills_report()

# %%
engine.trader.generate_positions_report()

# %% [markdown]
# ## What the run produces
#
# A 28-day run prints 8,065 5-minute bars and triggers 234 closed cycles
# across 468 fills (every crossover after the first emits a closing fill on
# the previous position and an opening fill on the new one). 72 of the 234
# cycles are profitable. The strategy ends down 209,000 JPY: a textbook
# whipsaw signature on a noisy 5-minute series.
#
# ![USD/JPY 5-minute close with EMAs across the month](./assets/backtest_fx_bars/panel_a_price_overview.png)
#
# **Figure 1.** *USD/JPY BID close at 5-minute resolution across 2013-02 with
# EMA(10) and EMA(20) overlaid. Long flat patches are weekend gaps in the FXCM
# bid feed.*
#
# ![Three-day zoom on crossovers](./assets/backtest_fx_bars/panel_b_zoom.png)
#
# **Figure 2.** *Zoom on 2013-02-12 to 2013-02-15 UTC. Each marker is a
# crossover entry: triangles up are long, triangles down are short.*
#
# ![Cumulative realised pnl](./assets/backtest_fx_bars/panel_c_pnl_curve.png)
#
# **Figure 3.** *Cumulative JPY pnl across all closed cycles. Marker color
# encodes per-cycle pnl: blue = positive, red = negative.*
#
# ![Hold-time and pnl distributions](./assets/backtest_fx_bars/panel_d_distributions.png)
#
# **Figure 4.** *Cycle hold time and per-cycle pnl distributions. Most cycles
# hold for under three hours; the pnl distribution is roughly symmetric and
# heavily concentrated near zero.*

# %% [markdown]
# ### Regenerate the panels
#
# The panels above are produced by a self-contained renderer that re-runs the
# backtest, pulls bars and fills from the engine cache, and writes PNGs using
# the shared `nautilus_dark` tearsheet theme.
#
# ```bash
# uv sync --extra visualization
# python3 docs/tutorials/assets/backtest_fx_bars/render_panels.py
# ```

# %% [markdown]
# ## Next steps
#
# - **Slow the signal**. The default 10/20 EMAs whip in low-trend sessions.
#   Try 20/60 on the same bars or move to 15-minute bars to cut the cycle
#   count.
# - **Add a regime filter**. Suppress entries when realised range is below
#   a threshold so the strategy only trades sessions with directional movement.
# - **Compare aggregations**. Build the bars from raw tick data via
#   `BarType.from_str("USD/JPY.SIM-5-MINUTE-BID-INTERNAL")` against an
#   externally aggregated dataset to confirm both paths agree.
