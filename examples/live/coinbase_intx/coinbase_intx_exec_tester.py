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

# This example demonstrates using the ExecTester strategy for testing
# execution functionality with Coinbase International Exchange.
# The strategy places limit orders at a specified offset from the market.

from decimal import Decimal

from nautilus_trader.adapters.coinbase_intx import COINBASE_INTX
from nautilus_trader.adapters.coinbase_intx import CoinbaseIntxDataClientConfig
from nautilus_trader.adapters.coinbase_intx import CoinbaseIntxExecClientConfig
from nautilus_trader.adapters.coinbase_intx import CoinbaseIntxLiveDataClientFactory
from nautilus_trader.adapters.coinbase_intx import CoinbaseIntxLiveExecClientFactory
from nautilus_trader.cache.config import CacheConfig
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.test_kit.strategies.tester_exec import ExecTester
from nautilus_trader.test_kit.strategies.tester_exec import ExecTesterConfig


instrument_id = InstrumentId.from_str("BTC-PERP.COINBASE_INTX")
offset_ticks = 10  # Number of ticks to offset limit orders from the market
trade_size = Decimal("0.01")
dry_run = False  # Set this to True to prevent trading

# Configure the trading node
config_node = TradingNodeConfig(
    trader_id=TraderId("TESTER-001"),
    logging=LoggingConfig(log_level="INFO", use_pyo3=True),
    exec_engine=LiveExecEngineConfig(
        reconciliation=True,
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
    exec_clients={
        COINBASE_INTX: CoinbaseIntxExecClientConfig(
            instrument_provider=InstrumentProviderConfig(load_all=True),
        ),
    },
    timeout_connection=60.0,
    timeout_reconciliation=20.0,
    timeout_portfolio=10.0,
    timeout_disconnection=5.0,
    timeout_post_stop=5.0,
)

# Instantiate the node with a configuration
node = TradingNode(config=config_node)

# Configure your strategy
config_tester = ExecTesterConfig(
    use_uuid_client_order_ids=True,  # <-- Required for Coinbase Intx
    instrument_id=instrument_id,
    external_order_claims=[instrument_id],
    order_qty=trade_size,
    tob_offset_ticks=offset_ticks,
    subscribe_quotes=True,
    subscribe_trades=True,
    use_post_only=True,
    close_positions_time_in_force=TimeInForce.IOC,  # <-- Required for Coinbase Intx
    dry_run=dry_run,
    log_data=True,
)

# Instantiate your strategy
strategy = ExecTester(config=config_tester)

# Add your strategies and modules
node.trader.add_strategy(strategy)

# Register your client factories with the node (can take user-defined factories)
node.add_data_client_factory(COINBASE_INTX, CoinbaseIntxLiveDataClientFactory)
node.add_exec_client_factory(COINBASE_INTX, CoinbaseIntxLiveExecClientFactory)
node.build()


# Stop and dispose of the node with SIGINT/CTRL+C
if __name__ == "__main__":
    try:
        node.run()
    finally:
        node.dispose()
