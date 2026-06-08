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
Databento Python v2 data tester example.

The default path builds a live node and attaches the native Rust DataTester without
connecting to Databento. Pass --run to start subscriptions.

"""

from __future__ import annotations

import argparse
from pathlib import Path

from nautilus_trader.adapters.databento import DatabentoDataClientFactory
from nautilus_trader.adapters.databento import DatabentoLiveClientConfig
from nautilus_trader.common import Environment
from nautilus_trader.live import LiveNode
from nautilus_trader.model import ClientId
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import TraderId
from nautilus_trader.testkit import DataTesterConfig


DATABENTO = "DATABENTO"
SMOKE_API_KEY = "00000000000000000000000000000000"


def main() -> None:
    args = parse_args()
    instrument_id = InstrumentId.from_str(args.instrument)

    builder = LiveNode.builder(
        "DATABENTO-DATA-TESTER-001",
        TraderId.from_str(args.trader_id),
        Environment.LIVE,
    ).add_data_client(
        None,
        DatabentoDataClientFactory(),
        DatabentoLiveClientConfig(
            api_key=args.api_key,
            publishers_filepath=args.publishers_filepath,
            use_exchange_as_venue=args.use_exchange_as_venue,
        ),
    )

    node = builder.build()
    node.add_native_actor(
        "DataTester",
        DataTesterConfig(
            client_id=ClientId.from_str(DATABENTO),
            instrument_ids=[instrument_id],
            subscribe_trades=True,
            log_data=True,
        ),
    )

    if args.run:
        node.run()
    else:
        print("Built Databento data tester node. Pass --run to connect.")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Build or run the Databento Python v2 data tester.",
    )
    parser.add_argument("--trader-id", default="TESTER-001")
    parser.add_argument("--instrument", default="BTCUSDT.BINANCE")
    parser.add_argument("--api-key", default=SMOKE_API_KEY)
    parser.add_argument("--publishers-filepath", type=Path, default=publishers_filepath())
    parser.add_argument(
        "--use-exchange-as-venue",
        action=argparse.BooleanOptionalAction,
        default=False,
    )
    parser.add_argument("--run", action="store_true")
    return parser.parse_args()


def publishers_filepath() -> Path:
    return Path(__file__).resolve().parents[3] / "crates/adapters/databento/publishers.json"


if __name__ == "__main__":
    main()
