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
Derive Python v2 execution tester example.

The default path builds a live node and attaches the native Rust ExecTester without
connecting to Derive or submitting orders. Pass --run to connect, and pass --live-orders
only when the account is funded and you intend to test live order flow.

"""

from __future__ import annotations

import argparse
from decimal import Decimal

from nautilus_trader.adapters.derive import DERIVE
from nautilus_trader.adapters.derive import DeriveDataClientConfig
from nautilus_trader.adapters.derive import DeriveDataClientFactory
from nautilus_trader.adapters.derive import DeriveEnvironment
from nautilus_trader.adapters.derive import DeriveExecClientConfig
from nautilus_trader.adapters.derive import DeriveExecFactoryConfig
from nautilus_trader.adapters.derive import DeriveExecutionClientFactory
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


SMOKE_WALLET_ADDRESS = "0x0000000000000000000000000000000000000001"
SMOKE_SESSION_KEY = "0x0000000000000000000000000000000000000000000000000000000000000001"


def main() -> None:
    args = parse_args()
    derive_environment = derive_environment_from_name(args.derive_environment)
    trader_id = TraderId.from_str(args.trader_id)
    instrument_id = InstrumentId.from_str(args.instrument)
    order_qty = Quantity.from_str(args.quantity)
    currency = args.currency or args.instrument.split("-", maxsplit=1)[0]

    exec_config = DeriveExecClientConfig(
        environment=derive_environment,
        max_fee_per_contract=Decimal(args.max_fee_per_contract),
        wallet_address=None if args.run else SMOKE_WALLET_ADDRESS,
        session_key=None if args.run else SMOKE_SESSION_KEY,
        subaccount_id=None if args.run else 0,
    )

    builder = (
        LiveNode.builder("DERIVE-EXEC-TESTER-001", trader_id, Environment.LIVE)
        .with_reconciliation(args.run)
        .with_risk_engine_config(LiveRiskEngineConfig(bypass=True))
        .add_data_client(
            None,
            DeriveDataClientFactory(),
            DeriveDataClientConfig(
                environment=derive_environment,
                currencies=[currency],
            ),
        )
        .add_exec_client(
            None,
            DeriveExecutionClientFactory(),
            DeriveExecFactoryConfig(
                trader_id=trader_id,
                account_id=AccountId.from_str(args.account_id),
                config=exec_config,
            ),
        )
    )

    node = builder.build()
    node.add_native_strategy(
        ExecTesterConfig(
            strategy_id=StrategyId.from_str("EXEC_TESTER-001"),
            instrument_id=instrument_id,
            client_id=ClientId.from_str(DERIVE),
            external_order_claims=[instrument_id],
            order_qty=order_qty,
            subscribe_quotes=True,
            subscribe_trades=True,
            open_position_on_start_qty=Decimal(args.quantity) if args.live_orders else None,
            open_position_on_first_quote=args.live_orders,
            open_position_time_in_force=TimeInForce.IOC,
            enable_limit_buys=args.live_orders,
            enable_limit_sells=args.live_orders,
            use_post_only=True,
            dry_run=not args.live_orders,
            log_data=False,
        ),
    )

    if args.run:
        node.run()
    else:
        print("Built Derive exec tester node. Pass --run to connect.")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build or run the Derive Python v2 exec tester.")
    parser.add_argument("--derive-environment", choices=["testnet", "mainnet"], default="testnet")
    parser.add_argument("--trader-id", default="TESTER-001")
    parser.add_argument("--account-id", default="DERIVE-001")
    parser.add_argument("--instrument", default=f"ETH-PERP.{DERIVE}")
    parser.add_argument("--currency", default=None)
    parser.add_argument("--quantity", default="0.1")
    parser.add_argument("--max-fee-per-contract", default="1000")
    parser.add_argument("--run", action="store_true")
    parser.add_argument("--live-orders", action="store_true")
    return parser.parse_args()


def derive_environment_from_name(name: str) -> DeriveEnvironment:
    if name == "mainnet":
        return DeriveEnvironment.MAINNET

    return DeriveEnvironment.TESTNET


if __name__ == "__main__":
    main()
