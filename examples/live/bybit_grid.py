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

from nautilus_trader.adapters.bybit.common.enums import BybitInstrumentType
from nautilus_trader.adapters.bybit.config import BybitDataClientConfig
from nautilus_trader.adapters.bybit.config import BybitExecClientConfig
from nautilus_trader.adapters.bybit.factories import BybitLiveDataClientFactory
from nautilus_trader.adapters.bybit.factories import BybitLiveExecClientFactory
from nautilus_trader.common import Environment
from nautilus_trader.config import CacheConfig
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.config.common import DatabaseConfig
from nautilus_trader.examples.strategies.grid import GridConfig
from nautilus_trader.examples.strategies.grid import GridStrategy
from nautilus_trader.live.node import TradingNode


config_node = TradingNodeConfig(
    trader_id="FILIP-001",
    environment=Environment.LIVE,
    logging=LoggingConfig(log_level="INFO"),
    exec_engine=LiveExecEngineConfig(
        reconciliation=True,
        reconciliation_lookback_mins=1440,
    ),
    cache=CacheConfig(
        database=DatabaseConfig(),
        buffer_interval_ms=100,
    ),
    data_clients={
        "BYBIT": BybitDataClientConfig(
            api_key=None,  # 'BYBIT_API_KEY' env var
            api_secret=None,  # 'BYBIT_API_SECRET' env var
            instrument_types=[BybitInstrumentType.LINEAR],
            testnet=True,
            instrument_provider=InstrumentProviderConfig(load_all=True),
        ),
    },
    exec_clients={
        "BYBIT": BybitExecClientConfig(
            api_key=None,  # 'BYBIT_API_KEY' env var
            api_secret=None,  # 'BYBIT_API_SECRET' env var
            instrument_types=[BybitInstrumentType.LINEAR],
            testnet=True,  # If client uses the testnet
            instrument_provider=InstrumentProviderConfig(load_all=True),
        ),
    },
    timeout_connection=20.0,
    timeout_reconciliation=10.0,
    timeout_portfolio=10.0,
    timeout_disconnection=10.0,
    timeout_post_stop=5.0,
)

node = TradingNode(config=config_node)

instrument_id = "ETHUSDT-LINEAR.BYBIT"
grid_config = GridConfig(
    instrument_id=instrument_id,
    value="test",
)
grid_strategy = GridStrategy(config=grid_config)

node.trader.add_strategy(grid_strategy)
node.add_data_client_factory("BYBIT", BybitLiveDataClientFactory)
node.add_exec_client_factory("BYBIT", BybitLiveExecClientFactory)
node.build()

if __name__ == "__main__":
    try:
        node.run()
    finally:
        node.dispose()
