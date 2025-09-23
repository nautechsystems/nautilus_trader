# ---
# jupyter:
#   jupytext:
#     formats: py:percent
#     text_representation:
#       extension: .py
#       format_name: percent
#       format_version: '1.3'
#       jupytext_version: 1.17.3
#   kernelspec:
#     display_name: Python 3 (ipykernel)
#     language: python
#     name: python3
# ---

# %% [markdown]
# Note: Use the jupytext python package to be able to open this python file in jupyter as a notebook.
# Also run `jupytext-config set-default-viewer` to open jupytext python files as notebooks by default.

# %% [markdown]
# # Backtest: FX bar data
#
# Tutorial for [NautilusTrader](https://nautilustrader.io/docs/) a high-performance algorithmic trading platform and event driven backtester.
#
# [View source on GitHub](https://github.com/nautechsystems/nautilus_trader/blob/develop/docs/tutorials/backtest_fx_bars.ipynb).
#
# :::info
# We are currently working on this tutorial.
# :::

# %% [markdown]
# ## Overview
#
# This tutorial runs through how to set up a `BacktestEngine` (low-level API) for a single 'one-shot' backtest run using FX bar data.

# %% [markdown]
# ## Prerequisites
#
# - Python 3.11+ installed
# - [JupyterLab](https://jupyter.org/) or similar installed (`pip install -U jupyterlab`)
# - [NautilusTrader](https://pypi.org/project/nautilus_trader/) latest release installed (`pip install -U nautilus_trader`)

# %% [markdown]
# ## Imports
#
# We'll start with all of our imports for the remainder of this tutorial.

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
# ## Set up backtest engine

# %%
# Initialize a backtest configuration
config = BacktestEngineConfig(
    trader_id="BACKTESTER-001",
    logging=LoggingConfig(log_level="ERROR"),
    risk_engine=RiskEngineConfig(
        bypass=True,  # Example of bypassing pre-trade risk checks for backtests
    ),
)

# Build backtest engine
engine = BacktestEngine(config=config)

# %% [markdown]
# ## Add simulation module
#
# We can optionally plug in a module to simulate rollover interest. The data is available from pre-packaged test data.

# %%
provider = TestDataProvider()
interest_rate_data = provider.read_csv("short-term-interest.csv")
config = FXRolloverInterestConfig(interest_rate_data)
fx_rollover_interest = FXRolloverInterestModule(config=config)

# %% [markdown]
# ## Add fill model

# %% [markdown]
# For this backtest we'll use a simple probabilistic fill model.

# %%
fill_model = FillModel(
    prob_fill_on_limit=0.2,
    prob_fill_on_stop=0.95,
    prob_slippage=0.5,
    random_seed=42,
)

# %% [markdown]
# ## Add venue

# %% [markdown]
# For this backtest we just need a single trading venue which will be a similated FX ECN.

# %%
SIM = Venue("SIM")
engine.add_venue(
    venue=SIM,
    oms_type=OmsType.HEDGING,  # Venue will generate position IDs
    account_type=AccountType.MARGIN,
    base_currency=None,  # Multi-currency account
    starting_balances=[Money(1_000_000, USD), Money(10_000_000, JPY)],
    fill_model=fill_model,
    modules=[fx_rollover_interest],
)

# %% [markdown]
# ## Add instruments and data

# %% [markdown]
# Now we can add instruments and data. For this backtest we'll pre-process bid and ask side bar data into quotes using a `QuoteTickDataWrangler`.

# %%
# Add instruments
USDJPY_SIM = TestInstrumentProvider.default_fx_ccy("USD/JPY", SIM)
engine.add_instrument(USDJPY_SIM)

# Add data
wrangler = QuoteTickDataWrangler(instrument=USDJPY_SIM)
ticks = wrangler.process_bar_data(
    bid_data=provider.read_csv_bars("fxcm/usdjpy-m1-bid-2013.csv"),
    ask_data=provider.read_csv_bars("fxcm/usdjpy-m1-ask-2013.csv"),
)
engine.add_data(ticks)

# %% [markdown]
# ## Configure strategy

# %% [markdown]
# Next we'll configure and initialize a simple `EMACross` strategy we'll use for the backtest.

# %%
# Configure your strategy
ema_cross_config = EMACrossConfig(
    instrument_id=USDJPY_SIM.id,
    bar_type=BarType.from_str("USD/JPY.SIM-5-MINUTE-BID-INTERNAL"),
    fast_ema_period=10,
    slow_ema_period=20,
    trade_size=Decimal(1_000_000),
)

# Instantiate and add your strategy
strategy = EMACross(config=ema_cross_config)
engine.add_strategy(strategy=strategy)

# %% [markdown]
# ## Run backtest
#
# We now have everything required to run the backtest. Once the engine has completed running through all the data, a post-analysis report will be logged.

# %%
engine.run()

# %% [markdown]
# ## Generating reports
#
# Additionally, we can produce various reports to further analyze the backtest result.

# %%
engine.trader.generate_account_report(SIM)

# %%
engine.trader.generate_order_fills_report()

# %%
engine.trader.generate_positions_report()
