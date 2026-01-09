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
"""
Deribit execution client tester example.

This example demonstrates how to use the Deribit execution adapter to:
- Connect to Deribit (testnet by default)
- Load instruments for futures
- Fetch account state

Environment variables (for testnet):
- DERIBIT_TESTNET_API_KEY: Your Deribit testnet API key
- DERIBIT_TESTNET_API_SECRET: Your Deribit testnet API secret

For production, use:
- DERIBIT_API_KEY: Your Deribit API key
- DERIBIT_API_SECRET: Your Deribit API secret

Run with:
    source .venv/bin/activate && USE_TESTNET=false python examples/live/deribit/deribit_exec_tester.py

"""

import os
from decimal import Decimal

from nautilus_trader.adapters.deribit import DERIBIT
from nautilus_trader.adapters.deribit import DeribitDataClientConfig
from nautilus_trader.adapters.deribit import DeribitExecClientConfig
from nautilus_trader.adapters.deribit import DeribitLiveDataClientFactory
from nautilus_trader.adapters.deribit import DeribitLiveExecClientFactory
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.core.nautilus_pyo3 import DeribitInstrumentKind
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.test_kit.strategies.tester_exec import ExecTester
from nautilus_trader.test_kit.strategies.tester_exec import ExecTesterConfig


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***

# Configuration
# Use testnet by default for safety
USE_TESTNET = os.getenv("USE_TESTNET", "true").lower() != "false"

# Optional: Filter by instrument kinds
instrument_kinds: tuple[DeribitInstrumentKind, ...] | None = (DeribitInstrumentKind.FUTURE,)

# Define instrument to test with
perpetual_id = InstrumentId.from_str(f"BTC-PERPETUAL.{DERIBIT}")

# Order quantity (minimum for BTC-PERPETUAL is 10 USD)
order_qty = Decimal(10)

# Configure the trading node
config_node = TradingNodeConfig(
    trader_id=TraderId("TESTER-001"),
    logging=LoggingConfig(
        log_level="INFO",
        use_pyo3=True,
    ),
    exec_engine=LiveExecEngineConfig(
        reconciliation=False,  # Disable reconciliation for testing
    ),
    data_clients={
        DERIBIT: DeribitDataClientConfig(
            api_key=None,  # Will use env var
            api_secret=None,  # Will use env var
            instrument_kinds=instrument_kinds,
            instrument_provider=InstrumentProviderConfig(
                load_all=True,
            ),
            is_testnet=USE_TESTNET,
            http_timeout_secs=30,
        ),
    },
    exec_clients={
        DERIBIT: DeribitExecClientConfig(
            api_key=None,  # Will use env var
            api_secret=None,  # Will use env var
            instrument_kinds=instrument_kinds,
            instrument_provider=InstrumentProviderConfig(
                load_all=True,
            ),
            is_testnet=USE_TESTNET,
            http_timeout_secs=30,
        ),
    },
    timeout_connection=30.0,
    timeout_disconnection=5.0,
    timeout_post_stop=5.0,
)

# Instantiate the node with a configuration
node = TradingNode(config=config_node)

# Configure and initialize the tester
config_tester = ExecTesterConfig(
    instrument_id=perpetual_id,
    external_order_claims=[perpetual_id],
    order_qty=order_qty,
    subscribe_quotes=True,
    subscribe_trades=True,
    enable_limit_buys=True,  # Enable limit buy orders
    enable_limit_sells=True,  # Enable limit sell orders
)
tester = ExecTester(config=config_tester)

node.trader.add_strategy(tester)

# Register your client factories with the node
node.add_data_client_factory(DERIBIT, DeribitLiveDataClientFactory)
node.add_exec_client_factory(DERIBIT, DeribitLiveExecClientFactory)
node.build()


# Stop and dispose of the node with SIGINT/CTRL+C
if __name__ == "__main__":
    try:
        node.run()
    finally:
        node.dispose()
