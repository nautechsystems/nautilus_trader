# ---
# jupyter:
#   jupytext:
#     formats: py:percent
#     text_representation:
#       extension: .py
#       format_name: percent
#       format_version: '1.3'
#       jupytext_version: 1.18.1
#   kernelspec:
#     display_name: Python 3 (ipykernel)
#     language: python
#     name: python3
# ---

# %% [markdown]
# ## Futures settlement at expiry (Databento)
#
# This example runs a short backtest around a futures contract expiry. It downloads
# only a small window of data around expiry via `databento_data`. The backtest engine
# settles open positions at expiry using `settlement_prices` (or market if omitted).

# %% [markdown]
# ## Imports

# %%
from datetime import UTC
from datetime import datetime
from datetime import timedelta

from nautilus_trader.adapters.databento.data_utils import databento_data
from nautilus_trader.adapters.databento.data_utils import init_databento_client
from nautilus_trader.adapters.databento.data_utils import load_catalog
from nautilus_trader.backtest.node import BacktestNode
from nautilus_trader.common.enums import LogColor
from nautilus_trader.config import BacktestDataConfig
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.config import BacktestRunConfig
from nautilus_trader.config import BacktestVenueConfig
from nautilus_trader.config import ImportableStrategyConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import StrategyConfig
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Quantity
from nautilus_trader.trading.strategy import Strategy


# %% [markdown]
# ## Parameters
#
# Set the expiry end time (e.g. contract settlement time). Data is downloaded
# from 5 minutes before up to the expiry end time. The future symbol is derived
# from the expiry date so the correct expiring contract is downloaded.

# %%
catalog_folder = "futures_settlement"
venue_name = "XCME"

# Expiry end time UTC (ES expires at 14:30 UTC)
expiry_end_time = "2025-12-19T14:30"

# CME month codes (F=Jan, G=Feb, H=Mar, J=Apr, K=May, M=Jun, N=Jul, Q=Aug, U=Sep, V=Oct, X=Nov, Z=Dec)
_CME_MONTH_CODES = "FGHJKMNQUVXZ"

_expiry_dt = datetime.fromisoformat(expiry_end_time)
if _expiry_dt.tzinfo is None:
    _expiry_dt = _expiry_dt.replace(tzinfo=UTC)

# Derive future symbol for the contract that expires on this date (e.g. ESZ5 for Dec 2025)
_product = "ES"
_month_code = _CME_MONTH_CODES[_expiry_dt.month - 1]
_year_digit = str(_expiry_dt.year)[-1]
future_symbol = f"{_product}{_month_code}{_year_digit}"

# Next future (e.g. ESH6 after ESZ5) so we have data from 14:25 to 14:35 and the clock advances past expiry
# CME ES quarterly: H=Mar, M=Jun, U=Sep, Z=Dec; next quarter is +3 months (or Mar next year after Dec)
if _expiry_dt.month == 12:
    _next_month_code = "H"
    _next_year_digit = str(_expiry_dt.year + 1)[-1]
else:
    _next_month = _expiry_dt.month + 3
    _next_month_code = _CME_MONTH_CODES[_next_month - 1]
    _next_year_digit = _year_digit
next_future_symbol = f"{_product}{_next_month_code}{_next_year_digit}"

future_id = InstrumentId.from_str(f"{future_symbol}.{venue_name}")
next_future_id = InstrumentId.from_str(f"{next_future_symbol}.{venue_name}")

# Window: 5 min before expiry and 5 min after expiry so the backtest clock
# advances past 14:30 and the settlement timer executes
_window_start_dt = _expiry_dt - timedelta(minutes=5)
_window_end_dt = _expiry_dt + timedelta(minutes=5)
window_start = _window_start_dt.strftime("%Y-%m-%dT%H:%M:%S")
window_end = _window_end_dt.strftime("%Y-%m-%dT%H:%M:%S")

backtest_start_time = window_start
backtest_end_time = window_end

# %% [markdown]
# ## Download data (optional)
#
# Download definition and bbo-1m for the expiring future and the next future
# from 14:25 to 14:35. Data for the next future extends past 14:30 so the
# backtest clock advances and the settlement timer executes.
# Set `load_databento_files_if_exist=True` to skip re-download when files exist.

# %%
# Uncomment to download; set your API key first.
init_databento_client(databento_api_key=None)

# Expiring future and next future so we have data past 14:30 (next future 14:25-14:35)
databento_data(
    [future_symbol, next_future_symbol],
    window_start,
    window_end,
    "bbo-1m",
    "futures_settlement",
    catalog_folder,
    dataset="GLBX.MDP3",
    to_catalog=True,
    load_databento_files_if_exist=True,
)

# %% [markdown]
# ## Load catalog

# %%
catalog = load_catalog(catalog_folder)

# %% [markdown]
# ## Strategy
#
# Subscribes to quote ticks for the expiring future and the next future so the
# backtest clock advances past 14:30 and we see the crossing of expiry.
# Opens one long in the expiring future; settlement at expiry uses
# settlement_prices from the engine config.


# %%
class FuturesSettlementConfigStrategy(StrategyConfig, frozen=True):
    future_id: InstrumentId
    next_future_id: InstrumentId


class FuturesSettlementStrategy(Strategy):
    """
    Opens one long futures position in the expiring contract; subscribes to both
    expiring and next future so the clock advances past 14:30 and settlement at expiry
    is visible.
    """

    def __init__(self, config: FuturesSettlementConfigStrategy):
        super().__init__(config=config)
        self.order_submitted = False

    def on_start(self):
        self.request_instrument(self.config.future_id)
        self.request_instrument(self.config.next_future_id)
        self.subscribe_quote_ticks(self.config.future_id)
        self.subscribe_quote_ticks(self.config.next_future_id)

    def on_quote_tick(self, tick: QuoteTick):
        if tick.instrument_id == self.config.future_id and not self.order_submitted:
            self.log.warning(
                f"Quote received, submitting market order for {self.config.future_id}",
                color=LogColor.RED,
            )
            order = self.order_factory.market(
                instrument_id=self.config.future_id,
                order_side=OrderSide.BUY,
                quantity=Quantity.from_int(1),
            )
            self.submit_order(order)
            self.order_submitted = True

    def on_order_filled(self, event):
        self.log.warning(f"Order filled: {event}", color=LogColor.RED)

    def on_position_opened(self, event):
        self.log.warning(f"Position opened: {event}", color=LogColor.RED)

    def on_position_closed(self, event):
        self.log.warning(f"Position closed: {event}", color=LogColor.RED)

    def on_stop(self):
        self.unsubscribe_quote_ticks(self.config.future_id)
        self.unsubscribe_quote_ticks(self.config.next_future_id)


# %% [markdown]
# ## Backtest node

# %%
strategies = [
    ImportableStrategyConfig(
        strategy_path=FuturesSettlementStrategy.fully_qualified_name(),
        config_path=FuturesSettlementConfigStrategy.fully_qualified_name(),
        config={
            "future_id": future_id,
            "next_future_id": next_future_id,
        },
    ),
]

logging = LoggingConfig(
    log_level="WARNING",
    log_level_file="WARNING",
    log_directory=".",
    log_file_name="databento_futures_settlement",
    clear_log_file=True,
)

# Custom settlement price for the expiring future (e.g. official CME settlement)
settlement_prices = {future_id: 6000.0}

engine_config = BacktestEngineConfig(
    logging=logging,
    strategies=strategies,
)

data = [
    BacktestDataConfig(
        data_cls=QuoteTick,
        catalog_path=catalog.path,
        instrument_ids=[future_id, next_future_id],
    ),
]

venues = [
    BacktestVenueConfig(
        name=venue_name,
        oms_type="NETTING",
        account_type="MARGIN",
        base_currency="USD",
        starting_balances=["1_000_000 USD"],
        settlement_prices=settlement_prices,
    ),
]

configs = [
    BacktestRunConfig(
        engine=engine_config,
        data=data,
        venues=venues,
        start=backtest_start_time,
        end=backtest_end_time,
    ),
]

node = BacktestNode(configs=configs)

# %%
results = node.run()

# %% [markdown]
# ## Backtest results

# %%
engine = node.get_engine(configs[0].id)
engine.trader.generate_order_fills_report()

# %%
engine.trader.generate_positions_report()

# %%
engine.trader.generate_account_report(Venue(venue_name))

# %%
node.dispose()
