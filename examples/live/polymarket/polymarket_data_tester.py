#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.adapters.polymarket import POLYMARKET
from nautilus_trader.adapters.polymarket import PolymarketDataClientConfig
from nautilus_trader.adapters.polymarket import PolymarketLiveDataClientFactory
from nautilus_trader.adapters.polymarket import get_polymarket_instrument_id
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.test_kit.strategies.tester_data import DataTester
from nautilus_trader.test_kit.strategies.tester_data import DataTesterConfig


# For correct subscription operation, you must specify all instruments to be immediately
# subscribed for as part of the data client configuration

# To find active markets run `python nautilus_trader/adapters/polymarket/scripts/active_markets.py`

# x-and-truth-social-merger-announced-before-august
# Active: True
# Condition ID: 0x79d3dd10febe982a33c279ef96ec5521bf73f0e54df3d332d46ebf7ce7221e3c
# Token IDs: 4035065291772644731876334741178162861113899806117080318351467946714817079716, 8807253522691129263460582179245445512612974167513689659640198687172344193269  # noqa
# Link: https://polymarket.com/event/x-and-truth-social-merger-announced-before-august
condition_id = "0x79d3dd10febe982a33c279ef96ec5521bf73f0e54df3d332d46ebf7ce7221e3c"
token_id = "4035065291772644731876334741178162861113899806117080318351467946714817079716"

instrument_ids = [
    get_polymarket_instrument_id(condition_id, token_id),
]

filters = {
    # "next_cursor": "MTE3MDA=",
    "is_active": True,
}

load_ids = [str(x) for x in instrument_ids]
instrument_provider_config = InstrumentProviderConfig(load_ids=frozenset(load_ids))
# instrument_provider_config = InstrumentProviderConfig(load_all=True, filters=filters)

# Configure the trading node
config_node = TradingNodeConfig(
    trader_id=TraderId("TESTER-001"),
    logging=LoggingConfig(log_level="INFO", use_pyo3=True),
    exec_engine=LiveExecEngineConfig(
        reconciliation=False,  # Not applicable
    ),
    data_clients={
        POLYMARKET: PolymarketDataClientConfig(
            private_key=None,  # 'POLYMARKET_PK' env var
            api_key=None,  # 'POLYMARKET_API_KEY' env var
            api_secret=None,  # 'POLYMARKET_API_SECRET' env var
            passphrase=None,  # 'POLYMARKET_PASSPHRASE' env var
            instrument_provider=instrument_provider_config,
            compute_effective_deltas=True,
        ),
    },
    timeout_connection=20.0,
    timeout_disconnection=10.0,
    timeout_post_stop=0.0,  # No stop delay needed for data testing
)

# Instantiate the node with a configuration
node = TradingNode(config=config_node)

# Configure and initialize the tester
config_tester = DataTesterConfig(
    instrument_ids=instrument_ids,
    subscribe_book_deltas=False,
    subscribe_book_at_interval=True,
    subscribe_quotes=True,
    subscribe_trades=True,
    can_unsubscribe=False,  # Polymarket does not support unsubscribing from ws streams
)
tester = DataTester(config=config_tester)

node.trader.add_actor(tester)

# Register your client factories with the node (can take user-defined factories)
node.add_data_client_factory(POLYMARKET, PolymarketLiveDataClientFactory)
node.build()


# Stop and dispose of the node with SIGINT/CTRL+C
if __name__ == "__main__":
    try:
        node.run()
    finally:
        node.dispose()
