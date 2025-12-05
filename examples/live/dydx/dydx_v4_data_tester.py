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
"""
DYdX v4 DataTester example using the Rust-backed adapter.

This script demonstrates how to use the DataTester actor to validate
market data functionality for the dYdX v4 adapter.

Prerequisites:
  - Environment variables:
      DYDX_WALLET_ADDRESS (or DYDX_TESTNET_WALLET_ADDRESS for testnet)

Usage:
  python dydx_v4_data_tester.py

"""

from nautilus_trader.adapters.dydx_v4 import DYDX_VENUE
from nautilus_trader.adapters.dydx_v4 import DYDXv4DataClientConfig
from nautilus_trader.adapters.dydx_v4 import DYDXv4LiveDataClientFactory
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.data import BarType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.test_kit.strategies.tester_data import DataTester
from nautilus_trader.test_kit.strategies.tester_data import DataTesterConfig


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***

# dYdX v4 perpetual markets
# All instruments follow {BASE}-{QUOTE}-PERP.DYDX naming
symbol = "ETH-USD-PERP"
instrument_id = InstrumentId.from_str(f"{symbol}.{DYDX_VENUE}")

# Configure the trading node
config_node = TradingNodeConfig(
    trader_id=TraderId("DYDX-DATA-TESTER-001"),
    logging=LoggingConfig(
        log_level="INFO",
        use_pyo3=True,
    ),
    data_clients={
        "DYDX": DYDXv4DataClientConfig(
            wallet_address=None,  # 'DYDX_WALLET_ADDRESS' or 'DYDX_TESTNET_WALLET_ADDRESS' env var
            instrument_provider=InstrumentProviderConfig(load_all=True),
            is_testnet=False,  # Mainnet by default; flip to True for testnet
        ),
    },
    timeout_connection=20.0,
    timeout_reconciliation=10.0,
    timeout_portfolio=10.0,
    timeout_disconnection=10.0,
    timeout_post_stop=1.0,
)

# Instantiate the node with a configuration
node = TradingNode(config=config_node)

# Configure your data tester
config_tester = DataTesterConfig(
    instrument_ids=[instrument_id],
    bar_types=[BarType.from_str(f"{instrument_id}-1-MINUTE-LAST-EXTERNAL")],
    subscribe_instrument=True,
    subscribe_quotes=True,
    subscribe_trades=True,
    subscribe_book_deltas=True,
    subscribe_funding_rates=True,
    manage_book=True,
    book_levels_to_print=10,
    log_data=True,
)

# Instantiate your actor
tester = DataTester(config=config_tester)

# Add your actors and modules
node.trader.add_actor(tester)

# Register your client factories with the node (using v4 Rust-backed factory)
node.add_data_client_factory("DYDX", DYDXv4LiveDataClientFactory)
node.build()


# Stop and dispose of the node with SIGINT/CTRL+C
if __name__ == "__main__":
    try:
        node.run()
    finally:
        node.dispose()
