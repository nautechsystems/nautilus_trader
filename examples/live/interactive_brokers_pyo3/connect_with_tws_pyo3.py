#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
# -------------------------------------------------------------------------------------------------

import os
import threading
import time

from nautilus_trader.adapters.interactive_brokers.common import IB
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
from nautilus_trader.config import LiveExecClientConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import RoutingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.examples.interactive_brokers import resolve_ib_endpoint
from nautilus_trader.examples.strategies.subscribe import SubscribeStrategy
from nautilus_trader.examples.strategies.subscribe import SubscribeStrategyConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.identifiers import InstrumentId


ENABLE_EXECUTION_CLIENT = os.getenv("IB_PYO3_ENABLE_EXECUTION", "0") == "1"
EXEC_ACCOUNT_ID = os.getenv("TWS_ACCOUNT")
IB_HOST, IB_PORT = resolve_ib_endpoint("IB_PYO3_HOST", "IB_PYO3_PORT")

instrument_provider = InteractiveBrokersInstrumentProviderConfig(
    load_ids=frozenset(
        [
            "^SPX.CBOE",
        ],
    ),
)

exec_clients: dict[str, LiveExecClientConfig] = {}
if ENABLE_EXECUTION_CLIENT and EXEC_ACCOUNT_ID is not None:
    exec_clients = {
        IB: InteractiveBrokersExecClientConfig(
            ibg_host=IB_HOST,
            ibg_port=IB_PORT,
            ibg_client_id=int(os.getenv("IB_PYO3_EXEC_CLIENT_ID", "1302")),
            account_id=EXEC_ACCOUNT_ID,
            instrument_provider=instrument_provider,
            routing=RoutingConfig(default=True),
        ),
    }

config_node = TradingNodeConfig(
    trader_id="TESTER-001",
    logging=LoggingConfig(log_level="INFO"),
    data_clients={
        IB: InteractiveBrokersDataClientConfig(
            ibg_host=IB_HOST,
            ibg_port=IB_PORT,
            ibg_client_id=int(os.getenv("IB_PYO3_DATA_CLIENT_ID", "1301")),
            instrument_provider=instrument_provider,
            market_data_type=MarketDataType.DelayedFrozen,
        ),
    },
    exec_clients=exec_clients,
    data_engine=LiveDataEngineConfig(
        time_bars_timestamp_on_close=False,
        validate_data_sequence=True,
    ),
    timeout_connection=90.0,
    timeout_reconciliation=5.0,
    timeout_portfolio=5.0,
    timeout_disconnection=5.0,
    timeout_post_stop=2.0,
)

node = TradingNode(config=config_node)
strategy = SubscribeStrategy(
    config=SubscribeStrategyConfig(
        instrument_id=InstrumentId.from_str("^SPX.CBOE"),
        trade_ticks=False,
        quote_ticks=False,
        bars=False,
        index_prices=True,
    ),
)
node.trader.add_strategy(strategy)
node.add_data_client_factory(IB, InteractiveBrokersV1LiveDataClientFactory)
if exec_clients:
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
