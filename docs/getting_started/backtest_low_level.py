# %% [markdown]
# # Backtest (Low-Level API)
#
# Use `BacktestEngine` for direct component access: load market data, wire up
# strategies and execution algorithms, and run backtests with full control over
# every step. This tutorial backtests an EMA cross strategy with a TWAP execution
# algorithm on a simulated Binance Spot exchange using historical trade tick data.
#
# [View source on GitHub](https://github.com/nautechsystems/nautilus_trader/blob/develop/docs/getting_started/backtest_low_level.py).

# %% [markdown]
# ## Prerequisites
# - Python 3.12+
# - [NautilusTrader](https://pypi.org/project/nautilus_trader/) latest release installed (`pip install nautilus_trader`)

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
# ## Load data
#
# Load bundled test data (ETHUSDT trades from Binance), initialize the matching
# instrument, and wrangle the raw CSV into Nautilus `TradeTick` objects.

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
# See the [Data](../concepts/data.md) concept guide for details on the data processing pipeline.

# %% [markdown]
# ## Initialize the engine
#
# Pass a `BacktestEngineConfig` to configure the engine. Here we set a custom
# `trader_id` to show the pattern.

# %%
# Configure backtest engine
config = BacktestEngineConfig(trader_id=TraderId("BACKTESTER-001"))

# Build the backtest engine
engine = BacktestEngine(config=config)

# %% [markdown]
# ## Add a venue
#
# Set up a simulated venue that matches the market data. Here we configure a
# Binance Spot exchange with a cash account.

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
# Add the instrument and trade ticks to the engine.

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
# Configure and add an EMA cross strategy with TWAP execution parameters.

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
# The strategy config references TWAP parameters, but the execution algorithm
# itself is a separate component.
#
# ## Add execution algorithms
#
# Add a TWAP execution algorithm to the engine.

# %%
# Instantiate and add your execution algorithm
exec_algorithm = TWAPExecAlgorithm()  # Using defaults
engine.add_exec_algorithm(exec_algorithm)

# %% [markdown]
# ## Run the backtest
#
# Call `.run()` to process all available data. The engine replays events in
# timestamp order with deterministic execution semantics.

# %%
# Run the engine (from start to end of data)
engine.run()

# %% [markdown]
# ## Post-run analysis
#
# The engine retains data and execution objects in memory for generating reports.
# It also logs a tearsheet with default statistics; see the
# [Portfolio statistics](../concepts/portfolio.md#portfolio-statistics) guide for
# custom statistics.

# %%
engine.trader.generate_account_report(BINANCE)

# %%
engine.trader.generate_order_fills_report()

# %%
engine.trader.generate_positions_report()

# %% [markdown]
# ## Repeated runs
#
# Reset the engine for repeated runs with different configurations. Instruments
# and data persist across resets, so you only need to add new components.

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
