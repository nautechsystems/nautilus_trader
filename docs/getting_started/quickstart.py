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
from nautilus_trader.model import Bar
from nautilus_trader.model import BarType
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import OrderSide
from nautilus_trader.trading import Strategy


class EMACrossConfig(StrategyConfig):
    _CUSTOM_FIELDS = (
        "instrument_id",
        "bar_type",
        "trade_size",
        "fast_ema_period",
        "slow_ema_period",
    )

    def __new__(cls, *args: object, **kwargs: object) -> object:
        for field in cls._CUSTOM_FIELDS:
            kwargs.pop(field, None)
        return super().__new__(cls, *args, **kwargs)

    def __init__(
        self,
        instrument_id: InstrumentId,
        bar_type: BarType,
        trade_size: Decimal,
        fast_ema_period: int = 10,
        slow_ema_period: int = 20,
        **_kwargs: object,
    ) -> None:
        super().__init__()
        self.instrument_id = instrument_id
        self.bar_type = bar_type
        self.trade_size = trade_size
        self.fast_ema_period = fast_ema_period
        self.slow_ema_period = slow_ema_period


class EMACross(Strategy):
    def __init__(self, config: EMACrossConfig) -> None:
        super().__init__(config)
        self.fast_ema = ExponentialMovingAverage(config.fast_ema_period)
        self.slow_ema = ExponentialMovingAverage(config.slow_ema_period)

    def on_start(self) -> None:
        self.register_indicator_for_bars(self.config.bar_type, self.fast_ema)
        self.register_indicator_for_bars(self.config.bar_type, self.slow_ema)
        self.subscribe_bars(self.config.bar_type)

    def on_bar(self, _bar: Bar) -> None:
        if not self.indicators_initialized():
            return

        if self.fast_ema.value >= self.slow_ema.value:
            if self.portfolio.is_net_flat(self.config.instrument_id):
                self.buy()
            elif self.portfolio.is_net_short(self.config.instrument_id):
                self.close_all_positions(self.config.instrument_id)
                self.buy()
        elif self.fast_ema.value < self.slow_ema.value:
            if self.portfolio.is_net_flat(self.config.instrument_id):
                self.sell()
            elif self.portfolio.is_net_long(self.config.instrument_id):
                self.close_all_positions(self.config.instrument_id)
                self.sell()

    def buy(self) -> None:
        instrument = self.cache.instrument(self.config.instrument_id)
        order = self.order_factory.market(
            self.config.instrument_id,
            OrderSide.BUY,
            instrument.make_qty(self.config.trade_size),
        )
        self.submit_order(order)

    def sell(self) -> None:
        instrument = self.cache.instrument(self.config.instrument_id)
        order = self.order_factory.market(
            self.config.instrument_id,
            OrderSide.SELL,
            instrument.make_qty(self.config.trade_size),
        )
        self.submit_order(order)

    def on_stop(self) -> None:
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

from nautilus_trader.backtest import BacktestEngine
from nautilus_trader.common import LogLevel
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.config import LoggerConfig
from nautilus_trader.model import AccountType
from nautilus_trader.model import Currency
from nautilus_trader.model import CurrencyPair
from nautilus_trader.model import Money
from nautilus_trader.model import OmsType
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import Symbol
from nautilus_trader.model import Venue


# Create a EUR/USD instrument on the SIM venue
EUR = Currency.from_str("EUR")
USD = Currency.from_str("USD")
EURUSD = CurrencyPair(
    instrument_id=InstrumentId.from_str("EUR/USD.SIM"),
    raw_symbol=Symbol("EUR/USD"),
    base_currency=EUR,
    quote_currency=USD,
    price_precision=5,
    size_precision=0,
    price_increment=Price.from_str("0.00001"),
    size_increment=Quantity.from_int(1),
    ts_event=0,
    ts_init=0,
    lot_size=Quantity.from_int(1_000),
    margin_init=Decimal("0.03"),
    margin_maint=Decimal("0.03"),
)

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
bars = [
    Bar(
        bar_type=bar_type,
        open=Price(row.open, precision=EURUSD.price_precision),
        high=Price(row.high, precision=EURUSD.price_precision),
        low=Price(row.low, precision=EURUSD.price_precision),
        close=Price(row.close, precision=EURUSD.price_precision),
        volume=Quantity.from_int(1_000_000),
        ts_event=int(timestamp.value),
        ts_init=int(timestamp.value),
    )
    for timestamp, row in bars_df.iterrows()
]

# %% [markdown]
# Each row becomes a `Bar` with the instrument's price precision. The bar type
# string encodes the instrument, aggregation period, price source, and data origin.

# %% [markdown]
# ## Configure and run the engine
#
# Create a `BacktestEngine`, add a simulated FX venue with a margin account, wire
# up the instrument, data, and strategy, then run. The engine processes all bars
# in timestamp order with deterministic execution semantics.

# %%
engine = BacktestEngine(
    config=BacktestEngineConfig(
        logging=LoggerConfig(stdout_level=LogLevel.ERROR),
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
engine.generate_account_report(venue=SIM)

# %%
engine.generate_positions_report()

# %%
engine.generate_order_fills_report()

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
