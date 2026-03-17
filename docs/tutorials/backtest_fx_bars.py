# %% [markdown]
# # Backtest with FX Bar Data
#
# Run an EMA cross strategy on USD/JPY bar data with rollover interest simulation
# and a probabilistic fill model. This tutorial uses bundled test data, so it
# runs without any external downloads.
#
# [View source on GitHub](https://github.com/nautechsystems/nautilus_trader/blob/develop/docs/tutorials/backtest_fx_bars.py).

# %% [markdown]
# ## Prerequisites
#
# - Python 3.12+
# - [NautilusTrader](https://pypi.org/project/nautilus_trader/) latest release installed (`pip install nautilus_trader`)

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
# ## Set up the engine

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
# Plug in a module to simulate FX rollover interest. The interest rate data
# ships with the test kit.

# %%
provider = TestDataProvider()
interest_rate_data = provider.read_csv("short-term-interest.csv")
config = FXRolloverInterestConfig(interest_rate_data)
fx_rollover_interest = FXRolloverInterestModule(config=config)

# %% [markdown]
# ## Add fill model
#
# A probabilistic fill model adds realism by controlling limit order fill rates
# and slippage. This prevents overly optimistic backtest results.

# %%
fill_model = FillModel(
    prob_fill_on_limit=0.2,
    prob_slippage=0.5,
    random_seed=42,
)

# %% [markdown]
# ## Add venue
#
# Set up a simulated FX ECN with the fill model and rollover module attached.

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
#
# Pre-process bid and ask side bar data into quote ticks using a
# `QuoteTickDataWrangler`. The engine builds internal bars from these ticks.

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
#
# Configure an `EMACross` strategy on 5-minute bars.

# %%
# Configure your strategy
config = EMACrossConfig(
    instrument_id=USDJPY_SIM.id,
    bar_type=BarType.from_str("USD/JPY.SIM-5-MINUTE-BID-INTERNAL"),
    fast_ema_period=10,
    slow_ema_period=20,
    trade_size=Decimal(1_000_000),
)

# Instantiate and add your strategy
strategy = EMACross(config=config)
engine.add_strategy(strategy=strategy)

# %% [markdown]
# ## Run the backtest
#
# The engine processes all data in timestamp order and logs a post-analysis
# report when complete.

# %%
engine.run()

# %% [markdown]
# ## Reports

# %%
engine.trader.generate_account_report(SIM)

# %%
engine.trader.generate_order_fills_report()

# %%
engine.trader.generate_positions_report()
