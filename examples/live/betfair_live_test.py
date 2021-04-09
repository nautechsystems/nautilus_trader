#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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
import pathlib
import sys


sys.path.insert(
    0, str(pathlib.Path(__file__).parents[2])
)  # Allows relative imports from examples

from examples.strategies.betfair_test_strategy import BetfairTestStrategy
from nautilus_trader.adapters.betfair.factory import BetfairLiveDataClientFactory
from nautilus_trader.adapters.betfair.factory import BetfairLiveExecutionClientFactory
from nautilus_trader.live.node import TradingNode


# The configuration dictionary can come from anywhere such as a JSON or YAML
# file. Here it is hardcoded into the example for clarity.
market_id = "1.181245757"
config = {
    "trader": {
        "name": "TESTER",  # Not sent beyond system boundary
        "id_tag": "001",  # Used to ensure orders are unique for this trader
    },
    "system": {
        "connection_timeout": 30.0,  # Timeout for successful connections for all engine clients
        "disconnection_timeout": 30.0,  # Timeout for successful disconnection for all engine clients
        "check_residuals_delay": 15.0,  # How long to wait after stopping for residual events (secs)
    },
    "logging": {
        "level_stdout": "DBG",
    },
    "exec_database": {
        "type": "memory",
    },
    "risk": {},
    "strategy": {},
    "data_clients": {
        "BETFAIR": {
            "username": "BETFAIR_USERNAME",  # value is the environment variable key
            "password": "BETFAIR_PW",  # value is the environment variable key
            "app_key": "BETFAIR_APP_KEY",  # value is the environment variable key
            "cert_dir": "BETFAIR_CERT_DIR",  # value is the environment variable key
            "market_filter": {"market_id": market_id},
        },
    },
    "exec_clients": {
        "BETFAIR": {
            "username": "BETFAIR_USERNAME",  # value is the environment variable key
            "password": "BETFAIR_PW",  # value is the environment variable key
            "app_key": "BETFAIR_APP_KEY",  # value is the environment variable key
            "cert_dir": "BETFAIR_CERT_DIR",  # value is the environment variable key
            "market_filter": {"market_id": market_id},
            "sandbox_mode": False,  # If clients use the testnet
        },
    },
}


# Instantiate your strategies to pass into the trading node. You could add
# custom options into the configuration file or even use another configuration
# file.

strategy = BetfairTestStrategy(
    instrument_filter={"market_id": market_id},
    trade_size=Decimal(10.0),
    order_id_tag="001",
)

# Instantiate the node passing a list of strategies and configuration
node = TradingNode(strategies=[strategy], config=config)

# Register your client factories with the node (can take user defined factories)
node.add_data_client_factory("BETFAIR", BetfairLiveDataClientFactory)
node.add_exec_client_factory("BETFAIR", BetfairLiveExecutionClientFactory)
node.build()


# Stop and dispose of the node with SIGINT/CTRL+C
if __name__ == "__main__":
    try:
        node.start()
    finally:
        node.dispose()
