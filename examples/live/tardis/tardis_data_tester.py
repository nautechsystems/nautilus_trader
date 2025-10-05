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

from nautilus_trader.adapters.tardis import TARDIS
from nautilus_trader.adapters.tardis import TARDIS_CLIENT_ID
from nautilus_trader.adapters.tardis import TardisDataClientConfig
from nautilus_trader.adapters.tardis import TardisLiveDataClientFactory
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


# Run the following to start the tardis-machine server:
# - The TM_API_KEY environment variable should be set
# - The TARDIS_MACHINE_WS_URL environment variable should be set to ws://localhost:8001
# - docker run -p 8000:8000 -p 8001:8001 -d tardisdev/tardis-machine

instrument_ids = [
    # InstrumentId.from_str("BTCUSDT-PERP.BINANCE"),
    # InstrumentId.from_str("BTCUSD_PERP.BINANCE_DELIVERY"),
    # InstrumentId.from_str("USDTUSD.BINANCE_US"),
    # InstrumentId.from_str("BTCUSDT-SPOT.BYBIT"),
    # InstrumentId.from_str("BTCUSDT-LINEAR.BYBIT"),
    # InstrumentId.from_str("BTCUSDT.BINANCE"),
    InstrumentId.from_str("XBTUSDT.BITMEX"),
    InstrumentId.from_str("ETHUSDT.BITMEX"),
    # InstrumentId.from_str("BTC_USDT.GATE_IO"),
    # InstrumentId.from_str("BTC_USDT-PERP.GATE_IO"),
]

# See supported venues https://nautilustrader.io/docs/nightly/integrations/tardis#venues
venues = {i.venue.value for i in instrument_ids}
filters = {"venues": frozenset(venues)}
instrument_provider_config = InstrumentProviderConfig(load_all=True, filters=filters)

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
        TARDIS: TardisDataClientConfig(
            api_key=None,  # 'TARDIS_API_KEY' env var
            instrument_provider=instrument_provider_config,
        ),
    },
    timeout_connection=60.0,
    timeout_reconciliation=10.0,  # Not applicable
    timeout_portfolio=10.0,
    timeout_disconnection=10.0,
    timeout_post_stop=1.0,
)

# Instantiate the node with a configuration
node = TradingNode(config=config_node)

# Configure and initialize the tester
config_tester = DataTesterConfig(
    client_id=TARDIS_CLIENT_ID,
    instrument_ids=instrument_ids,
    # subscribe_book_deltas=True,
    # subscribe_book_depth=True,
    # subscribe_book_at_interval=True,
    subscribe_quotes=True,
    subscribe_trades=True,
    subscribe_funding_rates=True,
    # subscribe_bars=True,
    book_interval_ms=10,
)
tester = DataTester(config=config_tester)

node.trader.add_actor(tester)

# Register your client factories with the node (can take user-defined factories)
node.add_data_client_factory(TARDIS, TardisLiveDataClientFactory)
node.build()


# Stop and dispose of the node with SIGINT/CTRL+C
if __name__ == "__main__":
    try:
        node.run()
    finally:
        node.dispose()
