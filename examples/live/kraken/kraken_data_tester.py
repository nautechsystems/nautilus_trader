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

from nautilus_trader.adapters.kraken import KRAKEN
from nautilus_trader.adapters.kraken import KrakenDataClientConfig
from nautilus_trader.adapters.kraken import KrakenEnvironment
from nautilus_trader.adapters.kraken import KrakenLiveDataClientFactory
from nautilus_trader.adapters.kraken import KrakenProductType
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.data import BarType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.test_kit.strategies.tester_data import DataTester
from nautilus_trader.test_kit.strategies.tester_data import DataTesterConfig


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***

# Configuration - Change product_type to switch between trading modes
product_type = KrakenProductType.FUTURES  # SPOT or FUTURES
token = "ETH"

# Symbol and settings based on product type
if product_type == KrakenProductType.SPOT:
    symbol = f"{token}/USD"
    environment = KrakenEnvironment.MAINNET
elif product_type == KrakenProductType.FUTURES:
    # Kraken Futures perpetual symbols use PI_ prefix (e.g., PI_XBTUSD, PI_ETHUSD)
    symbol = f"PI_{token}USD"
    environment = KrakenEnvironment.MAINNET
    # environment = KrakenEnvironment.DEMO  # Use demo-futures.kraken.com
else:
    raise ValueError(f"Unsupported product type: {product_type}")

instrument_id = InstrumentId.from_str(f"{symbol}.{KRAKEN}")
product_types = (product_type,)

# Configure the trading node
config_node = TradingNodeConfig(
    trader_id=TraderId("TESTER-001"),
    logging=LoggingConfig(
        log_level="INFO",
        # log_level_file="DEBUG",
        use_pyo3=True,
    ),
    exec_engine=LiveExecEngineConfig(
        reconciliation=False,  # Not applicable
    ),
    data_clients={
        KRAKEN: KrakenDataClientConfig(
            api_key=None,  # 'KRAKEN_API_KEY' env var
            api_secret=None,  # 'KRAKEN_API_SECRET' env var
            environment=environment,
            product_types=product_types,
            instrument_provider=InstrumentProviderConfig(load_all=True),
        ),
    },
    timeout_connection=30.0,
    timeout_disconnection=10.0,
    timeout_post_stop=5.0,
)

# Instantiate the node with a configuration
node = TradingNode(config=config_node)

# Configure and initialize the tester
config_tester = DataTesterConfig(
    instrument_ids=[instrument_id],
    bar_types=[BarType.from_str(f"{instrument_id.value}-1-MINUTE-LAST-EXTERNAL")],
    subscribe_instrument=True,
    # subscribe_book_deltas=True,
    # subscribe_book_depth=True,
    # subscribe_book_at_interval=True,
    subscribe_quotes=True,
    subscribe_trades=True,
    subscribe_mark_prices=product_type == KrakenProductType.FUTURES,
    subscribe_index_prices=product_type == KrakenProductType.FUTURES,
    subscribe_bars=True,
    # book_depth=10,
    # book_interval_ms=10,
    # requests_start_delta=pd.Timedelta(days=1),
    # request_bars=True,
    # request_trades=True,
)
tester = DataTester(config=config_tester)

node.trader.add_actor(tester)

# Register your client factories with the node
node.add_data_client_factory(KRAKEN, KrakenLiveDataClientFactory)
node.build()


# Stop and dispose of the node with SIGINT/CTRL+C
if __name__ == "__main__":
    try:
        node.run()
    finally:
        node.dispose()
