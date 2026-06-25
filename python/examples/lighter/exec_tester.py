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
Lighter Python v2 execution tester example.

The default path builds a live node and attaches the native Rust ExecTester without
connecting to Lighter or submitting orders. Pass --run to connect. Pass --live-orders
only when the account is funded and you intend to test live order flow.

"""

from __future__ import annotations

import argparse
from decimal import Decimal

from nautilus_trader.adapters.lighter import LIGHTER
from nautilus_trader.adapters.lighter import LighterDataClientConfig
from nautilus_trader.adapters.lighter import LighterDataClientFactory
from nautilus_trader.adapters.lighter import LighterEnvironment
from nautilus_trader.adapters.lighter import LighterExecClientConfig
from nautilus_trader.adapters.lighter import LighterExecutionClientFactory
from nautilus_trader.common import Environment
from nautilus_trader.live import LiveNode
from nautilus_trader.live import LiveRiskEngineConfig
from nautilus_trader.model import AccountId
from nautilus_trader.model import ClientId
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Quantity
from nautilus_trader.model import StrategyId
from nautilus_trader.model import TimeInForce
from nautilus_trader.model import TraderId
from nautilus_trader.testkit import ExecTesterConfig


def main() -> None:
    args = parse_args()
    lighter_environment = lighter_environment_from_name(args.lighter_environment)
    trader_id = TraderId.from_str(args.trader_id)
    account_id = AccountId.from_str(args.account_id)
    instrument_id = InstrumentId.from_str(args.instrument)
    order_qty = Quantity.from_str(args.quantity)

    builder = (
        LiveNode.builder("LIGHTER-EXEC-TESTER-001", trader_id, Environment.LIVE)
        .with_reconciliation(args.run)
        .with_risk_engine_config(LiveRiskEngineConfig(bypass=True))
        .add_data_client(
            None,
            LighterDataClientFactory(),
            LighterDataClientConfig(environment=lighter_environment),
        )
        .add_exec_client(
            None,
            LighterExecutionClientFactory(),
            LighterExecClientConfig(
                trader_id=trader_id,
                account_id=account_id,
                environment=lighter_environment,
            ),
        )
    )

    node = builder.build()
    node.add_builtin_strategy(
        "ExecTester",
        ExecTesterConfig(
            strategy_id=StrategyId.from_str("EXEC_TESTER-001"),
            instrument_id=instrument_id,
            client_id=ClientId.from_str(LIGHTER),
            external_order_claims=[instrument_id],
            order_qty=order_qty,
            subscribe_quotes=True,
            subscribe_trades=False,
            open_position_on_start_qty=Decimal(args.quantity) if args.live_orders else None,
            open_position_on_first_quote=args.live_orders,
            open_position_time_in_force=TimeInForce.IOC,
            enable_limit_buys=args.live_orders,
            enable_limit_sells=False,
            use_post_only=True,
            dry_run=not args.live_orders,
            log_data=False,
        ),
    )

    if args.run:
        node.run()
    else:
        print("Built Lighter exec tester node. Pass --run to connect.")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build or run the Lighter Python v2 exec tester.")
    parser.add_argument("--lighter-environment", choices=["testnet", "mainnet"], default="testnet")
    parser.add_argument("--trader-id", default="TESTER-001")
    parser.add_argument("--account-id", default="LIGHTER-001")
    parser.add_argument("--instrument", default=f"DOGE-PERP.{LIGHTER}")
    parser.add_argument("--quantity", default="200")
    parser.add_argument("--run", action="store_true")
    parser.add_argument("--live-orders", action="store_true")
    return parser.parse_args()


def lighter_environment_from_name(name: str) -> LighterEnvironment:
    if name == "mainnet":
        return LighterEnvironment.MAINNET

    return LighterEnvironment.TESTNET


if __name__ == "__main__":
    main()
