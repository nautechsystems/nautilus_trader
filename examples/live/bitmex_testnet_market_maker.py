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

from examples.strategies.volatility_market_maker import VolatilityMarketMaker
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.bar import BarSpecification
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue


# The configuration dictionary can come from anywhere such as a JSON or YAML
# file. Here it is hardcoded into the example for clarity.
config = {
    "trader": {
        "name": "TESTER",              # Not sent beyond system boundary
        "id_tag": "001",               # Used to ensure orders are unique for this trader
        "check_residuals_delay": 5.0,  # How long to wait after stopping for residual events (secs)
    },

    "logging": {
        "log_level_console": "INF",
        "log_level_file": "DBG",
        "log_level_store": "WRN",
        "log_thread_id": False,
        "log_to_file": False,
        "log_file_path": "logs/",
    },

    "exec_database": {
        "type": "redis",
        "host": "localhost",
        "port": 6379,
    },

    "strategy": {
        "load_state": True,  # Strategy state is loaded from the database on start
        "save_state": True,  # Strategy state is saved to the database on shutdown
    },

    "adapters": {
        "ccxt-bitmex": {
            "data_client": True,  # If a data client should be created
            "exec_client": True,  # If a exec client should be created
            "account_id": "BITMEX_ACCOUNT_ID_SANDBOX",  # value is the environment variable key
            "api_key": "BITMEX_API_KEY_SANDBOX",        # value is the environment variable key
            "api_secret": "BITMEX_API_SECRET_SANDBOX",  # value is the environment variable key
            "sandbox_mode": True,  # If clients use the testnet
        },
    },
}


# Instantiate your strategies to pass into the trading node. You could add
# custom options into the configuration file or even use another configuration
# file.
strategy = VolatilityMarketMaker(
    symbol=Symbol("BTC/USD", Venue("BITMEX")),
    bar_spec=BarSpecification(1, BarAggregation.MINUTE, PriceType.LAST),
    trade_size=Decimal("10"),
    atr_period=20,
    atr_multiple=1.5,
    order_id_tag="091",
)

# Instantiate the node passing a list of strategies and configuration
node = TradingNode(strategies=[strategy], config=config)


# Stop and dispose of the node with SIGINT/CTRL+C
if __name__ == "__main__":
    try:
        node.start()
    finally:
        node.dispose()
