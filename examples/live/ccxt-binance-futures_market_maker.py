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

from examples.strategies.volatility_market_maker import VolatilityMarketMaker
from nautilus_trader.adapters.ccxt.factories import CCXTDataClientFactory
from nautilus_trader.adapters.ccxt.factories import CCXTExecutionClientFactory
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.bar import BarSpecification
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue


# The configuration dictionary can come from anywhere such as a JSON or YAML
# file. Here it is hardcoded into the example for clarity.
config = {
    "trader": {
        "name": "TESTER",  # Not sent beyond system boundary
        "id_tag": "001",  # Used to ensure orders are unique for this trader
    },
    "system": {
        "loop_debug": False,  # The event loop debug mode
        "connection_timeout": 5.0,  # Timeout for successful connections for all engine clients
        "disconnection_timeout": 5.0,  # Timeout for successful disconnection for all engine clients
        "check_residuals_delay": 5.0,  # How long to wait after stopping for residual events (secs)
    },
    "logging": {
        "level_stdout": "INF",
    },
    "exec_database": {
        "type": "redis",
        "host": "localhost",
        "port": 6379,
    },
    "risk": {},
    "strategy": {
        "load_state": True,  # Strategy state is loaded from the database on start
        "save_state": True,  # Strategy state is saved to the database on shutdown
    },
    "data_clients": {
        "CCXT-BINANCE": {
            "account_id": "BINANCE_ACCOUNT_ID",  # value is the environment variable key
            "api_key": "BINANCE_API_KEY",  # value is the environment variable key
            "api_secret": "BINANCE_API_SECRET",  # value is the environment variable key
            "sandbox_mode": False,  # If client uses the testnet
            "defaultType": "future",  # If client uses the futures market
        },
    },
    "exec_clients": {
        "CCXT-BINANCE": {
            "account_id": "BINANCE_ACCOUNT_ID",  # value is the environment variable key
            "api_key": "BINANCE_API_KEY",  # value is the environment variable key
            "api_secret": "BINANCE_API_SECRET",  # value is the environment variable key
            "sandbox_mode": False,  # If client uses the testnet,
            "defaultType": "future",  # If client uses the futures market
        },
    },
}

# Instantiate your strategies to pass into the trading node. You could add
# custom options into the configuration file or even use another configuration
# file.

instrument_id = InstrumentId(
    symbol=Symbol("ETH/USDT"),
    venue=Venue("BINANCE"),
)

strategy = VolatilityMarketMaker(
    instrument_id=instrument_id,
    bar_spec=BarSpecification(1, BarAggregation.MINUTE, PriceType.LAST),
    trade_size=Decimal("0.05"),
    atr_period=20,
    atr_multiple=1.0,
    order_id_tag="001",
)

# Instantiate the node passing a list of strategies and configuration
node = TradingNode(strategies=[strategy], config=config)

# Register your client factories with the node (can take user defined factories)
node.add_data_client_factory("CCXT", CCXTDataClientFactory)
node.add_exec_client_factory("CCXT", CCXTExecutionClientFactory)
node.build()

# Stop and dispose of the node with SIGINT/CTRL+C
if __name__ == "__main__":
    try:
        node.start()
    finally:
        node.dispose()
