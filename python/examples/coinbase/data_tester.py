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
Coinbase Python v2 data tester example.

The default path builds a live node and attaches the native Rust DataTester without
connecting to Coinbase. Pass --run to start subscriptions.

"""

from __future__ import annotations

import argparse

from nautilus_trader.adapters.coinbase import COINBASE
from nautilus_trader.adapters.coinbase import CoinbaseDataClientConfig
from nautilus_trader.adapters.coinbase import CoinbaseDataClientFactory
from nautilus_trader.adapters.coinbase import CoinbaseEnvironment
from nautilus_trader.common import Environment
from nautilus_trader.live import LiveNode
from nautilus_trader.model import BarType
from nautilus_trader.model import ClientId
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import TraderId
from nautilus_trader.testkit import DataTesterConfig


def main() -> None:
    args = parse_args()
    coinbase_environment = coinbase_environment_from_name(args.coinbase_environment)
    instrument_id = InstrumentId.from_str(args.instrument)

    builder = LiveNode.builder(
        "COINBASE-DATA-TESTER-001",
        TraderId.from_str(args.trader_id),
        Environment.LIVE,
    ).add_data_client(
        None,
        CoinbaseDataClientFactory(),
        CoinbaseDataClientConfig(environment=coinbase_environment),
    )

    node = builder.build()
    node.add_native_actor(
        "DataTester",
        DataTesterConfig(
            client_id=ClientId.from_str(COINBASE),
            instrument_ids=[instrument_id],
            bar_types=[BarType.from_str(f"{args.instrument}-1-MINUTE-LAST-EXTERNAL")],
            subscribe_book_deltas=True,
            subscribe_quotes=True,
            subscribe_trades=True,
            subscribe_funding_rates=args.subscribe_funding_rates,
            request_instruments=True,
            request_trades=True,
            request_bars=True,
            request_book_snapshot=True,
            request_funding_rates=args.subscribe_funding_rates,
            manage_book=True,
            log_data=True,
        ),
    )

    if args.run:
        node.run()
    else:
        print("Built Coinbase data tester node. Pass --run to connect.")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build or run the Coinbase Python v2 data tester.")
    parser.add_argument("--coinbase-environment", choices=["live", "sandbox"], default="live")
    parser.add_argument("--trader-id", default="TESTER-001")
    parser.add_argument("--instrument", default=f"BTC-USD.{COINBASE}")
    parser.add_argument(
        "--subscribe-funding-rates",
        action=argparse.BooleanOptionalAction,
        default=False,
    )
    parser.add_argument("--run", action="store_true")
    return parser.parse_args()


def coinbase_environment_from_name(name: str) -> CoinbaseEnvironment:
    if name == "sandbox":
        return CoinbaseEnvironment.SANDBOX

    return CoinbaseEnvironment.LIVE


if __name__ == "__main__":
    main()
