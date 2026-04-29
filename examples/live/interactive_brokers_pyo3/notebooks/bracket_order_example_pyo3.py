#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

import os
import threading
import time

import pandas as pd

from nautilus_trader.adapters.interactive_brokers.common import IB
from nautilus_trader.adapters.interactive_brokers.common import IBContract
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
from nautilus_trader.examples.interactive_brokers import resolve_ib_endpoint
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model import BarType
from nautilus_trader.model import TraderId
from nautilus_trader.model.enums import ContingencyType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.orders import OrderList
from nautilus_trader.trading import Strategy
from nautilus_trader.trading.config import StrategyConfig


IB_HOST, IB_PORT = resolve_ib_endpoint("IB_PYO3_HOST", "IB_PYO3_PORT")


class StratMreConfig(StrategyConfig, frozen=True):
    tradable_instrument_id: str | None = "ESM6.CME"


class StratMre(Strategy):
    def __init__(self, config: StratMreConfig) -> None:
        super().__init__(config)
        self.bar_type_m1: dict[InstrumentId, BarType] = {}
        self.tradable_instrument_id = config.tradable_instrument_id

    def on_start(self) -> None:
        for instrument in self.cache.instruments():
            if str(instrument.id) == self.tradable_instrument_id:
                self.bar_type_m1[instrument.id] = BarType.from_str(
                    f"{instrument.id}-1-MINUTE-LAST-EXTERNAL",
                )
                self.buy_bracket(instrument.id, instrument.make_price(5900))
                self.clock.set_time_alert(
                    "modify-sl",
                    self.clock.utc_now() + pd.Timedelta(seconds=10),
                    lambda _event, instrument=instrument: self.modify_sl(
                        instrument.id,
                        instrument.make_price(5850),
                    ),
                )

    def buy_bracket(self, instrument_id, low):
        instrument = self.cache.instrument(instrument_id)
        if instrument is None:
            return

        order_list: OrderList = self.order_factory.bracket(
            instrument_id=instrument_id,
            order_side=OrderSide.BUY,
            quantity=instrument.make_qty(1),
            time_in_force=TimeInForce.DAY,
            entry_post_only=False,
            contingency_type=ContingencyType.OCO,
            sl_trigger_price=instrument.make_price(low - 10),
            tp_order_type=OrderType.LIMIT,
            tp_price=instrument.make_price(low + 10),
            tp_post_only=False,
            entry_order_type=OrderType.MARKET,
            emulation_trigger=TriggerType.NO_TRIGGER,
        )
        self.submit_order_list(order_list)

    def modify_sl(self, instrument_id, low):
        for order in self.cache.orders(instrument_id=instrument_id):
            if order.is_open and order.order_type == OrderType.STOP_MARKET:
                self.modify_order(order, trigger_price=low)
                break


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
        trader_id=TraderId("TESTER-001"),
        logging=LoggingConfig(log_level="INFO"),
        data_clients={
            IB: InteractiveBrokersDataClientConfig(
                ibg_host=IB_HOST,
                ibg_port=IB_PORT,
                ibg_client_id=int(os.getenv("IB_PYO3_DATA_CLIENT_ID", "1401")),
                instrument_provider=instrument_provider,
                market_data_type=MarketDataType.DelayedFrozen,
                use_regular_trading_hours=False,
            ),
        },
        exec_clients={
            IB: InteractiveBrokersExecClientConfig(
                ibg_host=IB_HOST,
                ibg_port=IB_PORT,
                ibg_client_id=int(os.getenv("IB_PYO3_EXEC_CLIENT_ID", "1402")),
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
        timeout_post_stop=2.0,
    ),
)

strategy = StratMre(
    config=StratMreConfig(
        tradable_instrument_id="ESM6.CME",
        manage_stop=True,
        market_exit_time_in_force=TimeInForce.DAY,
        market_exit_reduce_only=False,
    ),
)

node.trader.add_strategy(strategy)
node.add_data_client_factory(IB, InteractiveBrokersV1LiveDataClientFactory)
node.add_exec_client_factory(IB, InteractiveBrokersV1LiveExecClientFactory)
node.build()


if __name__ == "__main__":
    auto_stop_seconds = int(os.getenv("IB_PYO3_AUTO_STOP_SECONDS", "20"))

    def stop_after_delay() -> None:
        time.sleep(auto_stop_seconds)
        node.stop()

    if auto_stop_seconds > 0:
        threading.Thread(target=stop_after_delay, daemon=True).start()

    try:
        node.run()
    finally:
        node.dispose()
