# %% [markdown]
# # Quickstart
#
# Tutorial for [NautilusTrader](https://nautilustrader.io/docs/latest/) a high-performance algorithmic trading platform and event-driven backtester.
#
# [View source on GitHub](https://github.com/nautechsystems/nautilus_trader/blob/develop/docs/getting_started/quickstart.py).

# %% [markdown]
# ## Overview
#
# This quickstart walks through backtesting with NautilusTrader using FX data.
# Pre-loaded test data ships in Nautilus Parquet format.
#

# %% [markdown]
# ## Prerequisites
# - Python 3.12+ installed.
# - [NautilusTrader](https://pypi.org/project/nautilus_trader/) latest release installed (`uv pip install nautilus_trader`).

# %% [markdown]
# ## 1. Get sample data
#
# To save time, we have prepared sample data in the Nautilus format for use with this example.
# Run the next cell to download and set up the data (this takes 1-2 minutes).
#
# For details on loading data into Nautilus, see the [Loading External Data](https://nautilustrader.io/docs/latest/concepts/data#loading-data) guide.

# %%
import os
import urllib.request
from pathlib import Path

from nautilus_trader.persistence.catalog import ParquetDataCatalog
from nautilus_trader.persistence.wranglers import QuoteTickDataWrangler
from nautilus_trader.test_kit.providers import CSVTickDataLoader
from nautilus_trader.test_kit.providers import TestInstrumentProvider


# Create catalog directory in current working directory
catalog_path = Path.cwd() / "catalog"
catalog_path.mkdir(exist_ok=True)

print(f"Working directory: {Path.cwd()}")
print(f"Catalog directory: {catalog_path}")

try:
    # Download EUR/USD sample data
    print("Downloading EUR/USD sample data...")
    url = "https://raw.githubusercontent.com/nautechsystems/nautilus_data/main/raw_data/fx_hist_data/DAT_ASCII_EURUSD_T_202001.csv.gz"
    filename = "EURUSD_202001.csv.gz"

    print(f"Downloading from: {url}")
    urllib.request.urlretrieve(url, filename)  # noqa: S310
    print("Download complete")

    # Create the instrument
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

    catalog.write_data([instrument])
    print("Instrument written to catalog")

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
    raise SystemExit(1) from e

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
# The catalog now contains one EUR/USD instrument.

# %%
# Load the catalog from current working directory
catalog_path = Path.cwd() / "catalog"

catalog = ParquetDataCatalog(str(catalog_path))
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
# Read more about [MACD here](https://www.investopedia.com/terms/m/macd.asp). This indicator serves as an example with no expected alpha. You can register indicators to receive data types automatically, but here we pass the `QuoteTick` to the indicator manually in `on_quote_tick`.
#

# %%
from nautilus_trader.core.message import Event
from nautilus_trader.indicators import MovingAverageConvergenceDivergence
from nautilus_trader.model import InstrumentId
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.events import PositionClosed
from nautilus_trader.trading.strategy import Strategy
from nautilus_trader.trading.strategy import StrategyConfig


class MACDConfig(StrategyConfig):
    instrument_id: InstrumentId
    fast_period: int = 12
    slow_period: int = 26
    trade_size: int = 1_000_000


class MACDStrategy(Strategy):
    """
    Simple MACD crossover strategy.
    """

    def __init__(self, config: MACDConfig):
        super().__init__(config=config)
        self.macd = MovingAverageConvergenceDivergence(
            fast_period=config.fast_period,
            slow_period=config.slow_period,
            price_type=PriceType.MID,
        )
        self.trade_size = Quantity.from_int(config.trade_size)
        self.last_macd_above_zero: bool | None = None
        self.pending_entry: OrderSide | None = None

    def on_start(self):
        self.subscribe_quote_ticks(instrument_id=self.config.instrument_id)

    def on_stop(self):
        self.close_all_positions(self.config.instrument_id)
        self.unsubscribe_quote_ticks(instrument_id=self.config.instrument_id)

    def on_quote_tick(self, tick: QuoteTick):
        self.macd.handle_quote_tick(tick)
        if self.macd.initialized:
            self.check_signals()

    def on_event(self, event: Event):
        # When a position closes, enter the pending order if we were flipping
        if (
            isinstance(event, PositionClosed)
            and self.pending_entry
            and event.instrument_id == self.config.instrument_id
        ):
            self.enter(self.pending_entry)
            self.pending_entry = None

    def check_signals(self):
        current_above = self.macd.value > 0

        if self.last_macd_above_zero is None:
            self.last_macd_above_zero = current_above
            return

        # Only act on crossovers
        if self.last_macd_above_zero == current_above:
            return

        self.last_macd_above_zero = current_above
        target_side = OrderSide.BUY if current_above else OrderSide.SELL

        # If we have a position, close it first and queue the new entry
        if self.cache.positions_open(instrument_id=self.config.instrument_id):
            self.pending_entry = target_side
            self.close_all_positions(self.config.instrument_id)
        else:
            self.enter(target_side)

    def enter(self, side: OrderSide):
        order = self.order_factory.market(
            instrument_id=self.config.instrument_id,
            order_side=side,
            quantity=self.trade_size,
        )
        self.submit_order(order)


# %% [markdown]
# ## 4. Configure backtest
#
# Nautilus uses a `BacktestNode` to orchestrate backtest runs. Configure a `BacktestRunConfig` with these minimal fields:
#
# - `engine`: The trading system, including strategies.
# - `venues`: The simulated venues (exchanges or brokers).
# - `data`: The input data for the backtest.
#
# See the docs for additional configuration options.

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
# Load the instruments from the `ParquetDataCatalog`.

# %%
instruments = catalog.instruments()
instruments

# %% [markdown]
# Add a `BacktestDataConfig` for each tick type and instrument. Here we add the `QuoteTick`(s) for EUR/USD:

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
# Add strategies via `ImportableStrategyConfig`, which imports strategies from any file or package. Here our `MACDStrategy` lives in the current module (`__main__`).

# %%
engine = BacktestEngineConfig(
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
# Pass the config pieces to a `BacktestRunConfig`.

# %%
config = BacktestRunConfig(
    engine=engine,
    venues=[venue],
    data=[data],
)

# %% [markdown]
# `BacktestNode` separates configuration from execution, so it can run multiple configs (different parameters or data batches).
#

# %%
from nautilus_trader.backtest.results import BacktestResult


node = BacktestNode(configs=[config])

# Runs one or many configs synchronously
results: list[BacktestResult] = node.run()

# %% [markdown]
# ### Expected Output
#
# When you run the backtest, you should see:
# - **Trades being executed** (both BUY and SELL orders).
# - **Positions being opened and closed** based on MACD crossover signals.
# - **P&L calculations** showing wins and losses.
# - **Performance metrics** including win rate, profit factor, and additional statistics.
#
# If you're not seeing any trades, check:
# 1. The data time range (you may need more data).
# 2. The indicator warm-up period (MACD needs time to initialize).

# %% [markdown]
# ## 8. Analyze results

# %% [markdown]
# Query the `BacktestEngine` used by the `BacktestNode` via the config ID. The engine provides additional reports.

# %%
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.model import Venue


engine: BacktestEngine = node.get_engine(config.id)

len(engine.trader.generate_order_fills_report())

# %%
engine.trader.generate_positions_report()

# %%
engine.trader.generate_account_report(Venue("SIM"))

# %% [markdown]
# ## 9. Performance metrics
#
# Additional performance metrics to better understand how our strategy performed:

# %%
# Get performance statistics

# Get the account and positions
account = engine.trader.generate_account_report(Venue("SIM"))
positions = engine.trader.generate_positions_report()
orders = engine.trader.generate_order_fills_report()

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
        print(f"Risk/Reward Ratio: {avg_win / avg_loss:.2f}")
else:
    print("\nNo positions generated. Check strategy parameters.")

print("\n=== FINAL ACCOUNT STATE ===")
print(account.tail(1).to_string())
