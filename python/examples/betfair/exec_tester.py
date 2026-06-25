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
Betfair Python v2 execution tester example.

The default path builds a live node and attaches the native Rust ExecTester without
connecting to Betfair or submitting orders. Pass --run to connect. Pass --live-orders
only when the account is funded and you intend to test live order flow.

"""

from __future__ import annotations

import argparse
from decimal import Decimal

from nautilus_trader.adapters.betfair import BetfairDataClientFactory
from nautilus_trader.adapters.betfair import BetfairDataConfig
from nautilus_trader.adapters.betfair import BetfairExecConfig
from nautilus_trader.adapters.betfair import BetfairExecutionClientFactory
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


BETFAIR = "BETFAIR"


def main() -> None:
    args = parse_args()
    trader_id = TraderId.from_str(args.trader_id)
    account_id = AccountId.from_str(args.account_id)
    instrument_id = InstrumentId.from_str(args.instrument)
    order_qty = Quantity.from_str(args.quantity)

    builder = (
        LiveNode.builder("BETFAIR-EXEC-TESTER-001", trader_id, Environment.LIVE)
        .with_reconciliation(args.run)
        .with_risk_engine_config(LiveRiskEngineConfig(bypass=True))
        .add_data_client(
            None,
            BetfairDataClientFactory(),
            BetfairDataConfig(
                account_currency=args.account_currency,
                market_ids=[args.market_id],
                stream_conflate_ms=0,
            ),
        )
        .add_exec_client(
            None,
            BetfairExecutionClientFactory(),
            BetfairExecConfig(
                trader_id=trader_id,
                account_id=account_id,
                account_currency=args.account_currency,
                stream_market_ids_filter=[args.market_id],
                ignore_external_orders=True,
                reconcile_market_ids_only=True,
                reconcile_market_ids=[args.market_id],
            ),
        )
    )

    node = builder.build()
    node.add_builtin_strategy(
        "ExecTester",
        ExecTesterConfig(
            strategy_id=StrategyId.from_str("EXEC_TESTER-001"),
            instrument_id=instrument_id,
            client_id=ClientId.from_str(BETFAIR),
            external_order_claims=[instrument_id],
            order_qty=order_qty,
            subscribe_quotes=False,
            subscribe_trades=False,
            open_position_on_start_qty=Decimal(args.quantity) if args.live_orders else None,
            open_position_time_in_force=TimeInForce.AT_THE_CLOSE,
            enable_limit_buys=False,
            enable_limit_sells=False,
            cancel_orders_on_stop=args.live_orders,
            close_positions_on_stop=False,
            reduce_only_on_stop=False,
            dry_run=not args.live_orders,
            can_unsubscribe=False,
            log_data=False,
        ),
    )

    if args.run:
        node.run()
    else:
        print("Built Betfair exec tester node. Pass --run to connect.")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build or run the Betfair Python v2 exec tester.")
    parser.add_argument("--trader-id", default="TESTER-001")
    parser.add_argument("--account-id", default="BETFAIR-001")
    parser.add_argument("--account-currency", default="GBP")
    parser.add_argument("--market-id", default="1.234567890")
    parser.add_argument("--instrument", default=f"1.234567890-123456.{BETFAIR}")
    parser.add_argument("--quantity", default="2.00")
    parser.add_argument("--run", action="store_true")
    parser.add_argument("--live-orders", action="store_true")
    return parser.parse_args()


if __name__ == "__main__":
    main()
