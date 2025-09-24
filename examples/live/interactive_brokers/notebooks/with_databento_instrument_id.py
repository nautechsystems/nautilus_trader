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

# %%
# Note: Use the jupytext python extension to be able to open this python file in jupyter as a notebook

# %%
# fmt: off
import os
import threading
import time

from ibapi.common import MarketDataTypeEnum as IBMarketDataTypeEnum

from nautilus_trader.adapters.interactive_brokers.common import IB
from nautilus_trader.adapters.interactive_brokers.common import IB_VENUE
from nautilus_trader.adapters.interactive_brokers.config import InteractiveBrokersDataClientConfig
from nautilus_trader.adapters.interactive_brokers.config import InteractiveBrokersExecClientConfig
from nautilus_trader.adapters.interactive_brokers.config import InteractiveBrokersInstrumentProviderConfig
from nautilus_trader.adapters.interactive_brokers.config import SymbologyMethod
from nautilus_trader.adapters.interactive_brokers.factories import InteractiveBrokersLiveDataClientFactory
from nautilus_trader.adapters.interactive_brokers.factories import InteractiveBrokersLiveExecClientFactory
from nautilus_trader.common.enums import LogColor
from nautilus_trader.config import LiveDataEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import RoutingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.events import PositionOpened
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.trading.config import StrategyConfig
from nautilus_trader.trading.strategy import Strategy


# fmt: on


# %%
class DemoStrategyConfig(StrategyConfig, frozen=True):
    bar_type: BarType
    instrument_id: InstrumentId


class DemoStrategy(Strategy):
    def __init__(self, config: DemoStrategyConfig):
        super().__init__(config=config)

        # Track if we've already placed an order
        self.order_placed = False

        # Track total bars seen
        self.count_of_bars: int = 0
        self.show_portfolio_at_bar: int | None = 0

    def on_start(self):
        """
        Handle strategy start event.
        """
        self.request_instrument(self.config.instrument_id)

        # self.request_instruments(
        #     venue=IB_VENUE,
        #     params={
        #         "ib_contracts": (
        #             {
        #                 "secType": "CONTFUT",
        #                 "exchange": "CME",
        #                 "symbol": "ES",
        #                 "build_futures_chain": True,
        #                 "build_options_chain": True,
        #                 "min_expiry_days": 10,
        #                 "max_expiry_days": 11,
        #             },
        #         ),
        #     },
        # )

    def on_instrument(self, instrument):
        self.log.info(f"Instrument ID: {instrument.id}")

        self.instrument = self.cache.instrument(self.config.instrument_id)

        # utc_now = self._clock.utc_now()
        # start = utc_now - pd.Timedelta(
        #     minutes=30,
        # )
        # self.request_bars(
        #     BarType.from_str(f"{self.config.instrument_id}-1-MINUTE-LAST-EXTERNAL"),
        #     start,
        # )

        # utc_now = self.clock.utc_now()
        # self.subscribe_bars(self.config.bar_type, params={"start_ns":(utc_now - pd.Timedelta(minutes=2)).value})

        # Prepare values for order
        last_price = self.instrument.make_price(46745)
        tick_size = self.instrument.price_increment
        profit_price = self.instrument.make_price(last_price + (10 * tick_size))
        stoploss_price = self.instrument.make_price(last_price - (10 * tick_size))

        # Create BUY MARKET order with PT and SL (both 10 ticks)
        bracket_order_list = self.order_factory.bracket(
            instrument_id=self.config.instrument_id,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(1),  # Trade size: 1 contract
            time_in_force=TimeInForce.GTC,
            tp_price=profit_price,
            sl_trigger_price=stoploss_price,
            entry_post_only=False,
            tp_post_only=False,
        )

        # Submit order and remember it
        self.submit_order_list(bracket_order_list)
        self.order_placed = True
        self.log.info(f"Submitted bracket order: {bracket_order_list}", color=LogColor.GREEN)

    def on_bar(self, bar: Bar):
        """
        Handle new bar event.
        """
        # Increment total bars seen
        self.count_of_bars += 1

        # Show portfolio state if we reached target bar
        if self.show_portfolio_at_bar == self.count_of_bars:
            self.show_portfolio_info("Portfolio state (2 minutes after position opened)")

        # Only place one order for demonstration
        if not self.order_placed:
            # Prepare values for order
            last_price = bar.close
            tick_size = self.instrument.price_increment
            profit_price = self.instrument.make_price(last_price + (10 * tick_size))
            stoploss_price = self.instrument.make_price(last_price - (10 * tick_size))

            # Create BUY MARKET order with PT and SL (both 10 ticks)
            bracket_order_list = self.order_factory.bracket(
                instrument_id=self.config.instrument_id,
                order_side=OrderSide.BUY,
                quantity=self.instrument.make_qty(1),  # Trade size: 1 contract
                time_in_force=TimeInForce.GTC,
                tp_price=profit_price,
                sl_trigger_price=stoploss_price,
                entry_post_only=False,
                tp_post_only=False,
            )

            # Submit order and remember it
            self.submit_order_list(bracket_order_list)
            self.order_placed = True
            self.log.info(f"Submitted bracket order: {bracket_order_list}", color=LogColor.GREEN)

    def on_position_opened(self, event: PositionOpened):
        """
        Handle position opened event.
        """
        # Log position details
        self.log.info(f"Position opened: {event}", color=LogColor.GREEN)

        # Show portfolio state when position is opened
        self.show_portfolio_info("Portfolio state (In position):")

        # Set target bar number for next portfolio display
        self.show_portfolio_at_bar = self.count_of_bars + 2  # Show after 2 bars

    def on_stop(self):
        """
        Handle strategy stop event.
        """
        # Show final portfolio state
        self.show_portfolio_info("Portfolio state (After trade)")

    def show_portfolio_info(self, intro_message: str = ""):
        """
        Display current portfolio information.
        """
        if intro_message:
            self.log.info(f"====== {intro_message} ======")

        # POSITION information
        self.log.info("Portfolio -> Position information:", color=LogColor.BLUE)
        is_flat = self.portfolio.is_flat(self.config.instrument_id)
        self.log.info(f"Is flat: {is_flat}", color=LogColor.BLUE)

        net_position = self.portfolio.net_position(self.config.instrument_id)
        self.log.info(f"Net position: {net_position} contract(s)", color=LogColor.BLUE)

        net_exposure = self.portfolio.net_exposure(self.config.instrument_id)
        self.log.info(f"Net exposure: {net_exposure}", color=LogColor.BLUE)

        # -----------------------------------------------------

        # P&L information
        self.log.info("Portfolio -> P&L information:", color=LogColor.YELLOW)

        realized_pnl = self.portfolio.realized_pnl(self.config.instrument_id)
        self.log.info(f"Realized P&L: {realized_pnl}", color=LogColor.YELLOW)

        unrealized_pnl = self.portfolio.unrealized_pnl(self.config.instrument_id)
        self.log.info(f"Unrealized P&L: {unrealized_pnl}", color=LogColor.YELLOW)

        # -----------------------------------------------------

        self.log.info("Portfolio -> Account information:", color=LogColor.CYAN)
        margins_init = self.portfolio.margins_init(IB_VENUE)
        self.log.info(f"Initial margin: {margins_init}", color=LogColor.CYAN)

        margins_maint = self.portfolio.margins_maint(IB_VENUE)
        self.log.info(f"Maintenance margin: {margins_maint}", color=LogColor.CYAN)

        balances_locked = self.portfolio.balances_locked(IB_VENUE)
        self.log.info(f"Locked balance: {balances_locked}", color=LogColor.CYAN)


# %%
# Tested instrument id
instrument_id = "YMZ5.XCBT"  # "^SPX.XCBO", "ES.XCME", "AAPL.XNAS", "YMU5.XCBT"

instrument_provider = InteractiveBrokersInstrumentProviderConfig(
    symbology_method=SymbologyMethod.IB_SIMPLIFIED,
    convert_exchange_to_mic_venue=True,
    build_futures_chain=False,
    build_options_chain=False,
    min_expiry_days=0,
    max_expiry_days=5,
    load_ids=frozenset(
        [
            instrument_id,
        ],
    ),
)

# Configure the trading node
# IMPORTANT: you must use the imported IB string so this client works properly
config_node = TradingNodeConfig(
    trader_id="TESTER-001",
    logging=LoggingConfig(log_level="INFO"),
    data_clients={
        IB: InteractiveBrokersDataClientConfig(
            ibg_port=7497,
            handle_revised_bars=False,
            use_regular_trading_hours=False,
            instrument_provider=instrument_provider,
            market_data_type=IBMarketDataTypeEnum.DELAYED_FROZEN,
        ),
    },
    exec_clients={
        IB: InteractiveBrokersExecClientConfig(
            ibg_port=7497,
            instrument_provider=instrument_provider,
            routing=RoutingConfig(default=True),
            account_id=os.environ.get("TWS_ACCOUNT"),
        ),
    },
    data_engine=LiveDataEngineConfig(
        time_bars_timestamp_on_close=False,  # Will use opening time as `ts_event` (same as IB)
        validate_data_sequence=True,  # Will make sure DataEngine discards any Bars received out of sequence
    ),
    timeout_connection=90.0,
    timeout_reconciliation=5.0,
    timeout_portfolio=5.0,
    timeout_disconnection=5.0,
    timeout_post_stop=2.0,
)


# Instantiate the node with a configuration
node = TradingNode(config=config_node)

# Instantiate your strategy
strategy_config = DemoStrategyConfig(
    bar_type=BarType.from_str(f"{instrument_id}-1-MINUTE-LAST-EXTERNAL"),
    instrument_id=InstrumentId.from_str(instrument_id),
)
strategy = DemoStrategy(config=strategy_config)

# Add your strategies and modules
node.trader.add_strategy(strategy)

# Register your client factories with the node (can take user-defined factories)
node.add_data_client_factory(IB, InteractiveBrokersLiveDataClientFactory)
node.add_exec_client_factory(IB, InteractiveBrokersLiveExecClientFactory)
node.build()


# %%
def auto_stop_node(node, delay_seconds=15):
    """
    Automatically stop the node after a delay.
    """

    def stop_after_delay():
        time.sleep(delay_seconds)
        node.stop()

    thread = threading.Thread(target=stop_after_delay)
    thread.daemon = True
    thread.start()


# %%
node.run()

# %%
# # Start auto-stop timer
# # auto_stop_node(node, delay_seconds=60)

# try:
#     node.run()
# except KeyboardInterrupt:
#     node.stop()
# finally:
#     node.dispose()

# %%
node.trader.strategies()[0].on_bar(2)

# %%
# ?node.*

# %%
