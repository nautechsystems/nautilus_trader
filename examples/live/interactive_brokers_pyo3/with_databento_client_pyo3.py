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

from nautilus_trader.adapters.databento import DATABENTO
from nautilus_trader.adapters.databento import DatabentoDataClientConfig
from nautilus_trader.adapters.databento import DatabentoLiveDataClientFactory
from nautilus_trader.adapters.interactive_brokers.common import IB
from nautilus_trader.adapters.interactive_brokers_pyo3 import InteractiveBrokersExecClientConfig
from nautilus_trader.adapters.interactive_brokers_pyo3 import (
    InteractiveBrokersInstrumentProviderConfig,
)
from nautilus_trader.adapters.interactive_brokers_pyo3 import (
    InteractiveBrokersV1LiveExecClientFactory,
)
from nautilus_trader.config import InstrumentProviderConfig
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

instrument_ids = [
    InstrumentId.from_str("SPY.XNAS"),
    InstrumentId.from_str("AAPL.XNAS"),
    InstrumentId.from_str("V.XNAS"),
    InstrumentId.from_str("CLM6.GLBX"),
    InstrumentId.from_str("ESM6.GLBX"),
    InstrumentId.from_str("TFMG7.NDEX"),
    InstrumentId.from_str("CN5.IFEU"),
    InstrumentId.from_str("GH5.IFEU"),
]

exec_clients: dict[str, LiveExecClientConfig] = {}
if ENABLE_EXECUTION_CLIENT and EXEC_ACCOUNT_ID is not None:
    exec_clients = {
        IB: InteractiveBrokersExecClientConfig(
            ibg_host=IB_HOST,
            ibg_port=IB_PORT,
            ibg_client_id=int(os.getenv("IB_PYO3_EXEC_CLIENT_ID", "1312")),
            account_id=EXEC_ACCOUNT_ID,
            instrument_provider=InteractiveBrokersInstrumentProviderConfig(
                load_ids=frozenset(instrument_ids),
            ),
            routing=RoutingConfig(default=True),
        ),
    }

config_node = TradingNodeConfig(
    trader_id="TESTER-001",
    logging=LoggingConfig(log_level="INFO"),
    data_clients={
        DATABENTO: DatabentoDataClientConfig(
            api_key=None,
            http_gateway=None,
            instrument_provider=InstrumentProviderConfig(load_all=True),
            instrument_ids=instrument_ids,
        ),
    },
    exec_clients=exec_clients,
    timeout_connection=90.0,
    timeout_reconciliation=5.0,
    timeout_portfolio=5.0,
    timeout_disconnection=5.0,
    timeout_post_stop=2.0,
)

node = TradingNode(config=config_node)
strategy = SubscribeStrategy(
    config=SubscribeStrategyConfig(
        instrument_id=InstrumentId.from_str("SPY.XNAS"),
        trade_ticks=False,
        quote_ticks=True,
        bars=True,
    ),
)
node.trader.add_strategy(strategy)
node.add_data_client_factory(DATABENTO, DatabentoLiveDataClientFactory)
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
