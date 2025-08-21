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

from nautilus_trader.adapters.coinbase_intx import COINBASE_INTX
from nautilus_trader.adapters.coinbase_intx import CoinbaseIntxDataClientConfig
from nautilus_trader.adapters.coinbase_intx import CoinbaseIntxLiveDataClientFactory
from nautilus_trader.cache.config import CacheConfig
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.test_kit.strategies.tester_data import DataTester
from nautilus_trader.test_kit.strategies.tester_data import DataTesterConfig


# Configure the trading node
config_node = TradingNodeConfig(
    trader_id=TraderId("TESTER-001"),
    logging=LoggingConfig(log_level="INFO", use_pyo3=True),
    exec_engine=LiveExecEngineConfig(
        reconciliation=False,  # Not applicable
        inflight_check_interval_ms=0,  # Not applicable
        # snapshot_orders=True,
        # snapshot_positions=True,
        # snapshot_positions_interval_secs=5.0,
    ),
    cache=CacheConfig(
        # database=DatabaseConfig(),
        encoding="msgpack",
        timestamps_as_iso8601=True,
        buffer_interval_ms=100,
    ),
    # message_bus=MessageBusConfig(
    #     database=DatabaseConfig(),
    #     encoding="json",
    #     timestamps_as_iso8601=True,
    #     buffer_interval_ms=100,
    #     streams_prefix="quoters",
    #     use_instance_id=False,
    #     # types_filter=[QuoteTick],
    #     autotrim_mins=30,
    # ),
    # heartbeat_interval=1.0,
    data_clients={
        COINBASE_INTX: CoinbaseIntxDataClientConfig(
            instrument_provider=InstrumentProviderConfig(load_all=True),
        ),
    },
    timeout_connection=20.0,
    timeout_reconciliation=10.0,  # Not applicable
    timeout_portfolio=10.0,
    timeout_disconnection=10.0,
    timeout_post_stop=0.0,  # Not required as no order state
)

# Instantiate the node with a configuration
node = TradingNode(config=config_node)


instrument_ids = [
    InstrumentId.from_str("ETH-PERP.COINBASE_INTX"),
]

# Configure your actor
config_tester = DataTesterConfig(
    instrument_ids=instrument_ids,
    subscribe_quotes=True,
    subscribe_trades=True,
    subscribe_mark_prices=True,
    subscribe_index_prices=True,
    # subscribe_book_deltas=True,
    # manage_book=True,
    # book_levels_to_print=3,
    # subscribe_book_at_interval=True,
    # book_depth=20,
    # book_interval_ms=1000,
    log_data=True,
)

# Instantiate your actor
data_tester = DataTester(config=config_tester)

# Add your actors and modules
node.trader.add_actor(data_tester)

# Register your client factories with the node (can take user-defined factories)
node.add_data_client_factory(COINBASE_INTX, CoinbaseIntxLiveDataClientFactory)
node.build()


# Stop and dispose of the node with SIGINT/CTRL+C
if __name__ == "__main__":
    try:
        node.run()
    finally:
        node.dispose()
