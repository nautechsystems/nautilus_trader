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

from nautilus_trader.adapters.bybit import BYBIT
from nautilus_trader.adapters.bybit import BybitDataClientConfig
from nautilus_trader.adapters.bybit import BybitLiveDataClientFactory
from nautilus_trader.adapters.bybit import BybitLiveExecClientFactory
from nautilus_trader.adapters.bybit import BybitProductType
from nautilus_trader.config import InstrumentProviderConfig
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

# SPOT/LINEAR
product_type = BybitProductType.LINEAR
symbol = f"ETHUSDT-{product_type.value.upper()}"
instrument_id = InstrumentId.from_str(f"{symbol}.{BYBIT}")

# INVERSE
# product_type = BybitProductType.INVERSE
# symbol = f"XRPUSD-{product_type.value.upper()}"  # Use for inverse
# trade_size = Decimal("100")  # Use for inverse

# Configure the trading node
config_node = TradingNodeConfig(
    trader_id=TraderId("TESTER-001"),
    logging=LoggingConfig(
        log_level="INFO",
        # log_level_file="DEBUG",
        # log_file_max_size=1_000_000_000,
        use_pyo3=True,
    ),
    data_clients={
        BYBIT: BybitDataClientConfig(
            api_key=None,  # 'BYBIT_API_KEY' env var
            api_secret=None,  # 'BYBIT_API_SECRET' env var
            base_url_http=None,  # Override with custom endpoint
            instrument_provider=InstrumentProviderConfig(load_all=True),
            product_types=[product_type],  # Will load all instruments
            demo=False,  # If client uses the demo API
            testnet=False,  # If client uses the testnet API
            recv_window_ms=5_000,  # Default
        ),
    },
    timeout_connection=20.0,
    timeout_reconciliation=10.0,
    timeout_portfolio=10.0,
    timeout_disconnection=10.0,
    timeout_post_stop=1.0,
)

# Instantiate the node with a configuration
node = TradingNode(config=config_node)

# Configure your strategy
config_tester = DataTesterConfig(
    instrument_ids=[instrument_id],
    bar_types=[BarType.from_str(f"{instrument_id}-1-MINUTE-LAST-EXTERNAL")],
    # subscribe_book=True,
    subscribe_quotes=True,
    subscribe_trades=True,
    subscribe_funding_rates=True,
    subscribe_bars=True,
)

# Instantiate your actor
tester = DataTester(config=config_tester)

# Add your actors and modules
node.trader.add_actor(tester)

# Register your client factories with the node (can take user-defined factories)
node.add_data_client_factory(BYBIT, BybitLiveDataClientFactory)
node.add_exec_client_factory(BYBIT, BybitLiveExecClientFactory)
node.build()


# Stop and dispose of the node with SIGINT/CTRL+C
if __name__ == "__main__":
    try:
        node.run()
    finally:
        node.dispose()
