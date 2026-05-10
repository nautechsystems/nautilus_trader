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

# %%
# Note: Use the jupytext python extension to be able to open this python file in jupyter as a notebook

# %%
import os
import threading
import time
from typing import Any

import pandas as pd

from nautilus_trader.adapters.interactive_brokers.common import IB
from nautilus_trader.adapters.interactive_brokers.common import IB_VENUE
from nautilus_trader.adapters.interactive_brokers_pyo3 import InteractiveBrokersDataClientConfig
from nautilus_trader.adapters.interactive_brokers_pyo3 import InteractiveBrokersExecClientConfig
from nautilus_trader.adapters.interactive_brokers_pyo3 import (
    InteractiveBrokersInstrumentProviderConfig,
)
from nautilus_trader.adapters.interactive_brokers_pyo3 import (
    InteractiveBrokersV1LiveDataClientFactory,
)
from nautilus_trader.adapters.interactive_brokers_pyo3 import (
    InteractiveBrokersV1LiveExecClientFactory,
)
from nautilus_trader.adapters.interactive_brokers_pyo3.config import MarketDataType
from nautilus_trader.common.enums import LogColor
from nautilus_trader.config import LiveDataEngineConfig
from nautilus_trader.config import LiveExecClientConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import RoutingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.examples.interactive_brokers import resolve_ib_endpoint
from nautilus_trader.execution.messages import CancelAllOrders
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.events import PositionOpened
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.trading.config import StrategyConfig
from nautilus_trader.trading.strategy import Strategy


# %%
ENABLE_ORDER_SUBMISSION = os.getenv("ENABLE_IB_PYO3_ORDER_SUBMISSION", "0") == "1"
ENABLE_LIVE_BAR_SUBSCRIPTION = os.getenv("ENABLE_IB_PYO3_LIVE_BARS", "0") == "1"
AUTO_STOP_DELAY_SECONDS = int(os.getenv("IB_PYO3_AUTO_STOP_SECONDS", "30"))
ORDER_CANCEL_WAIT_SECONDS = float(os.getenv("IB_PYO3_CANCEL_WAIT_SECONDS", "10"))
ORDER_CANCEL_POLL_SECONDS = float(os.getenv("IB_PYO3_CANCEL_POLL_SECONDS", "0.25"))
DATA_CLIENT_ID = int(os.getenv("IB_PYO3_DATA_CLIENT_ID", "2"))
EXEC_CLIENT_ID = int(os.getenv("IB_PYO3_EXEC_CLIENT_ID", "3"))
IB_HOST, IB_PORT = resolve_ib_endpoint("IB_PYO3_HOST", "IB_PYO3_PORT")


# %%
class DemoStrategyConfig(StrategyConfig, frozen=True):
    bar_type: BarType
    instrument_id: InstrumentId
    enable_order_submission: bool = False


class DemoStrategy(Strategy):
    def __init__(self, config: DemoStrategyConfig):
        super().__init__(config=config)

        self.order_placed = False
        self.count_of_bars: int = 0
        self.show_portfolio_at_bar: int | None = 0
        self.instrument = None
        self.exec_client = None
        self._cleanup_in_progress = False
        self._startup_requested = False

    def on_start(self):
        self.request_instrument(self.config.instrument_id)
        self.request_instruments(
            venue=IB_VENUE,
            params={
                "ib_contracts": [
                    {
                        "secType": "STK",
                        "symbol": "SPY",
                        "exchange": "SMART",
                        "primaryExchange": "CBOE",
                        "build_options_chain": True,
                        "min_expiry_days": 0,
                        "max_expiry_days": 3,
                    },
                ],
            },
        )

    def on_instrument(self, instrument):
        self.log.info(f"Instrument ID: {instrument.id}")

        if self._startup_requested:
            return

        self.instrument = self.cache.instrument(self.config.instrument_id)
        if self.instrument is None:
            self.log.warning(f"{self.config.instrument_id} not found in cache")
            return

        self._startup_requested = True

        utc_now = self._clock.utc_now()
        start = utc_now - pd.Timedelta(minutes=30)
        self.request_bars(
            BarType.from_str(f"{self.config.instrument_id}-1-MINUTE-LAST-EXTERNAL"),
            start,
        )

        if ENABLE_LIVE_BAR_SUBSCRIPTION:
            utc_now = self.clock.utc_now()
            self.subscribe_bars(
                self.config.bar_type,
                params={"start_ns": (utc_now - pd.Timedelta(minutes=2)).value},
            )
        else:
            self.log.info(
                "Skipping live bar subscription; set ENABLE_IB_PYO3_LIVE_BARS=1 to enable",
                color=LogColor.YELLOW,
            )

        self._submit_demo_bracket_order()

    def on_bar(self, bar: Bar) -> None:
        self.count_of_bars += 1
        self.log.info(f"Received bar #{self.count_of_bars}: {bar}")

        if self.show_portfolio_at_bar == self.count_of_bars:
            self.show_portfolio_info("Portfolio state (2 minutes after position opened)")

        if not self.order_placed:
            self._submit_demo_bracket_order(bar.close)

    def on_order_canceled(self, event: Any) -> None:
        if event.instrument_id != self.config.instrument_id:
            return

        if self._has_pending_cached_orders():
            return

        self._cleanup_in_progress = False
        if not self.order_placed:
            self._submit_demo_bracket_order()

    def on_order_pending_cancel(self, event: Any) -> None:
        if event.instrument_id != self.config.instrument_id:
            return

        self._cleanup_in_progress = True
        self.log.info(f"Order pending cancel: {event}", color=LogColor.YELLOW)

    def on_position_opened(self, event: PositionOpened) -> None:
        self.log.info(f"Position opened: {event}", color=LogColor.GREEN)
        self.show_portfolio_info("Portfolio state (In position):")
        self.show_portfolio_at_bar = self.count_of_bars + 2

    def on_stop(self) -> None:
        if (
            self.config.enable_order_submission
            and getattr(self, "instrument", None) is not None
            and not self._cleanup_in_progress
        ):
            self._cancel_all_cached_orders("strategy shutdown")

        self.show_portfolio_info("Portfolio state (After run)")

    def show_portfolio_info(self, intro_message: str = "") -> None:
        if intro_message:
            self.log.info(f"====== {intro_message} ======")

        if not self.config.enable_order_submission:
            self.log.info(
                "Order submission is disabled, portfolio output is informational only",
                color=LogColor.BLUE,
            )

        self.log.info("Portfolio -> Position information:", color=LogColor.BLUE)
        is_flat = self.portfolio.is_flat(self.config.instrument_id)
        self.log.info(f"Is flat: {is_flat}", color=LogColor.BLUE)

        net_position = self.portfolio.net_position(self.config.instrument_id)
        self.log.info(f"Net position: {net_position} contract(s)", color=LogColor.BLUE)

        net_exposure = self.portfolio.net_exposure(self.config.instrument_id)
        self.log.info(f"Net exposure: {net_exposure}", color=LogColor.BLUE)

    def _submit_demo_bracket_order(self, last_price: Any = None) -> None:
        if self.order_placed:
            return

        if not self.config.enable_order_submission:
            self.log.info(
                "Skipping bracket order submission; set ENABLE_IB_PYO3_ORDER_SUBMISSION=1 to enable",
                color=LogColor.YELLOW,
            )
            return

        if self.instrument is None:
            self.log.warning("Cannot submit demo bracket order without an instrument")
            return

        if self._has_pending_cached_orders():
            if not self._cleanup_in_progress:
                self._cancel_all_cached_orders("pre-submit cleanup")
            return

        if last_price is None:
            last_price = self.instrument.make_price(46745)

        tick_size = self.instrument.price_increment
        profit_price = self.instrument.make_price(last_price + (10 * tick_size))
        stoploss_price = self.instrument.make_price(last_price - (10 * tick_size))

        bracket_order_list = self.order_factory.bracket(
            instrument_id=self.config.instrument_id,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(1),
            time_in_force=TimeInForce.GTC,
            tp_price=profit_price,
            sl_trigger_price=stoploss_price,
            entry_post_only=False,
            tp_post_only=False,
        )

        self.submit_order_list(bracket_order_list)
        self.order_placed = True
        self.log.info(f"Submitted bracket order: {bracket_order_list}", color=LogColor.GREEN)

    def _cancel_all_cached_orders(self, reason: str) -> None:
        if not self.config.enable_order_submission:
            return

        orders_open = self.cache.orders_open(instrument_id=self.config.instrument_id)
        orders_inflight = self.cache.orders_inflight(instrument_id=self.config.instrument_id)
        total_orders = len(orders_open) + len(orders_inflight)
        if total_orders == 0:
            self._cleanup_in_progress = False
            return

        if self.exec_client is None:
            self.log.warning("No execution client is bound for cancel-all handling")
            return

        self._cleanup_in_progress = True
        self.log.info(
            f"Canceling {total_orders} cached {self.config.instrument_id} order(s) for {reason}",
            color=LogColor.YELLOW,
        )
        command = CancelAllOrders(
            trader_id=self.trader_id,
            strategy_id=self.id,
            instrument_id=self.config.instrument_id,
            order_side=OrderSide.NO_ORDER_SIDE,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )
        self.exec_client.cancel_all_orders(command)

    def _has_pending_cached_orders(self) -> bool:
        return bool(
            self.cache.orders_open(instrument_id=self.config.instrument_id)
            or self.cache.orders_inflight(instrument_id=self.config.instrument_id),
        )


# %%
instrument_id = "YMM6.XCBT"
instrument_id_obj = InstrumentId.from_str(instrument_id)
exec_account_id = os.environ.get("TWS_ACCOUNT")
enable_exec_client = ENABLE_ORDER_SUBMISSION and exec_account_id is not None

instrument_provider_config = InteractiveBrokersInstrumentProviderConfig(
    convert_exchange_to_mic_venue=True,
    build_futures_chain=False,
    build_options_chain=False,
    min_expiry_days=0,
    max_expiry_days=5,
    load_ids=frozenset([instrument_id_obj]),
)

ib_data_client_config = InteractiveBrokersDataClientConfig(
    ibg_host=IB_HOST,
    ibg_port=IB_PORT,
    handle_revised_bars=False,
    use_regular_trading_hours=False,
    instrument_provider=instrument_provider_config,
    routing=RoutingConfig(default=True),
    market_data_type=MarketDataType.DELAYED_FROZEN,
    ibg_client_id=DATA_CLIENT_ID,
)

exec_clients: dict[str, LiveExecClientConfig] = {}

if enable_exec_client:
    exec_clients = {
        IB: InteractiveBrokersExecClientConfig(
            ibg_host=IB_HOST,
            ibg_port=IB_PORT,
            instrument_provider=instrument_provider_config,
            routing=RoutingConfig(default=True),
            account_id=exec_account_id,
            ibg_client_id=EXEC_CLIENT_ID,
        ),
    }

data_engine_config = LiveDataEngineConfig(
    time_bars_timestamp_on_close=False,
    validate_data_sequence=True,
)

logging_config = LoggingConfig(
    log_level="INFO",
    use_tracing=True,
    log_component_levels={
        "DataEngine": "INFO",
        "ExecEngine": "INFO",
        "Strategy-001": "INFO",
    },
)

config_node = TradingNodeConfig(
    trader_id="TESTER-001",
    logging=logging_config,
    data_clients={IB: ib_data_client_config},
    exec_clients=exec_clients,
    data_engine=data_engine_config,
    timeout_connection=90.0,
    timeout_reconciliation=5.0,
    timeout_portfolio=5.0,
    timeout_disconnection=5.0,
    timeout_post_stop=max(2.0, ORDER_CANCEL_WAIT_SECONDS),
)

node = TradingNode(config=config_node)

strategy_config = DemoStrategyConfig(
    bar_type=BarType.from_str(f"{instrument_id}-1-MINUTE-LAST-EXTERNAL"),
    instrument_id=instrument_id_obj,
    enable_order_submission=enable_exec_client,
)
strategy = DemoStrategy(config=strategy_config)

node.trader.add_strategy(strategy)
node.add_data_client_factory(IB, InteractiveBrokersV1LiveDataClientFactory)

if enable_exec_client:
    node.add_exec_client_factory(IB, InteractiveBrokersV1LiveExecClientFactory)
node.build()

if enable_exec_client:
    exec_engine = node.kernel.exec_engine
    default_client_id = exec_engine.default_client
    if default_client_id is None:
        raise RuntimeError("Expected an Interactive Brokers execution client to be registered")
    strategy.exec_client = exec_engine._clients[default_client_id]


# %%
def auto_stop_node(
    node_to_stop,
    strategy_to_stop,
    instrument_id,
    delay_seconds=AUTO_STOP_DELAY_SECONDS,
):
    def stop_after_delay():
        time.sleep(delay_seconds)

        if enable_exec_client:
            loop = node_to_stop.get_event_loop()
            if loop is not None and loop.is_running():
                loop.call_soon_threadsafe(
                    strategy_to_stop._cancel_all_cached_orders,
                    "scheduled shutdown",
                )

            deadline = time.time() + ORDER_CANCEL_WAIT_SECONDS
            while time.time() < deadline:
                if not (
                    node_to_stop.cache.orders_open(instrument_id=instrument_id)
                    or node_to_stop.cache.orders_inflight(instrument_id=instrument_id)
                ):
                    break
                time.sleep(ORDER_CANCEL_POLL_SECONDS)

        node_to_stop.stop()

    thread = threading.Thread(target=stop_after_delay)
    thread.daemon = True
    thread.start()


# %%
auto_stop_node(node, strategy, instrument_id_obj)

try:
    node.run()
finally:
    node.dispose()
