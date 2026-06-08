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
Deribit Python v2 execution tester example.

The default path builds a live node and attaches the native Rust ExecTester without
connecting to Deribit or submitting orders. Pass --run to connect. Pass --live-orders
only when the account is funded and you intend to test live order flow.

"""

from __future__ import annotations

import argparse
from decimal import Decimal

from nautilus_trader.adapters.deribit import DeribitDataClientConfig
from nautilus_trader.adapters.deribit import DeribitDataClientFactory
from nautilus_trader.adapters.deribit import DeribitEnvironment
from nautilus_trader.adapters.deribit import DeribitExecClientConfig
from nautilus_trader.adapters.deribit import DeribitExecutionClientFactory
from nautilus_trader.adapters.deribit import DeribitProductType
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


DERIBIT = "DERIBIT"
SMOKE_API_KEY = "test_key"
SMOKE_API_SECRET = "test_secret"


def main() -> None:
    args = parse_args()
    trader_id = TraderId.from_str(args.trader_id)
    account_id = AccountId.from_str(args.account_id)
    instrument_id = InstrumentId.from_str(args.instrument)
    order_qty = Quantity.from_str(args.quantity)
    product_types = [DeribitProductType.FUTURE]

    builder = (
        LiveNode.builder("DERIBIT-EXEC-TESTER-001", trader_id, Environment.LIVE)
        .with_reconciliation(args.run)
        .with_risk_engine_config(LiveRiskEngineConfig(bypass=True))
        .add_data_client(
            None,
            DeribitDataClientFactory(),
            DeribitDataClientConfig(
                product_types=product_types,
                environment=DeribitEnvironment.TESTNET,
            ),
        )
        .add_exec_client(
            None,
            DeribitExecutionClientFactory(),
            DeribitExecClientConfig(
                trader_id=trader_id,
                account_id=account_id,
                product_types=product_types,
                environment=DeribitEnvironment.TESTNET,
                api_key=None if args.run else SMOKE_API_KEY,
                api_secret=None if args.run else SMOKE_API_SECRET,
            ),
        )
    )

    node = builder.build()
    node.add_native_strategy(
        "ExecTester",
        ExecTesterConfig(
            strategy_id=StrategyId.from_str("EXEC_TESTER-001"),
            instrument_id=instrument_id,
            client_id=ClientId.from_str(DERIBIT),
            external_order_claims=[instrument_id],
            order_qty=order_qty,
            subscribe_quotes=True,
            subscribe_trades=True,
            open_position_on_start_qty=Decimal(args.quantity) if args.live_orders else None,
            open_position_on_first_quote=args.live_orders,
            open_position_time_in_force=TimeInForce.IOC,
            enable_limit_buys=args.live_orders,
            enable_limit_sells=args.live_orders and args.limit_sells,
            tob_offset_ticks=args.tob_offset_ticks,
            use_post_only=True,
            cancel_orders_on_stop=args.live_orders,
            close_positions_on_stop=args.live_orders,
            reduce_only_on_stop=False,
            dry_run=not args.live_orders,
            log_data=False,
        ),
    )

    if args.run:
        node.run()
    else:
        print("Built Deribit exec tester node. Pass --run to connect.")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build or run the Deribit Python v2 exec tester.")
    parser.add_argument("--trader-id", default="TESTER-001")
    parser.add_argument("--account-id", default="DERIBIT-001")
    parser.add_argument("--instrument", default=f"BTC-PERPETUAL.{DERIBIT}")
    parser.add_argument("--quantity", default="10")
    parser.add_argument("--tob-offset-ticks", type=int, default=500)
    parser.add_argument("--run", action="store_true")
    parser.add_argument("--live-orders", action="store_true")
    parser.add_argument("--limit-sells", action="store_true")
    return parser.parse_args()


if __name__ == "__main__":
    main()
