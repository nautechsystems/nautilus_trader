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
import asyncio
import datetime
import os
import pathlib
import sys
from decimal import Decimal
from functools import partial

import betfairlightweight
import orjson

from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import LiveLogger
from nautilus_trader.persistence.streaming import FeatherWriter
from tests.test_kit.strategies import RepeatedOrders


sys.path.insert(
    0, str(os.path.abspath(__file__ + "/../../../"))
)  # Allows relative imports from examples

from nautilus_trader.adapters.betfair.factory import BetfairLiveDataClientFactory
from nautilus_trader.adapters.betfair.factory import BetfairLiveExecutionClientFactory
from nautilus_trader.live.node import TradingNode


# The configuration dictionary can come from anywhere such as a JSON or YAML
# file. Here it is hardcoded into the example for clarity.
market_id = "1.186182608"
config = {
    "trader": {
        "name": "TESTER",  # Not sent beyond system boundary
        "id_tag": "001",  # Used to ensure orders are unique for this trader
    },
    "system": {
        "loop_debug": False,  # If event loop debug mode
        "timeout_connection": 30.0,  # Timeout for all clients to connect and initialize
        "timeout_reconciliation": 10.0,  # Timeout for execution state to reconcile
        "timeout_portfolio": 10.0,  # Timeout for portfolio to initialize margins and unrealized PnLs
        "timeout_disconnection": 1.0,  # Timeout for all engine clients to disconnect
        "check_residuals_delay": 1.0,  # Delay to await residual events after stopping engines
    },
    "logging": {
        "level_stdout": "DBG",
    },
    "database": {
        "type": "in-memory",
    },
    "data_engine": {},
    "risk_engine": {},
    "exec_engine": {},
    "strategy": {
        "load_state": True,  # Strategy state is loaded from the database on start
        "save_state": True,  # Strategy state is saved to the database on shutdown
    },
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
            "base_currency": "AUD",
            "market_filter": {"market_id": market_id},
        },
    },
}
os.environ.update(
    {
        "BETFAIR_USERNAME": "Scholarship2021E",
        "BETFAIR_PW": "dRlkj2756!",
        "BETFAIR_APP_KEY": "q6VebE8rN7ZYra1w",
    }
)

# Find instruments
client = betfairlightweight.APIClient(
    username=os.getenv("BETFAIR_USERNAME"),
    password=os.getenv("BETFAIR_PW"),
    app_key=os.getenv("BETFAIR_APP_KEY"),
    certs=os.getenv("BETFAIR_CERT_DIR"),
    lightweight=True,
)
client.login()
logger = LiveLogger(loop=asyncio.get_event_loop(), clock=LiveClock())

provider = BetfairInstrumentProvider(
    client=client,
    logger=logger,
    market_filter={"market_id": market_id},
)
all_instruments = provider.list_instruments()

# Instantiate your strategies to pass into the trading node. You could add
# custom options into the configuration file or even use another configuration
# file.


strategy = RepeatedOrders(
    instrument_id=all_instruments[0].id,
    trade_size=Decimal(10.0),
)

# Instantiate the node passing a list of strategies and configuration
node = TradingNode(config=config)  # type: ignore

# Setup persistence
now = datetime.datetime.now().strftime("%Y%m%d_%H%M%S_%f")
name = f"betfair-order-tests-{now}"
root = pathlib.Path(os.environ["NAUTILUS_DATA"])
live_folder = root.joinpath("live")
writer = FeatherWriter(path=f"{live_folder}/{name}.feather")
node.trader.subscribe("*", writer.write)


# Setup logging
def sink(record, f):
    f.write(orjson.dumps(record) + b"\n")


log_sink = open(f"{root}/logs/{name}.log", "wb")
node.get_logger().register_sink(partial(sink, f=log_sink))

# Add your strategies and modules
node.trader.add_strategy(strategy)

# Register your client factories with the node (can take user defined factories)
node.add_data_client_factory("BETFAIR", BetfairLiveDataClientFactory)
node.add_exec_client_factory("BETFAIR", BetfairLiveExecutionClientFactory)
node.build()


# Stop and dispose of the node with SIGINT/CTRL+C

if __name__ == "__main__":
    #
    # tracer = VizTracer(
    #     output_file="profile.json",
    #     exclude_files=["/Users/bradleymcelroy/.pyenv/versions/3.9.6/lib/python3.9/"],
    #     tracer_entries=3_000_000
    # )

    try:
        # with tracer:
        node.start()
    finally:
        node.dispose()
