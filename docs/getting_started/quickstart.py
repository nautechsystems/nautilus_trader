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
# # Quickstart
#
# Tutorial for [NautilusTrader](https://nautilustrader.io/docs/) a high-performance algorithmic trading platform and event driven backtester.
#
# [View source on GitHub](https://github.com/nautechsystems/nautilus_trader/blob/develop/docs/getting_started/quickstart.ipynb).

# %% [markdown]
# ## Overview
#
# This quickstart tutorial shows you how to get up and running with NautilusTrader backtesting using FX data.
# To support this, we provide pre-loaded test data in the standard Nautilus persistence format (Parquet).
#

# %% [markdown]
# ## Prerequisites
# - Python 3.11+ installed.
# - [NautilusTrader](https://pypi.org/project/nautilus_trader/) latest release installed (`pip install -U nautilus_trader`).
# - [JupyterLab](https://jupyter.org/) or similar installed (`pip install -U jupyterlab`).

# %% [markdown]
# ## 1. Get sample data
#
# To save time, we have prepared sample data in the Nautilus format for use with this example.
# Run the next cell to download and set up the data (this should take ~ 1-2 mins).
#
# For further details on how to load data into Nautilus, see [Loading External Data](https://nautilustrader.io/docs/latest/concepts/data#loading-data) guide.

# %%
import os
import urllib.request
from pathlib import Path

from nautilus_trader.persistence.catalog import ParquetDataCatalog
from nautilus_trader.persistence.wranglers import QuoteTickDataWrangler
from nautilus_trader.test_kit.providers import CSVTickDataLoader
from nautilus_trader.test_kit.providers import TestInstrumentProvider


# Change to project root directory
original_cwd = os.getcwd()
project_root = os.path.abspath(os.path.join(os.getcwd(), "..", ".."))
os.chdir(project_root)

print(f"Working directory: {os.getcwd()}")

# Create catalog directory
catalog_path = Path("catalog")
catalog_path.mkdir(exist_ok=True)

print(f"Catalog directory: {catalog_path.absolute()}")

try:
    # Download EUR/USD sample data
    print("Downloading EUR/USD sample data...")
    url = "https://raw.githubusercontent.com/nautechsystems/nautilus_data/main/raw_data/fx_hist_data/DAT_ASCII_EURUSD_T_202001.csv.gz"
    filename = "EURUSD_202001.csv.gz"

    print(f"Downloading from: {url}")
    urllib.request.urlretrieve(url, filename)  # noqa: S310
    print("Download complete")

    # Create the instrument using the current schema (includes multiplier)
    print("Creating EUR/USD instrument...")
    instrument = TestInstrumentProvider.default_fx_ccy("EUR/USD")

    # Load and process the tick data
    print("Loading tick data...")
    wrangler = QuoteTickDataWrangler(instrument)

    df = CSVTickDataLoader.load(
        filename,
        index_col=0,
        datetime_format="%Y%m%d %H%M%S%f",
    )
    df.columns = ["bid_price", "ask_price", "size"]
    print(f"Loaded {len(df)} ticks")

    # Process ticks
    print("Processing ticks...")
    ticks = wrangler.process(df)

    # Write to catalog
    print("Writing data to catalog...")
    catalog = ParquetDataCatalog(str(catalog_path))

    # Write instrument first
    catalog.write_data([instrument])
    print("Instrument written to catalog")

    # Write tick data
    catalog.write_data(ticks)
    print("Tick data written to catalog")

    # Verify what was written
    print("\nVerifying catalog contents...")
    test_catalog = ParquetDataCatalog(str(catalog_path))
    loaded_instruments = test_catalog.instruments()
    print(f"Instruments in catalog: {[str(i.id) for i in loaded_instruments]}")

    # Clean up downloaded file
    os.unlink(filename)
    print("\nData setup complete!")

except Exception as e:
    print(f"Error: {e}")
    import traceback

    traceback.print_exc()
finally:
    os.chdir(original_cwd)
    print(f"Changed back to: {os.getcwd()}")

# %%
from nautilus_trader.backtest.node import BacktestDataConfig
from nautilus_trader.backtest.node import BacktestEngineConfig
from nautilus_trader.backtest.node import BacktestNode
from nautilus_trader.backtest.node import BacktestRunConfig
from nautilus_trader.backtest.node import BacktestVenueConfig
from nautilus_trader.config import ImportableStrategyConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.model import Quantity
from nautilus_trader.model import QuoteTick
from nautilus_trader.persistence.catalog import ParquetDataCatalog


# %% [markdown]
# ## 2. Set up a Parquet data catalog
#
# If everything worked correctly, you should be able to see a single EUR/USD instrument in the catalog.

# %%
# Load the catalog from the project root directory
project_root = os.path.abspath(os.path.join(os.getcwd(), "..", ".."))
catalog_path = Path(os.path.join(project_root, "catalog"))

catalog = ParquetDataCatalog(catalog_path)
instruments = catalog.instruments()

print(f"Loaded catalog from: {catalog_path}")
print(f"Available instruments: {[str(i.id) for i in instruments]}")

if instruments:
    print(f"\nUsing instrument: {instruments[0].id}")
else:
    print("\nNo instruments found. Please run the data download cell first.")

# %% [markdown]
# ## 3. Write a trading strategy
#
# NautilusTrader includes many built-in indicators. In this example we use the MACD indicator to build a simple trading strategy.
#
# You can read more about [MACD here](https://www.investopedia.com/terms/m/macd.asp); this indicator merely serves as an example without any expected alpha. You can also register indicators to receive certain data types; however, in this example we manually pass the received `QuoteTick` to the indicator in the `on_quote_tick` method.
#

# %%
from nautilus_trader.core.message import Event
from nautilus_trader.indicators import MovingAverageConvergenceDivergence
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Position
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.events import PositionClosed
from nautilus_trader.model.events import PositionOpened
from nautilus_trader.trading.strategy import Strategy
from nautilus_trader.trading.strategy import StrategyConfig


class MACDConfig(StrategyConfig):
    instrument_id: InstrumentId
    fast_period: int = 12
    slow_period: int = 26
    trade_size: int = 1_000_000


class MACDStrategy(Strategy):
    """
    A MACD-based strategy that only trades on zero-line crossovers.
    """

    def __init__(self, config: MACDConfig):
        super().__init__(config=config)
        # Our "trading signal"
        self.macd = MovingAverageConvergenceDivergence(
            fast_period=config.fast_period,
            slow_period=config.slow_period,
            price_type=PriceType.MID,
        )

        self.trade_size = Quantity.from_int(config.trade_size)

        # Track our position and MACD state
        self.position: Position | None = None
        self.last_macd_above_zero = None  # Track if MACD was above zero on last check

    def on_start(self):
        """
        Subscribe to market data on strategy start.
        """
        self.subscribe_quote_ticks(instrument_id=self.config.instrument_id)

    def on_stop(self):
        """
        Clean up on strategy stop.
        """
        self.close_all_positions(self.config.instrument_id)
        self.unsubscribe_quote_ticks(instrument_id=self.config.instrument_id)

    def on_quote_tick(self, tick: QuoteTick):
        """
        Process incoming quote ticks.
        """
        # Update indicator
        self.macd.handle_quote_tick(tick)

        if not self.macd.initialized:
            return  # Wait for indicator to warm up

        # Check for trading opportunities
        self.check_signals()

    def on_event(self, event: Event):
        """
        Handle position events.
        """
        if isinstance(event, PositionOpened):
            self.position = self.cache.position(event.position_id)
            self._log.info(f"Position opened: {self.position.side} @ {self.position.avg_px_open}")
        elif isinstance(event, PositionClosed):
            if self.position and self.position.id == event.position_id:
                self._log.info(f"Position closed with PnL: {self.position.realized_pnl}")
                self.position = None

    def check_signals(self):
        """Check MACD signals - only act on actual crossovers."""
        current_macd = self.macd.value
        current_above_zero = current_macd > 0

        # Skip if this is the first reading
        if self.last_macd_above_zero is None:
            self.last_macd_above_zero = current_above_zero
            return

        # Only act on actual crossovers
        if self.last_macd_above_zero != current_above_zero:
            if current_above_zero:  # Just crossed above zero
                # Only go long if we're not already long
                if not self.is_long:
                    # Close any short position first
                    if self.is_short:
                        self.close_position(self.position)
                    # Then go long (but only when flat)
                    self.go_long()

            else:  # Just crossed below zero
                # Only go short if we're not already short
                if not self.is_short:
                    # Close any long position first
                    if self.is_long:
                        self.close_position(self.position)
                    # Then go short (but only when flat)
                    self.go_short()

        self.last_macd_above_zero = current_above_zero

    def go_long(self):
        """
        Enter long position only if flat.
        """
        if self.is_flat:
            order = self.order_factory.market(
                instrument_id=self.config.instrument_id,
                order_side=OrderSide.BUY,
                quantity=self.trade_size,
            )
            self.submit_order(order)
            self._log.info(f"Going LONG - MACD crossed above zero: {self.macd.value:.6f}")

    def go_short(self):
        """
        Enter short position only if flat.
        """
        if self.is_flat:
            order = self.order_factory.market(
                instrument_id=self.config.instrument_id,
                order_side=OrderSide.SELL,
                quantity=self.trade_size,
            )
            self.submit_order(order)
            self._log.info(f"Going SHORT - MACD crossed below zero: {self.macd.value:.6f}")

    @property
    def is_flat(self) -> bool:
        """
        Check if we have no position.
        """
        return self.position is None

    @property
    def is_long(self) -> bool:
        """
        Check if we have a long position.
        """
        return bool(self.position and self.position.side == PositionSide.LONG)

    @property
    def is_short(self) -> bool:
        """
        Check if we have a short position.
        """
        return bool(self.position and self.position.side == PositionSide.SHORT)

    def on_dispose(self):
        """
        Clean up on strategy disposal.
        """


# %% [markdown]
# ### Enhanced Strategy with Stop-Loss and Take-Profit
#
# The basic MACD strategy above will now generate trades. For better risk management, here's an enhanced version with stop-loss and take-profit orders:

# %%
from nautilus_trader.model.objects import Price


class MACDEnhancedConfig(StrategyConfig):
    instrument_id: InstrumentId
    fast_period: int = 12
    slow_period: int = 26
    trade_size: int = 1_000_000
    entry_threshold: float = 0.00005
    exit_threshold: float = 0.00002
    stop_loss_pips: int = 20  # Stop loss in pips
    take_profit_pips: int = 40  # Take profit in pips


class MACDEnhancedStrategy(Strategy):
    """
    Enhanced MACD strategy with stop-loss and take-profit.
    """

    def __init__(self, config: MACDEnhancedConfig):
        super().__init__(config=config)
        self.macd = MovingAverageConvergenceDivergence(
            fast_period=config.fast_period,
            slow_period=config.slow_period,
            price_type=PriceType.MID,
        )

        self.trade_size = Quantity.from_int(config.trade_size)
        self.position: Position | None = None
        self.last_macd_sign = 0

    def on_start(self):
        """
        Subscribe to market data on strategy start.
        """
        self.subscribe_quote_ticks(instrument_id=self.config.instrument_id)

    def on_stop(self):
        """
        Clean up on strategy stop.
        """
        self.cancel_all_orders(self.config.instrument_id)
        self.close_all_positions(self.config.instrument_id)
        self.unsubscribe_quote_ticks(instrument_id=self.config.instrument_id)

    def on_quote_tick(self, tick: QuoteTick):
        """
        Process incoming quote ticks.
        """
        self.macd.handle_quote_tick(tick)

        if not self.macd.initialized:
            return

        self.check_signals(tick)

    def on_event(self, event: Event):
        """
        Handle position events.
        """
        if isinstance(event, PositionOpened):
            self.position = self.cache.position(event.position_id)
            self._log.info(f"Position opened: {self.position.side} @ {self.position.avg_px_open}")
            # Place stop-loss and take-profit orders
            self.place_exit_orders()
        elif isinstance(event, PositionClosed):
            if self.position and self.position.id == event.position_id:
                pnl = self.position.realized_pnl
                self._log.info(f"Position closed with PnL: {pnl}")
                self.position = None
                # Cancel any remaining exit orders
                self.cancel_all_orders(self.config.instrument_id)

    def check_signals(self, tick: QuoteTick):
        """
        Check MACD signals and manage positions.
        """
        current_macd = self.macd.value
        current_sign = 1 if current_macd > 0 else -1

        # Skip if we already have a position
        if self.position:
            return

        # Detect MACD zero-line crossover
        if self.last_macd_sign != 0 and self.last_macd_sign != current_sign:
            if current_sign > 0:
                self.go_long(tick)
            else:
                self.go_short(tick)

        # Entry signals based on threshold
        elif abs(current_macd) > self.config.entry_threshold:
            if current_macd > self.config.entry_threshold:
                self.go_long(tick)
            elif current_macd < -self.config.entry_threshold:
                self.go_short(tick)

        self.last_macd_sign = current_sign

    def go_long(self, tick: QuoteTick):
        """
        Enter long position.
        """
        if self.position:
            return  # Already have a position

        order = self.order_factory.market(
            instrument_id=self.config.instrument_id,
            order_side=OrderSide.BUY,
            quantity=self.trade_size,
        )
        self.submit_order(order)
        self._log.info(f"Going LONG @ {tick.ask_price} - MACD: {self.macd.value:.6f}")

    def go_short(self, tick: QuoteTick):
        """
        Enter short position.
        """
        if self.position:
            return  # Already have a position

        order = self.order_factory.market(
            instrument_id=self.config.instrument_id,
            order_side=OrderSide.SELL,
            quantity=self.trade_size,
        )
        self.submit_order(order)
        self._log.info(f"Going SHORT @ {tick.bid_price} - MACD: {self.macd.value:.6f}")

    def place_exit_orders(self):
        """
        Place stop-loss and take-profit orders for the current position.
        """
        if not self.position:
            return

        entry_price = float(self.position.avg_px_open)
        pip_value = 0.0001  # For FX pairs (adjust for different instruments)

        if self.position.side == PositionSide.LONG:
            # Long position: stop below entry, target above
            stop_price = entry_price - (self.config.stop_loss_pips * pip_value)
            target_price = entry_price + (self.config.take_profit_pips * pip_value)

            # Stop-loss order
            stop_loss = self.order_factory.stop_market(
                instrument_id=self.config.instrument_id,
                order_side=OrderSide.SELL,
                quantity=self.trade_size,
                trigger_price=Price.from_str(f"{stop_price:.5f}"),
            )
            self.submit_order(stop_loss)

            # Take-profit order
            take_profit = self.order_factory.limit(
                instrument_id=self.config.instrument_id,
                order_side=OrderSide.SELL,
                quantity=self.trade_size,
                price=Price.from_str(f"{target_price:.5f}"),
            )
            self.submit_order(take_profit)

            self._log.info(
                f"Placed LONG exit orders - Stop: {stop_price:.5f}, Target: {target_price:.5f}",
            )

        else:  # SHORT position
            # Short position: stop above entry, target below
            stop_price = entry_price + (self.config.stop_loss_pips * pip_value)
            target_price = entry_price - (self.config.take_profit_pips * pip_value)

            # Stop-loss order
            stop_loss = self.order_factory.stop_market(
                instrument_id=self.config.instrument_id,
                order_side=OrderSide.BUY,
                quantity=self.trade_size,
                trigger_price=Price.from_str(f"{stop_price:.5f}"),
            )
            self.submit_order(stop_loss)

            # Take-profit order
            take_profit = self.order_factory.limit(
                instrument_id=self.config.instrument_id,
                order_side=OrderSide.BUY,
                quantity=self.trade_size,
                price=Price.from_str(f"{target_price:.5f}"),
            )
            self.submit_order(take_profit)

            self._log.info(
                f"Placed SHORT exit orders - Stop: {stop_price:.5f}, Target: {target_price:.5f}",
            )

    def on_dispose(self):
        """
        Clean up on strategy disposal.
        """


# %% [markdown]
# ## Configuring backtests
#
# Now that we have a trading strategy and data, we can begin to configure a backtest run. Nautilus uses a `BacktestNode` to orchestrate backtest runs, which requires some setup. This may seem a little complex at first, however this is necessary for the capabilities that Nautilus strives for.
#
# To configure a `BacktestNode`, we first need to create an instance of a `BacktestRunConfig`, configuring the following (minimal) aspects of the backtest:
#
# - `engine`: The engine for the backtest representing our core system, which will also contain our strategies.
# - `venues`: The simulated venues (exchanges or brokers) available in the backtest.
# - `data`: The input data we would like to perform the backtest on.
#
# There are many more configurable features described later in the docs; for now this will get us up and running.
#

# %%
venue = BacktestVenueConfig(
    name="SIM",
    oms_type="NETTING",
    account_type="MARGIN",
    base_currency="USD",
    starting_balances=["1_000_000 USD"],
)

# %% [markdown]
# ## 5. Configure data
#
# We need to know about the instruments that we would like to load data for, we can use the `ParquetDataCatalog` for this.

# %%
instruments = catalog.instruments()
instruments

# %% [markdown]
# Next, configure the data for the backtest. Nautilus provides a flexible data-loading system for backtests, but that flexibility requires some configuration.
#
# For each tick type (and instrument), we add a `BacktestDataConfig`. In this instance we are simply adding the `QuoteTick`(s) for our EUR/USD instrument:
#

# %%
from nautilus_trader.model import QuoteTick


data = BacktestDataConfig(
    catalog_path=str(catalog.path),
    data_cls=QuoteTick,
    instrument_id=instruments[0].id,
    end_time="2020-01-10",
)

# %% [markdown]
# ## 6. Configure engine
#
# Create a `BacktestEngineConfig` to represent the configuration of our core trading system.
# Pass in your trading strategies, adjust the log level as needed, and configure any other components (the defaults are fine too).
#
# Add strategies via the `ImportableStrategyConfig`, which enables importing strategies from arbitrary files or user packages. In this instance our `MACDStrategy` lives in the current module, which Python refers to as `__main__`.
#

# %%
# NautilusTrader currently exceeds the rate limit for Jupyter notebook logging (stdout output),
# this is why the `log_level` is set to "ERROR". If you lower this level to see
# more logging then the notebook will hang during cell execution. A fix is currently
# being investigated which involves either raising the configured rate limits for
# Jupyter, or throttling the log flushing from Nautilus.
# https://github.com/jupyterlab/jupyterlab/issues/12845
# https://github.com/deshaw/jupyterlab-limit-output
engine_config = BacktestEngineConfig(
    strategies=[
        ImportableStrategyConfig(
            strategy_path="__main__:MACDStrategy",
            config_path="__main__:MACDConfig",
            config={
                "instrument_id": instruments[0].id,
                "fast_period": 12,
                "slow_period": 26,
            },
        ),
    ],
    logging=LoggingConfig(log_level="ERROR"),
)

# %% [markdown]
# ## 7. Run backtest
#
# We can now pass our various config pieces to the `BacktestRunConfig`. This object now contains the
# full configuration for our backtest.

# %%
config = BacktestRunConfig(
    engine=engine_config,
    venues=[venue],
    data=[data],
)

# %% [markdown]
# The `BacktestNode` class orchestrates the backtest run. This separation between configuration and execution enables the `BacktestNode` to run multiple configurations (different parameters or batches of data). We are now ready to run some backtests.
#

# %%
from nautilus_trader.backtest.results import BacktestResult


node = BacktestNode(configs=[config])

# Runs one or many configs synchronously
results: list[BacktestResult] = node.run()

# %% [markdown]
# ### Expected Output
#
# When you run the backtest with the improved MACD strategy, you should see:
# - **Actual trades being executed** (both BUY and SELL orders).
# - **Positions being opened and closed** with proper exit logic.
# - **P&L calculations** showing wins and losses.
# - **Performance metrics** including win rate, profit factor, and additional statistics.
#
# If you're not seeing any trades, check:
# 1. The data time range (you may need more data).
# 2. The threshold parameters (they might be too restrictive).
# 3. The indicator warm-up period (MACD needs time to initialize).
#

# %% [markdown]
# ## 8. Analyze results

# %% [markdown]
# Now that the run is complete, we can also directly query for the `BacktestEngine`(s) used internally by the `BacktestNode`
# by using the run configs ID.
#
# The engine(s) can provide additional reports and information.

# %%
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.model import Venue


backtest_engine: BacktestEngine = node.get_engine(config.id)

len(backtest_engine.trader.generate_order_fills_report())

# %%
backtest_engine.trader.generate_positions_report()

# %%
backtest_engine.trader.generate_account_report(Venue("SIM"))

# %% [markdown]
# ## 9. Performance Metrics
#
# Let's add some additional performance metrics to better understand how our strategy performed:

# %%
# Get performance statistics

# Get the account and positions
account = backtest_engine.trader.generate_account_report(Venue("SIM"))
positions = backtest_engine.trader.generate_positions_report()
orders = backtest_engine.trader.generate_order_fills_report()

# Print summary statistics
print("=== STRATEGY PERFORMANCE ===")
print(f"Total Orders: {len(orders)}")
print(f"Total Positions: {len(positions)}")

if len(positions) > 0:
    # Convert P&L strings to numeric values
    positions["pnl_numeric"] = positions["realized_pnl"].apply(
        lambda x: (
            float(str(x).replace(" USD", "").replace(",", "")) if isinstance(x, str) else float(x)
        ),
    )

    # Calculate win rate
    winning_trades = positions[positions["pnl_numeric"] > 0]
    losing_trades = positions[positions["pnl_numeric"] < 0]

    win_rate = len(winning_trades) / len(positions) * 100 if len(positions) > 0 else 0

    print(f"\nWin Rate: {win_rate:.1f}%")
    print(f"Winning Trades: {len(winning_trades)}")
    print(f"Losing Trades: {len(losing_trades)}")

    # Calculate returns
    total_pnl = positions["pnl_numeric"].sum()
    avg_pnl = positions["pnl_numeric"].mean()
    max_win = positions["pnl_numeric"].max()
    max_loss = positions["pnl_numeric"].min()

    print(f"\nTotal P&L: {total_pnl:.2f} USD")
    print(f"Average P&L: {avg_pnl:.2f} USD")
    print(f"Best Trade: {max_win:.2f} USD")
    print(f"Worst Trade: {max_loss:.2f} USD")

    # Calculate risk metrics if we have both wins and losses
    if len(winning_trades) > 0 and len(losing_trades) > 0:
        avg_win = winning_trades["pnl_numeric"].mean()
        avg_loss = abs(losing_trades["pnl_numeric"].mean())
        profit_factor = winning_trades["pnl_numeric"].sum() / abs(
            losing_trades["pnl_numeric"].sum(),
        )

        print(f"\nAverage Win: {avg_win:.2f} USD")
        print(f"Average Loss: {avg_loss:.2f} USD")
        print(f"Profit Factor: {profit_factor:.2f}")
        print(f"Risk/Reward Ratio: {avg_win/avg_loss:.2f}")
else:
    print("\nNo positions generated. Check strategy parameters.")

print("\n=== FINAL ACCOUNT STATE ===")
print(account.tail(1).to_string())

# %%
