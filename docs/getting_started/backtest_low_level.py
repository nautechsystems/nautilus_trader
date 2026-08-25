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

from nautilus_trader.backtest import BacktestEngine
from nautilus_trader.common import LogLevel
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.config import ExecutionAlgorithmConfig
from nautilus_trader.config import LoggerConfig
from nautilus_trader.config import StrategyConfig
from nautilus_trader.indicators import ExponentialMovingAverage
from nautilus_trader.model import AccountType
from nautilus_trader.model import Bar
from nautilus_trader.model import BarType
from nautilus_trader.model import Currency
from nautilus_trader.model import ExecAlgorithmId
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Money
from nautilus_trader.model import OmsType
from nautilus_trader.model import OrderSide
from nautilus_trader.model import TraderId
from nautilus_trader.model import Venue
from nautilus_trader.testkit.providers import TestDataProvider
from nautilus_trader.testkit.providers import TestInstrumentProvider
from nautilus_trader.trading import Strategy


# %% [markdown]
# ## Load data
#
# Load bundled test data (ETHUSDT trades from Binance), initialize the matching
# instrument, and build Nautilus `TradeTick` objects from the CSV.

# %%
# Initialize the instrument which matches the data
ETHUSDT_BINANCE = TestInstrumentProvider.ethusdt_binance()

# Build Nautilus trade ticks from the bundled Binance CSV
ticks = TestDataProvider.trades_from_binance_csv(
    ETHUSDT_BINANCE,
    "binance/ethusdt-trades.csv",
)

# %% [markdown]
# See the [Data](../concepts/data/) concept guide for details on the data processing pipeline.

# %% [markdown]
# ## Initialize the engine
#
# Pass a `BacktestEngineConfig` to configure the engine. Here we set a custom
# `trader_id` to show the pattern.

# %%
# Configure backtest engine
config = BacktestEngineConfig(
    trader_id=TraderId("BACKTESTER-001"),
    logging=LoggerConfig(stdout_level=LogLevel.ERROR),
)

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
    starting_balances=[
        Money(1_000_000.0, Currency.from_str("USDT")),
        Money(10.0, Currency.from_str("ETH")),
    ],
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
# The strategy extends `Strategy` and trades an EMA crossover on 250-tick bars,
# which the engine aggregates internally from the trade ticks. Entries are
# submitted with an `exec_algorithm_id` so the engine routes them to the TWAP
# execution algorithm for slicing.


# %%
class EMACrossTWAPConfig(StrategyConfig):
    _CUSTOM_FIELDS = (
        "instrument_id",
        "bar_type",
        "trade_size",
        "fast_ema_period",
        "slow_ema_period",
        "twap_horizon_secs",
        "twap_interval_secs",
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
        twap_horizon_secs: float = 10.0,
        twap_interval_secs: float = 2.5,
        **_kwargs: object,
    ) -> None:
        super().__init__()
        self.instrument_id = instrument_id
        self.bar_type = bar_type
        self.trade_size = trade_size
        self.fast_ema_period = fast_ema_period
        self.slow_ema_period = slow_ema_period
        self.twap_horizon_secs = twap_horizon_secs
        self.twap_interval_secs = twap_interval_secs


class EMACrossTWAP(Strategy):
    def __init__(self, config: EMACrossTWAPConfig) -> None:
        super().__init__(config)
        self.fast_ema = ExponentialMovingAverage(config.fast_ema_period)
        self.slow_ema = ExponentialMovingAverage(config.slow_ema_period)
        self.exec_algorithm_id = ExecAlgorithmId("TWAP")
        self.exec_algorithm_params = {
            "horizon_secs": str(config.twap_horizon_secs),
            "interval_secs": str(config.twap_interval_secs),
        }

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
        self.submit_twap_order(OrderSide.BUY)

    def sell(self) -> None:
        self.submit_twap_order(OrderSide.SELL)

    def submit_twap_order(self, side: OrderSide) -> None:
        instrument = self.cache.instrument(self.config.instrument_id)
        order = self.order_factory.market(
            self.config.instrument_id,
            side,
            instrument.make_qty(self.config.trade_size),
            exec_algorithm_id=self.exec_algorithm_id,
            exec_algorithm_params=self.exec_algorithm_params,
        )
        self.submit_order(order)

    def on_stop(self) -> None:
        self.close_all_positions(self.config.instrument_id)


# %%
# Configure and add the strategy
strategy_config = EMACrossTWAPConfig(
    instrument_id=ETHUSDT_BINANCE.id,
    bar_type=BarType.from_str("ETHUSDT.BINANCE-250-TICK-LAST-INTERNAL"),
    trade_size=Decimal("0.10"),
    fast_ema_period=10,
    slow_ema_period=20,
    twap_horizon_secs=10.0,
    twap_interval_secs=2.5,
)

strategy = EMACrossTWAP(config=strategy_config)
engine.add_strategy(strategy=strategy)

# %% [markdown]
# The strategy config carries the TWAP parameters, but the execution algorithm
# itself is a separate component.
#
# ## Add execution algorithms
#
# Register the built-in TWAP execution algorithm under the `TWAP` identifier the
# strategy references.

# %%
# Add the native TWAP execution algorithm
engine.add_native_exec_algorithm(
    "TwapAlgorithm",
    ExecutionAlgorithmConfig(exec_algorithm_id=ExecAlgorithmId("TWAP")),
)

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
engine.generate_account_report(BINANCE)

# %%
engine.generate_order_fills_report()

# %%
engine.generate_positions_report()

# %% [markdown]
# ## Repeated runs
#
# Reset the engine for repeated runs. Instruments, data, and loaded components
# persist across resets; loaded components have their internal state reset.

# %%
# For repeated backtest runs, reset the engine
engine.reset()

# Clear or remove loaded components before adding replacements.

# %% [markdown]
# Remove and add individual components (actors, strategies, execution algorithms) as required.
#
# See the [BacktestEngine](../api_reference/backtest.md) API reference for the add and clear methods.
#

# %%
# Once done, good practice to dispose of the object if the script continues
engine.dispose()
