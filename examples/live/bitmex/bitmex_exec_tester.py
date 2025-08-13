#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.adapters.bitmex import BITMEX
from nautilus_trader.adapters.bitmex import BitmexDataClientConfig
from nautilus_trader.adapters.bitmex import BitmexExecClientConfig
from nautilus_trader.adapters.bitmex import BitmexLiveDataClientFactory
from nautilus_trader.adapters.bitmex import BitmexLiveExecClientFactory
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.core.nautilus_pyo3 import BitmexSymbolStatus
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.test_kit.strategies.tester_exec import ExecTester
from nautilus_trader.test_kit.strategies.tester_exec import ExecTesterConfig


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***

# Example symbols for different BitMEX products
# Perpetual swap: XBTUSD (Bitcoin perpetual)
# Futures: XBTH25 (Bitcoin futures expiring March 2025)
# Alt perpetuals: ETHUSD, SOLUSD, etc.

symbol = "XBTUSD"  # Bitcoin perpetual swap
order_qty = Decimal("100")  # Contract size in USD

# Configure the trading node
config_node = TradingNodeConfig(
    trader_id=TraderId("TESTER-001"),
    logging=LoggingConfig(log_level="INFO", use_pyo3=True),
    exec_engine=LiveExecEngineConfig(
        reconciliation=True,
        snapshot_orders=True,
        snapshot_positions=True,
        snapshot_positions_interval_secs=5.0,
    ),
    data_clients={
        BITMEX: BitmexDataClientConfig(
            api_key=None,  # 'BITMEX_API_KEY' env var
            api_secret=None,  # 'BITMEX_API_SECRET' env var
            base_url_http=None,  # Override with custom endpoint
            base_url_ws=None,  # Override with custom endpoint
            instrument_provider=InstrumentProviderConfig(load_all=True),
            symbol_status=BitmexSymbolStatus.OPEN,  # Filter for open instruments
            testnet=False,  # If client uses the testnet API
        ),
    },
    exec_clients={
        BITMEX: BitmexExecClientConfig(
            api_key=None,  # 'BITMEX_API_KEY' env var
            api_secret=None,  # 'BITMEX_API_SECRET' env var
            base_url_http=None,  # Override with custom endpoint
            base_url_ws=None,  # Override with custom endpoint
            instrument_provider=InstrumentProviderConfig(load_all=True),
            symbol_status=BitmexSymbolStatus.OPEN,  # Filter for open instruments
            testnet=False,  # If client uses the testnet API
        ),
    },
    timeout_connection=30.0,
    timeout_reconciliation=10.0,
    timeout_disconnection=10.0,
)

# Configure the execution tester strategy
config_tester = ExecTesterConfig(
    instrument_id=InstrumentId.from_str(f"{symbol}.BITMEX"),
    order_qty=order_qty,
)

# Setup and run the trading node
node = TradingNode(config=config_node)

# Add the strategy to the node
node.trader.add_strategy(ExecTester(config=config_tester))

# Register the client factories
node.add_data_client_factory(BITMEX, BitmexLiveDataClientFactory)
node.add_exec_client_factory(BITMEX, BitmexLiveExecClientFactory)
node.build()

# Run the node
try:
    node.run()
except KeyboardInterrupt:
    node.stop()
    node.dispose()
