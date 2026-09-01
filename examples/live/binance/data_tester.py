#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
Stream Binance market data with the built-in DataTester actor.

Running the script connects to the Binance Spot live environment and starts
subscriptions immediately, logging all received data. No orders are placed.

"""

from __future__ import annotations

from nautilus_trader.adapters.binance import BinanceDataClientConfig
from nautilus_trader.adapters.binance import BinanceDataClientFactory
from nautilus_trader.adapters.binance import BinanceEnvironment
from nautilus_trader.adapters.binance import BinanceProductType
from nautilus_trader.adapters.binance import BinanceSpotMarketDataMode
from nautilus_trader.common import Environment
from nautilus_trader.live import LiveNode
from nautilus_trader.model import ClientId
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import TraderId
from nautilus_trader.testkit import DataTesterConfig


BINANCE = "BINANCE"
TRADER_ID = TraderId.from_str("TESTER-001")
INSTRUMENT_ID = InstrumentId.from_str(f"BTCUSDT.{BINANCE}")
BOOK_INTERVAL_MS = 10


def main() -> None:
    """
    Run the example.
    """
    builder = LiveNode.builder(
        "BINANCE-DATA-TESTER-001",
        TRADER_ID,
        Environment.LIVE,
    ).add_data_client(
        None,
        BinanceDataClientFactory(),
        BinanceDataClientConfig(
            product_type=BinanceProductType.SPOT,
            environment=BinanceEnvironment.LIVE,
            spot_market_data_mode=BinanceSpotMarketDataMode.Json,
        ),
    )

    node = builder.build()
    node.add_builtin_actor(
        "DataTester",
        DataTesterConfig(
            client_id=ClientId.from_str(BINANCE),
            instrument_ids=[INSTRUMENT_ID],
            subscribe_book_at_interval=True,
            book_interval_ms=BOOK_INTERVAL_MS,
            manage_book=True,
            log_data=True,
        ),
    )

    node.run()


if __name__ == "__main__":
    main()
