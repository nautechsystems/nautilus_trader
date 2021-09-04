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

import os
import sys
from decimal import Decimal


sys.path.insert(
    0, str(os.path.abspath(__file__ + "/../../../"))
)  # Allows relative imports from examples

from examples.strategies.ema_cross_simple import EMACross
from examples.strategies.ema_cross_simple import EMACrossConfig
from nautilus_trader.adapters.ccxt.factories import CCXTDataClientFactory
from nautilus_trader.adapters.ccxt.factories import CCXTExecutionClientFactory
from nautilus_trader.infrastructure.cache import CacheDatabaseConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.live.node import TradingNodeConfig


# Configure the trading node
config_node = TradingNodeConfig(
    trader_id="TESTER-001",
    log_level="INFO",
    cache_database=CacheDatabaseConfig(),  # Redis by default if provided
    data_clients={
        "CCXT-BINANCE": {
            "account_id": "BINANCE_ACCOUNT_ID",  # value is the environment variable key
            "api_key": "BINANCE_API_KEY",  # value is the environment variable key
            "api_secret": "BINANCE_API_SECRET",  # value is the environment variable key
            "sandbox_mode": False,  # If client uses the testnet
        },
    },
    exec_clients={
        "CCXT-BINANCE": {
            "account_id": "BINANCE_ACCOUNT_ID",  # value is the environment variable key
            "api_key": "BINANCE_API_KEY",  # value is the environment variable key
            "api_secret": "BINANCE_API_SECRET",  # value is the environment variable key
            "sandbox_mode": False,  # If client uses the testnet,
        },
    },
)
# Instantiate the node with a configuration
node = TradingNode(config=config_node)

# Configure your strategy
strat_config = EMACrossConfig(
    instrument_id="ETH/USDT.BINANCE",
    bar_type="ETH/USDT.BINANCE-1-MINUTE-LAST-INTERNAL",
    fast_ema_period=10,
    slow_ema_period=20,
    trade_size=Decimal("0.01"),
    order_id_tag="001",
)
# Instantiate your strategy
strategy = EMACross(config=strat_config)

# Add your strategies and modules
node.trader.add_strategy(strategy)

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
