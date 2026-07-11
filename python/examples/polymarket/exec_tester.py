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
Polymarket Python v2 execution tester example.

The default path builds a live node and attaches the native Rust ExecTester without
connecting to Polymarket or submitting orders. Pass --run to connect. Use
--live-orders for a market-order lifecycle or --limit-orders for passive limit orders.

"""

from __future__ import annotations

import argparse
from decimal import Decimal

from nautilus_trader.adapters.polymarket import PolymarketDataClientConfig
from nautilus_trader.adapters.polymarket import PolymarketDataClientFactory
from nautilus_trader.adapters.polymarket import PolymarketExecClientConfig
from nautilus_trader.adapters.polymarket import PolymarketExecutionClientFactory
from nautilus_trader.adapters.polymarket import PolymarketInstrumentProviderConfig
from nautilus_trader.adapters.polymarket import SignatureType
from nautilus_trader.common import Environment
from nautilus_trader.live import LiveExecEngineConfig
from nautilus_trader.live import LiveNode
from nautilus_trader.live import LiveRiskEngineConfig
from nautilus_trader.model import ClientId
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Quantity
from nautilus_trader.model import StrategyId
from nautilus_trader.model import TimeInForce
from nautilus_trader.model import TraderId
from nautilus_trader.testkit import ExecTesterConfig


POLYMARKET = "POLYMARKET"
DEFAULT_INSTRUMENT = (
    "0xac02cbb049e46d6a3627c0fdf52fa554982a9025d45968207b362acb6ca4b830-"
    f"28239418772633645184924651434956000849078365566842629564562475378531350731731.{POLYMARKET}"
)


def main() -> None:
    args = parse_args()
    trader_id = TraderId.from_str(args.trader_id)
    instrument_id = InstrumentId.from_str(args.instrument)
    order_qty = Quantity.from_str(args.quantity)

    builder = (
        LiveNode.builder("POLYMARKET-EXEC-TESTER-001", trader_id, Environment.LIVE)
        .with_reconciliation(args.run)
        .with_exec_engine_config(
            LiveExecEngineConfig(
                reconciliation_instrument_ids=[args.instrument],
                open_check_interval_secs=10,
                position_check_interval_secs=30,
            ),
        )
        .with_risk_engine_config(LiveRiskEngineConfig(bypass=True))
        .add_data_client(
            None,
            PolymarketDataClientFactory(),
            PolymarketDataClientConfig(
                instrument_config=PolymarketInstrumentProviderConfig(
                    event_slugs=[args.event_slug],
                    use_gamma_markets=True,
                ),
            ),
        )
        .add_exec_client(
            None,
            PolymarketExecutionClientFactory(),
            PolymarketExecClientConfig(
                trader_id=args.trader_id,
                account_id=args.account_id,
                private_key=None if args.run else args.private_key,
                api_key=None if args.run else args.api_key,
                api_secret=None if args.run else args.api_secret,
                passphrase=None if args.run else args.passphrase,
                funder=None if args.run else args.funder,
                signature_type=SignatureType.PolyGnosisSafe,
            ),
        )
    )

    node = builder.build()
    node.add_builtin_strategy(
        "ExecTester",
        ExecTesterConfig(
            strategy_id=StrategyId.from_str("EXEC_TESTER-001"),
            instrument_id=instrument_id,
            client_id=ClientId.from_str(POLYMARKET),
            external_order_claims=[instrument_id],
            use_uuid_client_order_ids=True,
            order_qty=order_qty,
            subscribe_quotes=True,
            subscribe_trades=True,
            open_position_on_start_qty=Decimal(args.quantity) if args.live_orders else None,
            open_position_on_first_quote=args.live_orders,
            open_position_time_in_force=TimeInForce.IOC,
            enable_limit_buys=args.limit_orders,
            enable_limit_sells=False,
            enable_stop_buys=False,
            enable_stop_sells=False,
            tob_offset_ticks=args.tob_offset_ticks,
            use_post_only=args.limit_orders,
            use_quote_quantity=args.live_orders,
            cancel_orders_on_stop=args.live_orders or args.limit_orders,
            close_positions_on_stop=args.live_orders,
            close_positions_time_in_force=TimeInForce.IOC,
            reduce_only_on_stop=False,
            dry_run=not args.live_orders and not args.limit_orders,
            log_data=False,
        ),
    )

    if args.run:
        node.run()
    else:
        print("Built Polymarket exec tester node. Pass --run to connect.")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Build or run the Polymarket Python v2 exec tester.",
    )
    parser.add_argument("--trader-id", default="TESTER-001")
    parser.add_argument("--account-id", default="POLYMARKET-001")
    parser.add_argument("--event-slug", default="fed-decision-in-september-762")
    parser.add_argument("--instrument", default=DEFAULT_INSTRUMENT)
    parser.add_argument(
        "--quantity",
        default="5",
        help="pUSD for --live-orders and shares for --limit-orders.",
    )
    parser.add_argument(
        "--private-key",
        default="0x0101010101010101010101010101010101010101010101010101010101010101",
        help="Valid unfunded key used only for the offline build path.",
    )
    parser.add_argument("--api-key", default="test_key")
    parser.add_argument(
        "--api-secret",
        default="dGVzdF9zZWNyZXRfa2V5XzMyYnl0ZXNfcGFkMTIzNDU=",
        help="Valid base64 placeholder used only for the offline build path.",
    )
    parser.add_argument("--passphrase", default="test_passphrase")
    parser.add_argument("--funder", default="0x0000000000000000000000000000000000000000")
    parser.add_argument("--tob-offset-ticks", type=int, default=5)
    parser.add_argument("--run", action="store_true")
    order_mode = parser.add_mutually_exclusive_group()
    order_mode.add_argument("--live-orders", action="store_true")
    order_mode.add_argument("--limit-orders", action="store_true")
    return parser.parse_args()


if __name__ == "__main__":
    main()
