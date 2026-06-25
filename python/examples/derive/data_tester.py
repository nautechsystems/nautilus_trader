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
Derive Python v2 data tester example.

The default path builds a live node and attaches the built-in Rust DataTester without
connecting to Derive. Pass --run to start subscriptions.

"""

from __future__ import annotations

import argparse

from nautilus_trader.adapters.derive import DERIVE
from nautilus_trader.adapters.derive import DeriveDataClientConfig
from nautilus_trader.adapters.derive import DeriveDataClientFactory
from nautilus_trader.adapters.derive import DeriveEnvironment
from nautilus_trader.common import Environment
from nautilus_trader.live import LiveNode
from nautilus_trader.model import BarType
from nautilus_trader.model import ClientId
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import TraderId
from nautilus_trader.testkit import DataTesterConfig


def main() -> None:
    args = parse_args()
    derive_environment = derive_environment_from_name(args.derive_environment)
    instrument_id = InstrumentId.from_str(args.instrument)
    currency = args.currency or args.instrument.split("-", maxsplit=1)[0]

    builder = LiveNode.builder(
        "DERIVE-DATA-TESTER-001",
        TraderId.from_str(args.trader_id),
        Environment.LIVE,
    ).add_data_client(
        None,
        DeriveDataClientFactory(),
        DeriveDataClientConfig(
            environment=derive_environment,
            currencies=[currency],
        ),
    )

    node = builder.build()
    node.add_builtin_actor(
        "DataTester",
        DataTesterConfig(
            client_id=ClientId.from_str(DERIVE),
            instrument_ids=[instrument_id],
            bar_types=[BarType.from_str(f"{args.instrument}-1-MINUTE-LAST-EXTERNAL")],
            subscribe_book_deltas=True,
            subscribe_quotes=True,
            subscribe_trades=True,
            subscribe_funding_rates=args.subscribe_funding_rates,
            subscribe_option_greeks=args.subscribe_option_greeks,
            request_instruments=True,
            request_trades=True,
            request_bars=True,
            request_funding_rates=args.subscribe_funding_rates,
            manage_book=True,
            log_data=True,
        ),
    )

    if args.run:
        node.run()
    else:
        print("Built Derive data tester node. Pass --run to connect.")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build or run the Derive Python v2 data tester.")
    parser.add_argument("--derive-environment", choices=["testnet", "mainnet"], default="testnet")
    parser.add_argument("--trader-id", default="TESTER-001")
    parser.add_argument("--instrument", default=f"ETH-PERP.{DERIVE}")
    parser.add_argument("--currency", default=None)
    parser.add_argument(
        "--subscribe-funding-rates",
        action=argparse.BooleanOptionalAction,
        default=True,
    )
    parser.add_argument("--subscribe-option-greeks", action="store_true")
    parser.add_argument("--run", action="store_true")
    return parser.parse_args()


def derive_environment_from_name(name: str) -> DeriveEnvironment:
    if name == "mainnet":
        return DeriveEnvironment.MAINNET

    return DeriveEnvironment.TESTNET


if __name__ == "__main__":
    main()
