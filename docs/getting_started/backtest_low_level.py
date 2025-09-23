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
# # Backtest (low-level API)
#
# Tutorial for [NautilusTrader](https://nautilustrader.io/docs/) a high-performance algorithmic trading platform and event driven backtester.
#
# [View source on GitHub](https://github.com/nautechsystems/nautilus_trader/blob/develop/docs/getting_started/backtest_low_level.ipynb).

# %% [markdown]
# ## Overview
#
# This tutorial walks through how to use a `BacktestEngine` to backtest a simple EMA cross strategy
# with a TWAP execution algorithm on a simulated Binance Spot exchange using historical trade tick data.
#
# The following points will be covered:
# - Load raw data (external to Nautilus) using data loaders and wranglers.
# - Add this data to a `BacktestEngine`.
# - Add venues, strategies, and execution algorithms to a `BacktestEngine`.
# - Run backtests with a `BacktestEngine`.
# - Perform post-run analysis and repeated runs.
#

# %% [markdown]
# ## Prerequisites
# - Python 3.11+ installed.
# - [JupyterLab](https://jupyter.org/) or similar installed (`pip install -U jupyterlab`).
# - [NautilusTrader](https://pypi.org/project/nautilus_trader/) latest release installed (`pip install -U nautilus_trader`).
#

# %% [markdown]
# ## Imports
#
# We'll start with all of our imports for the remainder of this tutorial.

# %%
from decimal import Decimal

from nautilus_trader.backtest.config import BacktestEngineConfig
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.examples.algorithms.twap import TWAPExecAlgorithm
from nautilus_trader.examples.strategies.ema_cross_twap import EMACrossTWAP
from nautilus_trader.examples.strategies.ema_cross_twap import EMACrossTWAPConfig
from nautilus_trader.model import BarType
from nautilus_trader.model import Money
from nautilus_trader.model import TraderId
from nautilus_trader.model import Venue
from nautilus_trader.model.currencies import ETH
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.persistence.wranglers import TradeTickDataWrangler
from nautilus_trader.test_kit.providers import TestDataProvider
from nautilus_trader.test_kit.providers import TestInstrumentProvider


# %% [markdown]
# ## Loading data
#
# For this tutorial we use stub test data from the NautilusTrader repository (the automated test suite also uses this data to verify platform correctness).
#
# First, instantiate a data provider to read raw CSV trade tick data into memory as a `pd.DataFrame`.
# Next, initialize the instrument that matches the data (in this case the `ETHUSDT` spot cryptocurrency pair for Binance) and reuse it for the remainder of the backtest run.
#
# Then wrangle the data into a list of Nautilus `TradeTick` objects so you can add them to the `BacktestEngine`.
#

# %%
# Load stub test data
provider = TestDataProvider()
trades_df = provider.read_csv_ticks("binance/ethusdt-trades.csv")

# Initialize the instrument which matches the data
ETHUSDT_BINANCE = TestInstrumentProvider.ethusdt_binance()

# Process into Nautilus objects
wrangler = TradeTickDataWrangler(instrument=ETHUSDT_BINANCE)
ticks = wrangler.process(trades_df)

# %% [markdown]
# See the [Loading External Data](https://nautilustrader.io/docs/latest/concepts/data#loading-data) guide for a more detailed explanation of the typical data processing components and pipeline.

# %% [markdown]
# ## Initialize a backtest engine
#
# Create a backtest engine. You can call `BacktestEngine()` to instantiate an engine with the default configuration.
#
# We also initialize a `BacktestEngineConfig` (with only a custom `trader_id` specified) to illustrate the general configuration pattern.
#
# See the [Configuration](https://nautilustrader.io/docs/api_reference/config) API reference for details of all configuration options available.
#

# %%
# Configure backtest engine
config = BacktestEngineConfig(trader_id=TraderId("BACKTESTER-001"))

# Build the backtest engine
engine = BacktestEngine(config=config)

# %% [markdown]
# ## Add venues
#
# Create a venue to trade on that matches the market data you add to the engine.
#
# In this case we set up a simulated Binance Spot exchange.
#

# %%
# Add a trading venue (multiple venues possible)
BINANCE = Venue("BINANCE")
engine.add_venue(
    venue=BINANCE,
    oms_type=OmsType.NETTING,
    account_type=AccountType.CASH,  # Spot CASH account (not for perpetuals or futures)
    base_currency=None,  # Multi-currency account
    starting_balances=[Money(1_000_000.0, USDT), Money(10.0, ETH)],
)

# %% [markdown]
# ## Add data
#
# Add data to the backtest engine. Start by adding the `Instrument` object we initialized earlier to match the data.
#
# Then add the trades we wrangled earlier.
#

# %%
# Add instrument(s)
engine.add_instrument(ETHUSDT_BINANCE)

# Add data
engine.add_data(ticks)

# %% [markdown]
# :::note
# Machine resources and your imagination limit the amount and variety of data types you can use (custom types are possible).
# You can also backtest across multiple venues, again constrained only by machine resources.
# :::
#

# %% [markdown]
# ## Add strategies
#
# Add the trading strategies you plan to run as part of the system.
#
# :::note
# You can backtest multiple strategies and instruments; machine resources remain the only limit.
# :::
#
# First, initialize a strategy configuration, then use it to initialize a strategy that you can add to the engine:
#

# %%
# Configure your strategy
strategy_config = EMACrossTWAPConfig(
    instrument_id=ETHUSDT_BINANCE.id,
    bar_type=BarType.from_str("ETHUSDT.BINANCE-250-TICK-LAST-INTERNAL"),
    trade_size=Decimal("0.10"),
    fast_ema_period=10,
    slow_ema_period=20,
    twap_horizon_secs=10.0,
    twap_interval_secs=2.5,
)

# Instantiate and add your strategy
strategy = EMACrossTWAP(config=strategy_config)
engine.add_strategy(strategy=strategy)

# %% [markdown]
# You may notice that this strategy config includes parameters related to a TWAP execution algorithm.
# We can flexibly use different parameters per order submit, but we still need to initialize and add the actual `ExecAlgorithm` component that executes the algorithm.
#
# ## Add execution algorithms
#
# NautilusTrader enables you to build complex systems of custom components. Here we highlight one built-in component: a TWAP execution algorithm. Configure it and add it to the engine using the same general pattern as for strategies.
#
# :::note
# You can backtest multiple execution algorithms; machine resources remain the only limit.
# :::
#

# %%
# Instantiate and add your execution algorithm
exec_algorithm = TWAPExecAlgorithm()  # Using defaults
engine.add_exec_algorithm(exec_algorithm)

# %% [markdown]
# ## Run backtest
#
# After configuring the data, venues, and trading system, run a backtest.
# Call the `.run(...)` method to process all available data by default.
#
# See the [BacktestEngineConfig](https://nautilustrader.io/docs/latest/api_reference/config) API reference for a complete description of all available methods and options.
#

# %%
# Run the engine (from start to end of data)
engine.run()

# %% [markdown]
# ## Post-run and analysis
#
# After the backtest completes, the engine automatically logs a post-run tearsheet with default statistics (or custom statistics that you load; see the advanced [Portfolio statistics](../concepts/portfolio.md#portfolio-statistics) guide).
#
# The engine also keeps many data and execution objects in memory, which you can use to generate additional reports for performance analysis.
#

# %%
engine.trader.generate_account_report(BINANCE)

# %%
engine.trader.generate_order_fills_report()

# %%
engine.trader.generate_positions_report()

# %% [markdown]
# ## Repeated runs
#
# You can reset the engine for repeated runs with different strategy and component configurations.
# Call the `.reset(...)` method to retain all loaded data and components while resetting other stateful values, as if you had a fresh `BacktestEngine`. This avoids loading the same data again.
#

# %%
# For repeated backtest runs make sure to reset the engine
engine.reset()

# %% [markdown]
# Remove and add individual components (actors, strategies, execution algorithms) as required.
#
# See the [Trader](../api_reference/trading.md) API reference for a description of all methods available to achieve this.
#

# %%
# Once done, good practice to dispose of the object if the script continues
engine.dispose()
