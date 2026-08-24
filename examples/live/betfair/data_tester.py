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
Stream Betfair market data with the built-in DataTester actor.

Running the script connects to Betfair and starts subscriptions for the configured
market immediately, logging all received data. No orders are placed.

"""

from __future__ import annotations

from nautilus_trader.adapters.betfair import BetfairDataClientConfig
from nautilus_trader.adapters.betfair import BetfairDataClientFactory
from nautilus_trader.common import Environment
from nautilus_trader.live import LiveNode
from nautilus_trader.model import ClientId
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import TraderId
from nautilus_trader.testkit import DataTesterConfig


BETFAIR = "BETFAIR"
TRADER_ID = TraderId.from_str("TESTER-001")
ACCOUNT_CURRENCY = "GBP"
MARKET_ID = "1.234567890"
INSTRUMENT_ID = InstrumentId.from_str(f"1.234567890-123456.{BETFAIR}")
STREAM_CONFLATE_MS = 0


def main() -> None:
    builder = LiveNode.builder(
        "BETFAIR-DATA-TESTER-001",
        TRADER_ID,
        Environment.LIVE,
    ).add_data_client(
        None,
        BetfairDataClientFactory(),
        BetfairDataClientConfig(
            account_currency=ACCOUNT_CURRENCY,
            market_ids=[MARKET_ID],
            stream_conflate_ms=STREAM_CONFLATE_MS,
        ),
    )

    node = builder.build()
    node.add_builtin_actor(
        "DataTester",
        DataTesterConfig(
            client_id=ClientId.from_str(BETFAIR),
            instrument_ids=[INSTRUMENT_ID],
            subscribe_book_deltas=True,
            subscribe_trades=True,
            subscribe_instrument_status=True,
            can_unsubscribe=False,
            manage_book=True,
            log_data=True,
        ),
    )

    node.run()


if __name__ == "__main__":
    main()
