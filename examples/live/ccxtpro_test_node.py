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

from examples.strategies.ema_cross_simple import EMACross
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
        "name": "TESTER",  # Not sent beyond system boundary
        "id_tag": "001",   # Used to ensure orders are unique for this trader
    },

    "logging": {
        "log_level_console": "INF",
        "log_level_file": "DGB",
        "log_level_store": "WRN",
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

    "data_clients": {
        "ccxt-binance": {
            "api_key": "BINANCE_API_KEY",        # value is the environment variable name
            "api_secret": "BINANCE_API_SECRET",  # value is the environment variable name
        },
    },

    "exec_clients": {
        "ccxt-binance": {
            "api_key": "BINANCE_API_KEY",        # value is the environment variable name
            "api_secret": "BINANCE_API_SECRET",  # value is the environment variable name
        },
    }
}


# Instantiate your strategies to pass into the trading node. You could add
# custom options into the configuration file or even use another configuration
# file.

# BarSpecification option examples:
tick_bars = BarSpecification(100, BarAggregation.TICK, PriceType.LAST)
time_bars = BarSpecification(1, BarAggregation.MINUTE, PriceType.LAST)

strategy1 = EMACross(
    symbol=Symbol("BTC/USDT", Venue("BINANCE")),
    bar_spec=time_bars,
    fast_ema=10,
    slow_ema=20,
    trade_size=Decimal("0.001"),
)

strategy2 = EMACross(
    symbol=Symbol("ETH/USDT", Venue("BINANCE")),
    bar_spec=time_bars,
    fast_ema=10,
    slow_ema=20,
    trade_size=Decimal("0.1"),
)

# Instantiate the node passing a list of strategies and configuration
node = TradingNode(
    strategies=[strategy1, strategy2],
    config=config,
)


# Stop and dispose of the node with SIGINT/CTRL+C
if __name__ == "__main__":
    try:
        node.start()
    finally:
        node.dispose()
