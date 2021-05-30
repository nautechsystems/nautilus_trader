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

from examples.strategies.ema_cross_simple import EMACross
from nautilus_trader.adapters.oanda.factories import OandaDataClientFactory
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.bar import BarSpecification
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue


# TODO: OandaExecutionClientFactory


# The configuration dictionary can come from anywhere such as a JSON or YAML
# file. Here it is hardcoded into the example for clarity.
config = {
    "trader": {
        "name": "TESTER",  # Not sent beyond system boundary
        "id_tag": "001",  # Used to ensure orders are unique for this trader
    },
    "system": {
        "loop_debug": False,  # If event loop debug mode
        "timeout_connection": 10.0,  # Timeout for all engines client to connect and initialize
        "timeout_reconciliation": 10.0,  # Timeout for execution state to reconcile
        "timeout_portfolio": 10.0,  # Timeout for portfolio to initialize margins and unrealized PnLs
        "timeout_disconnection": 5.0,  # Timeout for all engine clients to disconnect
        "check_residuals_delay": 5.0,  # Delay to await residual events after stopping engines
    },
    "logging": {
        "level_stdout": "INF",
    },
    "exec_database": {
        "type": "redis",
        "host": "localhost",
        "port": 6379,
    },
    "data_engine": {},
    "risk_engine": {},
    "exec_engine": {},
    "strategy": {
        "load_state": True,  # Strategy state is loaded from the database on start
        "save_state": True,  # Strategy state is saved to the database on shutdown
    },
    "data_clients": {
        "OANDA": {
            "api_token": "OANDA_API_TOKEN",  # value is the environment variable key
            "account_id": "OANDA_ACCOUNT_ID",  # value is the environment variable key
        },
    },
    "exec_clients": {
        # "OANDA": {
        #     "api_token": "OANDA_API_TOKEN",  # value is the environment variable key
        #     "account_id": "OANDA_ACCOUNT_ID",  # value is the environment variable key
        # },
    },
}


# Instantiate your strategies to pass into the trading node. You could add
# custom options into the configuration file or even use another configuration
# file.

instrument1 = InstrumentId(
    symbol=Symbol("AUD/USD"),
    venue=Venue("OANDA"),
)

strategy1 = EMACross(
    instrument_id=instrument1,
    bar_spec=BarSpecification(1, BarAggregation.MINUTE, PriceType.MID),
    fast_ema_period=10,
    slow_ema_period=20,
    trade_size=Decimal(10000),
    order_id_tag="001",
)

# ------------------------------------------------------------------------------

instrument2 = InstrumentId(
    symbol=Symbol("EUR/USD"),
    venue=Venue("OANDA"),
)

strategy2 = EMACross(
    instrument_id=instrument2,
    bar_spec=BarSpecification(1, BarAggregation.MINUTE, PriceType.MID),
    fast_ema_period=10,
    slow_ema_period=20,
    trade_size=Decimal(10000),
    order_id_tag="002",
)

# ------------------------------------------------------------------------------

instrument3 = InstrumentId(
    symbol=Symbol("GBP/USD"),
    venue=Venue("OANDA"),
)

strategy3 = EMACross(
    instrument_id=instrument3,
    bar_spec=BarSpecification(1, BarAggregation.MINUTE, PriceType.MID),
    fast_ema_period=10,
    slow_ema_period=20,
    trade_size=Decimal(10000),
    order_id_tag="003",
)

strategies = [
    strategy1,
    strategy2,
    strategy3,
]

# Instantiate the node passing a list of strategies and configuration
node = TradingNode(strategies=strategies, config=config)

# Register your client factories with the node (can take user defined factories)
node.add_data_client_factory("OANDA", OandaDataClientFactory)
node.build()


# Stop and dispose of the node with SIGINT/CTRL+C
if __name__ == "__main__":
    try:
        node.start()
    finally:
        node.dispose()
