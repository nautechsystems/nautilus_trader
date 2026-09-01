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
Stream Polymarket market data with the built-in DataTester actor.

Running this example connects to Polymarket and starts subscriptions for the configured
instrument immediately, logging all received data. No orders are placed.

"""

from __future__ import annotations

from nautilus_trader.adapters.polymarket import PolymarketDataClientConfig
from nautilus_trader.adapters.polymarket import PolymarketDataClientFactory
from nautilus_trader.adapters.polymarket import PolymarketInstrumentProviderConfig
from nautilus_trader.common import Environment
from nautilus_trader.live import LiveNode
from nautilus_trader.model import ClientId
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import TraderId
from nautilus_trader.testkit import DataTesterConfig


POLYMARKET = "POLYMARKET"
TRADER_ID = TraderId.from_str("TESTER-001")
EVENT_SLUG = "fed-decision-in-september-762"
INSTRUMENT_ID = InstrumentId.from_str(
    "0xac02cbb049e46d6a3627c0fdf52fa554982a9025d45968207b362acb6ca4b830-"
    f"28239418772633645184924651434956000849078365566842629564562475378531350731731.{POLYMARKET}",
)


def main() -> None:
    """
    Run the example.
    """
    node = (
        LiveNode.builder("POLYMARKET-DATA-TESTER-001", TRADER_ID, Environment.LIVE)
        .add_data_client(
            None,
            PolymarketDataClientFactory(),
            PolymarketDataClientConfig(
                instrument_config=PolymarketInstrumentProviderConfig(
                    event_slugs=[EVENT_SLUG],
                    use_gamma_markets=True,
                ),
                update_instruments_interval_mins=1,
            ),
        )
        .build()
    )
    node.add_builtin_actor(
        "DataTester",
        DataTesterConfig(
            client_id=ClientId.from_str(POLYMARKET),
            instrument_ids=[INSTRUMENT_ID],
            subscribe_trades=True,
            subscribe_quotes=True,
            subscribe_instrument=True,
            manage_book=True,
            log_data=True,
        ),
    )

    node.run()


if __name__ == "__main__":
    main()
