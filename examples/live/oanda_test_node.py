#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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
import threading
import time

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
        "oanda": {
            "api_token": "OANDA_API_TOKEN",    # value is the environment variable name
            "account_id": "OANDA_ACCOUNT_ID",  # value is the environment variable name
        },
    },

    "exec_clients": {
        "oanda": {
            "api_token": "OANDA_API_TOKEN",    # value is the environment variable name
            "account_id": "OANDA_ACCOUNT_ID",  # value is the environment variable name
        },
    }
}


# Instantiate your strategies to pass into the trading node. You could add
# custom options into the configuration file or even use another configuration
# file.
strategy1 = EMACross(
    symbol=Symbol("AUD/USD", Venue("OANDA")),
    bar_spec=BarSpecification(1, BarAggregation.MINUTE, PriceType.MID),
    fast_ema=10,
    slow_ema=20,
    trade_size=Decimal(10000),
)

strategy2 = EMACross(
    symbol=Symbol("EUR/USD", Venue("OANDA")),
    bar_spec=BarSpecification(1, BarAggregation.MINUTE, PriceType.MID),
    fast_ema=10,
    slow_ema=20,
    trade_size=Decimal(10000),
)

strategy3 = EMACross(
    symbol=Symbol("GBP/USD", Venue("OANDA")),
    bar_spec=BarSpecification(1, BarAggregation.MINUTE, PriceType.MID),
    fast_ema=10,
    slow_ema=20,
    trade_size=Decimal(10000),
)

# Instantiate the node passing a list of strategies and configuration
node = TradingNode(
    strategies=[strategy1, strategy2, strategy3],
    config=config,
)


def run_node():
    # Run node in a background thread
    run = threading.Thread(target=node.start, daemon=True)
    run.start()


def stop_and_dispose_node():
    # Get the event loop for the node
    loop = node.get_event_loop()
    loop.call_soon_threadsafe(node.stop)
    loop.call_soon_threadsafe(node.shutdown)

    time.sleep(5)  # Hard coded shutdown time-out
    node.dispose()


if __name__ == "__main__":
    run_node()

    # Stop and dispose of the node by pressing any key or SIGINT/CTRL+C
    input()
    stop_and_dispose_node()
