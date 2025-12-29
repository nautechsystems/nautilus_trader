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
"""
Demonstrates streaming public market data from Binance without API keys.

This example shows how to use the Binance data client for public market data without
requiring authentication. No API key or secret is needed.

"""

from nautilus_trader.adapters.binance import BINANCE
from nautilus_trader.adapters.binance import BinanceAccountType
from nautilus_trader.adapters.binance import BinanceDataClientConfig
from nautilus_trader.adapters.binance import BinanceLiveDataClientFactory
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.data import BarType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.test_kit.strategies.tester_data import DataTester
from nautilus_trader.test_kit.strategies.tester_data import DataTesterConfig


# Toggle between SPOT and FUTURES
account_type = BinanceAccountType.USDT_FUTURES
# account_type = BinanceAccountType.SPOT

if account_type == BinanceAccountType.SPOT:
    symbol = "BTCUSDT"
else:
    symbol = "BTCUSDT-PERP"

instrument_id = InstrumentId.from_str(f"{symbol}.BINANCE")

# Configure the trading node (data only, no execution)
config_node = TradingNodeConfig(
    trader_id=TraderId("TESTER-001"),
    logging=LoggingConfig(log_level="INFO", use_pyo3=True),
    data_clients={
        BINANCE: BinanceDataClientConfig(
            # No API key required for public market data
            api_key=None,
            api_secret=None,
            account_type=account_type,
            instrument_provider=InstrumentProviderConfig(load_ids=frozenset([instrument_id])),
        ),
    },
    # No exec_clients - data only
    timeout_connection=20.0,
    timeout_disconnection=10.0,
    timeout_post_stop=1.0,
)

# Instantiate the node
node = TradingNode(config=config_node)

# Configure the data tester
config_tester = DataTesterConfig(
    instrument_ids=[instrument_id],
    bar_types=[BarType.from_str(f"{instrument_id}-1-MINUTE-LAST-EXTERNAL")],
    subscribe_instrument=True,
    subscribe_book_at_interval=True,
    # subscribe_book_deltas=True,
    # subscribe_quotes=True,
    # subscribe_trades=True,
    # subscribe_bars=True,
    book_interval_ms=100,
)

# Instantiate the tester actor
tester = DataTester(config=config_tester)

# Add the actor
node.trader.add_actor(tester)

# Register the Binance data client factory
node.add_data_client_factory(BINANCE, BinanceLiveDataClientFactory)
node.build()

# Run the node (stop with SIGINT/CTRL+C)
if __name__ == "__main__":
    try:
        node.run()
    finally:
        node.dispose()
