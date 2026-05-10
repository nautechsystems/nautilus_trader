#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

from decimal import Decimal

from nautilus_trader.adapters.architect_ax import AX
from nautilus_trader.adapters.architect_ax import AxDataClientConfig
from nautilus_trader.adapters.architect_ax import AxEnvironment
from nautilus_trader.adapters.architect_ax import AxExecClientConfig
from nautilus_trader.adapters.architect_ax import AxLiveDataClientFactory
from nautilus_trader.adapters.architect_ax import AxLiveExecClientFactory
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.examples.strategies.orderbook_imbalance import OrderBookImbalance
from nautilus_trader.examples.strategies.orderbook_imbalance import OrderBookImbalanceConfig
from nautilus_trader.live.config import LiveRiskEngineConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***

instrument_id = InstrumentId.from_str(f"XAU-PERP.{AX}")

config_node = TradingNodeConfig(
    trader_id=TraderId("TESTER-001"),
    logging=LoggingConfig(
        log_level="INFO",
        use_pyo3=True,
    ),
    exec_engine=LiveExecEngineConfig(
        reconciliation=True,
        reconciliation_instrument_ids=[instrument_id],
    ),
    risk_engine=LiveRiskEngineConfig(bypass=True),
    data_clients={
        AX: AxDataClientConfig(
            environment=AxEnvironment.SANDBOX,
            instrument_provider=InstrumentProviderConfig(
                load_all=False,
                load_ids=frozenset([instrument_id]),
            ),
        ),
    },
    exec_clients={
        AX: AxExecClientConfig(
            environment=AxEnvironment.SANDBOX,
            instrument_provider=InstrumentProviderConfig(
                load_all=False,
                load_ids=frozenset([instrument_id]),
            ),
        ),
    },
    timeout_connection=20.0,
    timeout_reconciliation=10.0,
    timeout_portfolio=10.0,
    timeout_disconnection=10.0,
    timeout_post_stop=5.0,
)

node = TradingNode(config=config_node)

strategy = OrderBookImbalance(
    config=OrderBookImbalanceConfig(
        instrument_id=instrument_id,
        max_trade_size=Decimal(1),
        trigger_min_size=1.0,
        trigger_imbalance_ratio=0.10,
        min_seconds_between_triggers=5.0,
        book_type=BookType.L1_MBP,
        use_quote_ticks=True,
        manage_stop=True,
    ),
)

node.trader.add_strategy(strategy)

node.add_data_client_factory(AX, AxLiveDataClientFactory)
node.add_exec_client_factory(AX, AxLiveExecClientFactory)
node.build()

if __name__ == "__main__":
    try:
        node.run()
    finally:
        node.dispose()
