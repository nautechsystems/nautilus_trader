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
Stream Kalshi demo market data with the built-in DataTester actor.

Running this example connects to the Kalshi demo environment and subscribes to quotes,
trades, full L2 books, trading status, and final market closes for two markets in one
event. It also requests a book snapshot and the last hour of trades for each market, and
the last hour of one-minute candles for the first market. No orders are placed.

Edit the instrument constants to target open demo markets.

Demo credentials are read from the environment when both config fields are absent:

- ``KALSHI_DEMO_API_KEY_ID``
- ``KALSHI_DEMO_PRIVATE_KEY``

"""

from __future__ import annotations

from nautilus_trader.adapters.kalshi import KALSHI
from nautilus_trader.adapters.kalshi import KalshiDataClientConfig
from nautilus_trader.adapters.kalshi import KalshiDataClientFactory
from nautilus_trader.adapters.kalshi import KalshiEnvironment
from nautilus_trader.common import Environment
from nautilus_trader.live import LiveNode
from nautilus_trader.model import BarType
from nautilus_trader.model import ClientId
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import TraderId
from nautilus_trader.testkit import DataTesterConfig


TRADER_ID = TraderId.from_str("TESTER-001")
ENVIRONMENT = KalshiEnvironment.DEMO
INSTRUMENT_ID = InstrumentId.from_str(f"KXNBA1HTOTAL-26NOV01LACGSW-125.{KALSHI}")
SECOND_INSTRUMENT_ID = InstrumentId.from_str(f"KXNBA1HTOTAL-26NOV01LACGSW-97.{KALSHI}")
INSTRUMENT_IDS = [INSTRUMENT_ID, SECOND_INSTRUMENT_ID]
BAR_TYPE = BarType.from_str(f"{INSTRUMENT_ID}-1-MINUTE-LAST-EXTERNAL")


def main() -> None:
    """
    Run the example.
    """
    node = (
        LiveNode.builder("KALSHI-DATA-TESTER-001", TRADER_ID, Environment.LIVE)
        .add_data_client(
            None,
            KalshiDataClientFactory(),
            KalshiDataClientConfig(
                environment=ENVIRONMENT,
                instrument_ids=INSTRUMENT_IDS,
            ),
        )
        .build()
    )
    node.add_builtin_actor(
        "DataTester",
        DataTesterConfig(
            client_id=ClientId.from_str(KALSHI),
            instrument_ids=INSTRUMENT_IDS,
            bar_types=[BAR_TYPE],
            subscribe_book_deltas=True,
            subscribe_quotes=True,
            subscribe_trades=True,
            subscribe_instrument_status=True,
            subscribe_instrument_close=True,
            request_trades=True,
            request_bars=True,
            request_book_snapshot=True,
            manage_book=True,
            log_data=True,
        ),
    )
    node.run()


if __name__ == "__main__":
    main()
