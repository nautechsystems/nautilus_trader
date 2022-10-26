#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.adapters.ftx.config import FTXDataClientConfig
from nautilus_trader.adapters.ftx.config import FTXExecClientConfig
from nautilus_trader.adapters.ftx.factories import FTXLiveDataClientFactory
from nautilus_trader.adapters.ftx.factories import FTXLiveExecClientFactory
from nautilus_trader.config import CacheDatabaseConfig
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.examples.strategies.ema_cross_trailing_stop import EMACrossTrailingStop
from nautilus_trader.examples.strategies.ema_cross_trailing_stop import EMACrossTrailingStopConfig
from nautilus_trader.live.node import TradingNode


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***

# *** THIS INTEGRATION IS STILL UNDER CONSTRUCTION. ***
# *** PLEASE CONSIDER IT TO BE IN AN UNSTABLE BETA PHASE AND EXERCISE CAUTION. ***

# Configure the trading node
config_node = TradingNodeConfig(
    trader_id="TESTER-001",
    log_level="INFO",
    exec_engine={
        "reconciliation_lookback_mins": 1440,
    },
    cache_database=CacheDatabaseConfig(type="in-memory"),
    data_clients={
        "FTX": FTXDataClientConfig(
            api_key=None,  # "YOUR_FTX_API_KEY"
            api_secret=None,  # "YOUR_FTX_API_SECRET"
            subaccount=None,  # "YOUR_FTX_SUBACCOUNT"
            us=False,  # If client is for FTX US
            instrument_provider=InstrumentProviderConfig(
                load_all=True,
                log_warnings=False,
            ),
            override_usd=True,  # Use USD with a precision of 8
        ),
    },
    exec_clients={
        "FTX": FTXExecClientConfig(
            api_key=None,  # "YOUR_FTX_API_KEY"
            api_secret=None,  # "YOUR_FTX_API_SECRET"
            subaccount=None,  # "YOUR_FTX_SUBACCOUNT"
            us=False,  # If client is for FTX US
            instrument_provider=InstrumentProviderConfig(
                load_all=True,
                log_warnings=False,
            ),
            override_usd=True,  # Use USD with a precision of 8
        ),
    },
    timeout_connection=5.0,
    timeout_reconciliation=5.0,
    timeout_portfolio=5.0,
    timeout_disconnection=5.0,
    timeout_post_stop=2.0,
)
# Instantiate the node with a configuration
node = TradingNode(config=config_node)

# Configure your strategy
strat_config = EMACrossTrailingStopConfig(
    instrument_id="ETH-PERP.FTX",
    bar_type="ETH-PERP.FTX-15-SECOND-LAST-INTERNAL",
    fast_ema_period=10,
    slow_ema_period=20,
    atr_period=20,
    trailing_atr_multiple=3.0,
    trailing_offset_type="PRICE",
    trigger_type="LAST",
    trade_size=Decimal("0.01"),
    emulation_trigger="NONE",
)
# Instantiate your strategy
strategy = EMACrossTrailingStop(config=strat_config)

# Add your strategies and modules
node.trader.add_strategy(strategy)

# Register your client factories with the node (can take user defined factories)
node.add_data_client_factory("FTX", FTXLiveDataClientFactory)
node.add_exec_client_factory("FTX", FTXLiveExecClientFactory)
node.build()


# Stop and dispose of the node with SIGINT/CTRL+C
if __name__ == "__main__":
    try:
        node.start()
    finally:
        node.dispose()
