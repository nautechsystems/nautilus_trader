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
OKX Python v2 data tester example.

The default path builds a live node and attaches the built-in Rust DataTester without
connecting to OKX. Pass --run to start subscriptions.

"""

from __future__ import annotations

import argparse

from nautilus_trader.adapters.okx import OKX
from nautilus_trader.adapters.okx import OKXDataClientConfig
from nautilus_trader.adapters.okx import OKXDataClientFactory
from nautilus_trader.adapters.okx import OKXEnvironment
from nautilus_trader.adapters.okx import OKXInstrumentType
from nautilus_trader.common import Environment
from nautilus_trader.live import LiveNode
from nautilus_trader.model import BarType
from nautilus_trader.model import ClientId
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import TraderId
from nautilus_trader.testkit import DataTesterConfig


def main() -> None:
    args = parse_args()
    okx_environment = OKXEnvironment(args.okx_environment)
    instrument_type = OKXInstrumentType(args.instrument_type.capitalize())
    instrument_id = InstrumentId.from_str(args.instrument)

    builder = LiveNode.builder(
        "OKX-DATA-TESTER-001",
        TraderId.from_str(args.trader_id),
        Environment.LIVE,
    ).add_data_client(
        None,
        OKXDataClientFactory(),
        OKXDataClientConfig(
            instrument_types=[instrument_type],
            environment=okx_environment,
            load_spreads=args.load_spreads,
        ),
    )

    node = builder.build()
    node.add_builtin_actor(
        "DataTester",
        DataTesterConfig(
            client_id=ClientId.from_str(OKX),
            instrument_ids=[instrument_id],
            bar_types=[BarType.from_str(f"{args.instrument}-1-MINUTE-LAST-EXTERNAL")],
            subscribe_book_deltas=True,
            subscribe_quotes=True,
            subscribe_trades=True,
            subscribe_mark_prices=args.subscribe_mark_prices,
            subscribe_index_prices=args.subscribe_index_prices,
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
        print("Built OKX data tester node. Pass --run to connect.")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build or run the OKX Python v2 data tester.")
    parser.add_argument("--okx-environment", choices=["live", "demo"], default="demo")
    parser.add_argument(
        "--instrument-type",
        choices=["spot", "margin", "swap", "futures", "option"],
        default="spot",
    )
    parser.add_argument("--trader-id", default="TESTER-001")
    parser.add_argument("--instrument", default=f"BTC-USDT.{OKX}")
    parser.add_argument("--load-spreads", action="store_true")
    parser.add_argument("--subscribe-mark-prices", action="store_true")
    parser.add_argument("--subscribe-index-prices", action="store_true")
    parser.add_argument(
        "--subscribe-funding-rates",
        action=argparse.BooleanOptionalAction,
        default=False,
    )
    parser.add_argument("--run", action="store_true")
    return parser.parse_args()


if __name__ == "__main__":
    main()
