# %% [markdown]
# # Backtest (low-level API)
#
# Tutorial for [NautilusTrader](https://nautilustrader.io/docs/latest/) a high-performance algorithmic trading platform and event-driven backtester.
#
# [View source on GitHub](https://github.com/nautechsystems/nautilus_trader/blob/develop/docs/getting_started/backtest_low_level.py).

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
# - Python 3.12+ installed.
# - [NautilusTrader](https://pypi.org/project/nautilus_trader/) latest release installed (`uv pip install nautilus_trader`).

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
# First, instantiate a data provider to read raw CSV trade tick data into a `pd.DataFrame`.
# Next, initialize the matching instrument (`ETHUSDT` spot on Binance).
# Then wrangle the data into Nautilus `TradeTick` objects to add to the `BacktestEngine`.
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
# See the [Loading External Data](https://nautilustrader.io/docs/latest/concepts/data#loading-data) guide for details on the data processing pipeline.

# %% [markdown]
# ## Initialize a backtest engine
#
# Create a `BacktestEngine`. Here we pass a `BacktestEngineConfig` with a custom `trader_id` to show the configuration pattern.
#
# See the [Configuration](https://nautilustrader.io/docs/api_reference/config) API reference for all available options.
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
# You can add multiple data types (including custom types) and backtest across multiple venues.
# :::
#

# %% [markdown]
# ## Add strategies
#
# Add the trading strategies you plan to run as part of the system.
#
# Initialize a strategy configuration, then create and add the strategy:
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
# The strategy config above includes TWAP parameters, but we still need to add the `ExecAlgorithm` component.
#
# ## Add execution algorithms
#
# Add a TWAP execution algorithm to the engine, following the same pattern as strategies.
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
# See the [BacktestEngineConfig](https://nautilustrader.io/docs/latest/api_reference/config) API reference for all available options.
#

# %%
# Run the engine (from start to end of data)
engine.run()

# %% [markdown]
# ## Post-run and analysis
#
# The engine logs a post-run tearsheet with default statistics. You can load custom statistics too; see the [Portfolio statistics](../concepts/portfolio.md#portfolio-statistics) guide.
#
# The engine retains data and execution objects in memory for generating reports.
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
#
# Instruments and data persist across resets by default, so you don't need to reload them.

# %%
# For repeated backtest runs, reset the engine
engine.reset()

# Instruments and data persist, just add new components and run again

# %% [markdown]
# Remove and add individual components (actors, strategies, execution algorithms) as required.
#
# See the [Trader](../api_reference/trading.md) API reference for a description of all methods available to achieve this.
#

# %%
# Once done, good practice to dispose of the object if the script continues
engine.dispose()
