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
Polymarket Python v2 data tester example.

The default path builds a live node and attaches the built-in Rust DataTester without
connecting to Polymarket. Pass --run to start subscriptions.

"""

from __future__ import annotations

import argparse

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
DEFAULT_INSTRUMENT = (
    "0xac02cbb049e46d6a3627c0fdf52fa554982a9025d45968207b362acb6ca4b830-"
    f"28239418772633645184924651434956000849078365566842629564562475378531350731731.{POLYMARKET}"
)


def main() -> None:
    args = parse_args()
    instrument_id = InstrumentId.from_str(args.instrument)

    builder = LiveNode.builder(
        "POLYMARKET-DATA-TESTER-001",
        TraderId.from_str(args.trader_id),
        Environment.LIVE,
    ).add_data_client(
        None,
        PolymarketDataClientFactory(),
        PolymarketDataClientConfig(
            instrument_config=PolymarketInstrumentProviderConfig(
                event_slugs=[args.event_slug],
                use_gamma_markets=True,
            ),
        ),
    )

    node = builder.build()
    node.add_builtin_actor(
        "DataTester",
        DataTesterConfig(
            client_id=ClientId.from_str(POLYMARKET),
            instrument_ids=[instrument_id],
            subscribe_trades=True,
            subscribe_quotes=True,
            manage_book=True,
            log_data=True,
        ),
    )

    if args.run:
        node.run()
    else:
        print("Built Polymarket data tester node. Pass --run to connect.")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Build or run the Polymarket Python v2 data tester.",
    )
    parser.add_argument("--trader-id", default="TESTER-001")
    parser.add_argument("--event-slug", default="fed-decision-in-september-762")
    parser.add_argument("--instrument", default=DEFAULT_INSTRUMENT)
    parser.add_argument("--run", action="store_true")
    return parser.parse_args()


if __name__ == "__main__":
    main()
