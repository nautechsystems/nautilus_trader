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
DYdX Python v2 data tester example.

The default path builds a live node and attaches the native Rust DataTester without
connecting to dYdX. Pass --run to start subscriptions.

"""

from __future__ import annotations

import argparse

from nautilus_trader.adapters.dydx import DydxDataClientConfig
from nautilus_trader.adapters.dydx import DydxDataClientFactory
from nautilus_trader.adapters.dydx import DydxNetwork
from nautilus_trader.common import Environment
from nautilus_trader.live import LiveNode
from nautilus_trader.model import ClientId
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import TraderId
from nautilus_trader.testkit import DataTesterConfig


DYDX = "DYDX"


def main() -> None:
    args = parse_args()
    instrument_id = InstrumentId.from_str(args.instrument)

    builder = LiveNode.builder(
        "DYDX-DATA-TESTER-001",
        TraderId.from_str(args.trader_id),
        Environment.LIVE,
    ).add_data_client(
        None,
        DydxDataClientFactory(),
        DydxDataClientConfig(network=DydxNetwork.MAINNET),
    )

    node = builder.build()
    node.add_native_actor(
        "DataTester",
        DataTesterConfig(
            client_id=ClientId.from_str(DYDX),
            instrument_ids=[instrument_id],
            subscribe_book_at_interval=True,
            book_interval_ms=args.book_interval_ms,
            manage_book=True,
            log_data=True,
        ),
    )

    if args.run:
        node.run()
    else:
        print("Built dYdX data tester node. Pass --run to connect.")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build or run the dYdX Python v2 data tester.")
    parser.add_argument("--trader-id", default="TESTER-001")
    parser.add_argument("--instrument", default=f"BTC-USD-PERP.{DYDX}")
    parser.add_argument("--book-interval-ms", type=int, default=10)
    parser.add_argument("--run", action="store_true")
    return parser.parse_args()


if __name__ == "__main__":
    main()
