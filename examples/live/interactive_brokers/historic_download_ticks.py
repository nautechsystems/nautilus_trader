#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

import pandas as pd

# fmt: off
from nautilus_trader.adapters.interactive_brokers.config import InteractiveBrokersDataClientConfig
from nautilus_trader.adapters.interactive_brokers.factories import InteractiveBrokersLiveDataClientFactory
from nautilus_trader.adapters.interactive_brokers.factories import InteractiveBrokersLiveExecClientFactory
from nautilus_trader.adapters.interactive_brokers.historic.tick_data import TickDataDownloader
from nautilus_trader.adapters.interactive_brokers.historic.tick_data import TickDataDownloaderConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick


# fmt: on

# *** MAKE SURE YOU HAVE REQUIRED DATA SUBSCRIPTION FOR THIS WORK WORK AS INTENDED. ***

df = pd.DataFrame()


# Data Handler for TickDataDownloader
def do_something_with_ticks(ticks: list):
    global df
    ticks_dict = [
        TradeTick.to_dict(tick)
        if tick.__class__.__name__ == "TradeTick"
        else QuoteTick.to_dict(tick)
        for tick in ticks
    ]
    df = pd.concat([df, pd.DataFrame(ticks_dict)])
    df = df.sort_values(by="ts_init")


# Configure the trading node
config_node = TradingNodeConfig(
    trader_id="TESTER-001",
    logging=LoggingConfig(log_level="INFO"),
    data_clients={
        "InteractiveBrokers": InteractiveBrokersDataClientConfig(
            ibg_host="127.0.0.1",
            ibg_port=4002,
            ibg_client_id=1,
        ),
    },
    timeout_connection=90.0,
)

# Instantiate the node with a configuration
node = TradingNode(config=config_node)

# Configure your strategy
downloader_config = TickDataDownloaderConfig(
    start_iso_ts="2023-09-01T00:00:00+00:00",
    end_iso_ts="2023-09-30T00:00:00+00:00",
    instrument_ids=[
        "AAPL.NASDAQ",
    ],
    tick_types=["TRADES"],
    handler=do_something_with_ticks,
    freq="1W",
)

# Instantiate the downloader and add into node
downloader = TickDataDownloader(config=downloader_config)
node.trader.add_actor(downloader)

# Register your client factories with the node (can take user defined factories)
node.add_data_client_factory("InteractiveBrokers", InteractiveBrokersLiveDataClientFactory)
node.add_exec_client_factory("InteractiveBrokers", InteractiveBrokersLiveExecClientFactory)
node.build()

# Stop and dispose of the node with SIGINT/CTRL+C
if __name__ == "__main__":
    try:
        node.run()
    finally:
        node.dispose()

    print(df.head())
