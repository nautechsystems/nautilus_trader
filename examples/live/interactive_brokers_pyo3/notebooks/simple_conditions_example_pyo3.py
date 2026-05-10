#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

import datetime
import os
import threading
import time

from nautilus_trader.adapters.interactive_brokers.common import IB
from nautilus_trader.adapters.interactive_brokers.common import IBContract
from nautilus_trader.adapters.interactive_brokers.common import IBOrderTags
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
from nautilus_trader.config import LiveDataEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import RoutingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.examples.interactive_brokers import resolve_ib_endpoint
from nautilus_trader.execution.messages import CancelAllOrders
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model import TraderId
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.orders import LimitOrder
from nautilus_trader.trading import Strategy
from nautilus_trader.trading.config import StrategyConfig


IB_HOST, IB_PORT = resolve_ib_endpoint("IB_PYO3_HOST", "IB_PYO3_PORT")


class SimpleConditionsConfig(StrategyConfig, frozen=True):
    tradable_instrument_id: str | None = "ESM6.CME"


class SimpleConditionsStrategy(Strategy):
    def __init__(self, config: SimpleConditionsConfig) -> None:
        super().__init__(config)
        self.tradable_instrument_id = config.tradable_instrument_id
        self.exec_client = None

    def on_order_canceled(self, event):
        self.log.info(f"Order canceled: {event}")

    def on_order_pending_cancel(self, event):
        self.log.info(f"Order pending cancel: {event}")

    def on_start(self) -> None:
        for instrument in self.cache.instruments():
            if str(instrument.id) == self.tradable_instrument_id:
                self.test_time_condition_order(instrument)
                self.test_price_condition_order(instrument)

    def test_price_condition_order(self, instrument):
        contract_id = instrument.info.get("contract", {}).get("conId", 0)
        if not contract_id:
            self.log.error(
                f"Missing IB contract metadata for {instrument.id}; cannot build price condition",
            )
            return

        price_condition = {
            "type": "price",
            "conId": contract_id,
            "exchange": "CME",
            "isMore": True,
            "price": 6000.0,
            "triggerMethod": 0,
            "conjunction": "and",
        }
        order = LimitOrder(
            trader_id=self.trader_id,
            strategy_id=self.id,
            instrument_id=instrument.id,
            client_order_id=self.order_factory.generate_client_order_id(),
            order_side=OrderSide.BUY,
            quantity=instrument.make_qty(1),
            price=instrument.make_price(5950),
            init_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            time_in_force=TimeInForce.GTC,
            tags=[IBOrderTags(conditions=[price_condition]).value],
        )
        self.submit_order(order)

    def test_time_condition_order(self, instrument):
        time_str = (datetime.datetime.now() + datetime.timedelta(minutes=5)).strftime(
            "%Y%m%d-%H:%M:%S",
        )
        time_condition = {
            "type": "time",
            "time": time_str,
            "isMore": True,
            "conjunction": "and",
        }
        order = LimitOrder(
            trader_id=self.trader_id,
            strategy_id=self.id,
            instrument_id=instrument.id,
            client_order_id=self.order_factory.generate_client_order_id(),
            order_side=OrderSide.SELL,
            quantity=instrument.make_qty(1),
            price=instrument.make_price(6100),
            init_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            time_in_force=TimeInForce.GTC,
            tags=[IBOrderTags(conditions=[time_condition]).value],
        )
        self.submit_order(order)

    def _cancel_all_cached_orders(self, reason: str) -> None:
        instrument_id = InstrumentId.from_str(self.tradable_instrument_id)
        orders_open = self.cache.orders_open(instrument_id=instrument_id)
        orders_inflight = self.cache.orders_inflight(instrument_id=instrument_id)
        total_orders = len(orders_open) + len(orders_inflight)
        if total_orders == 0:
            return

        if self.exec_client is None:
            self.log.warning("No execution client is bound for cancel-all handling")
            return

        self.log.info(f"Canceling {total_orders} cached orders for {reason}")
        command = CancelAllOrders(
            trader_id=self.trader_id,
            strategy_id=self.id,
            instrument_id=instrument_id,
            order_side=OrderSide.NO_ORDER_SIDE,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )
        self.exec_client.cancel_all_orders(command)

    def _has_pending_cached_orders(self) -> bool:
        instrument_id = InstrumentId.from_str(self.tradable_instrument_id)
        return bool(
            self.cache.orders_open(instrument_id=instrument_id)
            or self.cache.orders_inflight(instrument_id=instrument_id),
        )


es_contract = IBContract(
    secType="FUT",
    exchange="CME",
    localSymbol="ESM6",
    lastTradeDateOrContractMonth="20260618",
)

instrument_provider = InteractiveBrokersInstrumentProviderConfig(
    load_contracts=frozenset([es_contract]),
)

node = TradingNode(
    config=TradingNodeConfig(
        trader_id=TraderId("CONDITIONS-TESTER-001"),
        logging=LoggingConfig(log_level="INFO"),
        data_clients={
            IB: InteractiveBrokersDataClientConfig(
                ibg_host=IB_HOST,
                ibg_port=IB_PORT,
                ibg_client_id=int(os.getenv("IB_PYO3_DATA_CLIENT_ID", "1421")),
                instrument_provider=instrument_provider,
                market_data_type=MarketDataType.DelayedFrozen,
                use_regular_trading_hours=False,
            ),
        },
        exec_clients={
            IB: InteractiveBrokersExecClientConfig(
                ibg_host=IB_HOST,
                ibg_port=IB_PORT,
                ibg_client_id=int(os.getenv("IB_PYO3_EXEC_CLIENT_ID", "1422")),
                account_id=os.environ.get("TWS_ACCOUNT"),
                instrument_provider=instrument_provider,
                routing=RoutingConfig(default=True),
            ),
        },
        data_engine=LiveDataEngineConfig(
            time_bars_timestamp_on_close=False,
            validate_data_sequence=True,
        ),
        timeout_connection=90.0,
        timeout_reconciliation=5.0,
        timeout_portfolio=5.0,
        timeout_disconnection=5.0,
        timeout_post_stop=10.0,
    ),
)

strategy = SimpleConditionsStrategy(
    config=SimpleConditionsConfig(
        tradable_instrument_id="ESM6.CME",
        manage_stop=True,
        market_exit_max_attempts=400,
        market_exit_time_in_force=TimeInForce.DAY,
        market_exit_reduce_only=False,
    ),
)

node.trader.add_strategy(strategy)
node.add_data_client_factory(IB, InteractiveBrokersV1LiveDataClientFactory)
node.add_exec_client_factory(IB, InteractiveBrokersV1LiveExecClientFactory)
node.build()

exec_engine = node.kernel.exec_engine
default_client_id = exec_engine.default_client
if default_client_id is None:
    raise RuntimeError("Expected an Interactive Brokers execution client to be registered")
strategy.exec_client = exec_engine._clients[default_client_id]


if __name__ == "__main__":
    auto_stop_seconds = int(os.getenv("IB_PYO3_AUTO_STOP_SECONDS", "20"))

    def stop_after_delay() -> None:
        time.sleep(auto_stop_seconds)
        strategy._cancel_all_cached_orders("scheduled shutdown")
        deadline = time.time() + 45
        while time.time() < deadline:
            if not strategy._has_pending_cached_orders():
                break
            time.sleep(0.25)
        node.stop()

    if auto_stop_seconds > 0:
        threading.Thread(target=stop_after_delay, daemon=True).start()

    try:
        node.run()
    finally:
        node.dispose()
