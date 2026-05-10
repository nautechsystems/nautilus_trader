# %% [markdown]
# # Quickstart
#
# Run your first backtest in under five minutes.
#
# [View source on GitHub](https://github.com/nautechsystems/nautilus_trader/blob/develop/docs/getting_started/quickstart.py).

# %% [markdown]
# ## Prerequisites
#
# - Python 3.12+
# - `pip install nautilus_trader`

# %% [markdown]
# ## Write a strategy
#
# A strategy extends the `Strategy` base class and overrides event handlers to
# react to market data. This one trades an EMA crossover: buy when a fast
# exponential moving average crosses above a slow one, sell when it crosses below.

# %%
from decimal import Decimal

from nautilus_trader.config import StrategyConfig
from nautilus_trader.indicators import ExponentialMovingAverage
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.trading.strategy import Strategy


class EMACrossConfig(StrategyConfig, frozen=True):
    instrument_id: InstrumentId
    bar_type: BarType
    trade_size: Decimal
    fast_ema_period: int = 10
    slow_ema_period: int = 20


class EMACross(Strategy):
    def __init__(self, config: EMACrossConfig):
        super().__init__(config)
        self.fast_ema = ExponentialMovingAverage(config.fast_ema_period)
        self.slow_ema = ExponentialMovingAverage(config.slow_ema_period)

    def on_start(self):
        self.register_indicator_for_bars(self.config.bar_type, self.fast_ema)
        self.register_indicator_for_bars(self.config.bar_type, self.slow_ema)
        self.subscribe_bars(self.config.bar_type)

    def on_bar(self, bar: Bar):
        if not self.indicators_initialized():
            return

        if self.fast_ema.value >= self.slow_ema.value:
            if self.portfolio.is_flat(self.config.instrument_id):
                self.buy()
            elif self.portfolio.is_net_short(self.config.instrument_id):
                self.close_all_positions(self.config.instrument_id)
                self.buy()
        elif self.fast_ema.value < self.slow_ema.value:
            if self.portfolio.is_flat(self.config.instrument_id):
                self.sell()
            elif self.portfolio.is_net_long(self.config.instrument_id):
                self.close_all_positions(self.config.instrument_id)
                self.sell()

    def buy(self):
        instrument = self.cache.instrument(self.config.instrument_id)
        order = self.order_factory.market(
            self.config.instrument_id,
            OrderSide.BUY,
            instrument.make_qty(self.config.trade_size),
        )
        self.submit_order(order)

    def sell(self):
        instrument = self.cache.instrument(self.config.instrument_id)
        order = self.order_factory.market(
            self.config.instrument_id,
            OrderSide.SELL,
            instrument.make_qty(self.config.trade_size),
        )
        self.submit_order(order)

    def on_stop(self):
        self.close_all_positions(self.config.instrument_id)


# %% [markdown]
# `on_start` registers the two EMA indicators so the engine updates them
# automatically with each new bar. `on_bar` waits for the indicators to warm up,
# then enters or reverses a position based on the crossover signal.

# %% [markdown]
# ## Generate synthetic data
#
# To keep the quickstart self-contained, we generate 10,000 synthetic EUR/USD
# 1-minute bars using a random walk. In practice you would load real market data
# from a vendor or the Parquet data catalog.

# %%
import numpy as np
import pandas as pd

from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.persistence.wranglers import BarDataWrangler
from nautilus_trader.test_kit.providers import TestInstrumentProvider

# Create a EUR/USD instrument on the SIM venue
EURUSD = TestInstrumentProvider.default_fx_ccy("EUR/USD")

# Generate synthetic 1-minute bars (random walk around 1.10)
rng = np.random.default_rng(42)
n = 10_000
price = 1.10 + np.cumsum(rng.normal(0, 0.0002, n))
spread = np.abs(rng.normal(0, 0.0003, n))
bars_df = pd.DataFrame(
    {
        "open": price,
        "high": price + spread,
        "low": price - spread,
        "close": price + rng.normal(0, 0.00005, n),
    },
    index=pd.date_range("2024-01-01", periods=n, freq="1min", tz="UTC"),
)
bars_df["high"] = bars_df[["open", "high", "close"]].max(axis=1)
bars_df["low"] = bars_df[["open", "low", "close"]].min(axis=1)

bar_type = BarType.from_str("EUR/USD.SIM-1-MINUTE-LAST-EXTERNAL")
bars = BarDataWrangler(bar_type, EURUSD).process(bars_df)

# %% [markdown]
# `BarDataWrangler` converts a pandas DataFrame with OHLCV columns into Nautilus
# `Bar` objects. The bar type string encodes the instrument, aggregation period,
# price source, and data origin.

# %% [markdown]
# ## Configure and run the engine
#
# Create a `BacktestEngine`, add a simulated FX venue with a margin account, wire
# up the instrument, data, and strategy, then run. The engine processes all bars
# in timestamp order with deterministic execution semantics.

# %%
engine = BacktestEngine(
    config=BacktestEngineConfig(
        logging=LoggingConfig(log_level="ERROR"),
    ),
)

# Add a simulated FX venue
SIM = Venue("SIM")
engine.add_venue(
    venue=SIM,
    oms_type=OmsType.NETTING,
    account_type=AccountType.MARGIN,
    starting_balances=[Money(1_000_000, USD)],
    base_currency=USD,
    default_leverage=Decimal(1),
)

# Add instrument, data, and strategy
engine.add_instrument(EURUSD)
engine.add_data(bars)

strategy = EMACross(
    EMACrossConfig(
        instrument_id=EURUSD.id,
        bar_type=bar_type,
        trade_size=Decimal(100000),
    ),
)
engine.add_strategy(strategy)

# Run the backtest
engine.run()

# %% [markdown]
# The engine processes all 10,000 bars in timestamp order. Each bar updates the
# registered indicators, then triggers `on_bar`. The simulated exchange fills
# market orders at the current price.

# %% [markdown]
# ## Review results
#
# The engine generates reports from the completed backtest. The account report
# shows balance changes over time, the positions report lists each round-trip
# trade with its realized PnL, and the order fills report shows every execution.

# %%
engine.trader.generate_account_report(SIM)

# %%
engine.trader.generate_positions_report()

# %%
engine.trader.generate_order_fills_report()

# %% [markdown]
# ## Next steps
#
# - [Backtest (low-level API)](backtest_low_level) for direct `BacktestEngine` usage
#   with real market data and execution algorithms.
# - [Backtest (high-level API)](backtest_high_level) for config-driven backtesting
#   with `BacktestNode` and the Parquet data catalog.
# - [Tutorials](../tutorials/) for strategy pattern walkthroughs covering
#   market making, mean reversion, order book imbalance, and more.

# %%
engine.dispose()
