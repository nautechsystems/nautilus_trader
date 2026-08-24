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
Test execution with the built-in ExecTester strategy on public Binance data.

Running this example streams public Binance Spot data while the sandbox matching engine
simulates execution locally. The ExecTester opens a position with an IOC order,
maintains post-only limit quotes on both sides of the book, then cancels all orders and
closes all positions on stop. No credentials or real funds are involved.

"""

from __future__ import annotations

from decimal import Decimal

from nautilus_trader.adapters.binance import BinanceDataClientConfig
from nautilus_trader.adapters.binance import BinanceDataClientFactory
from nautilus_trader.adapters.binance import BinanceEnvironment
from nautilus_trader.adapters.binance import BinanceProductType
from nautilus_trader.adapters.binance import BinanceSpotMarketDataMode
from nautilus_trader.adapters.sandbox import SandboxExecutionClientConfig
from nautilus_trader.adapters.sandbox import SandboxExecutionClientFactory
from nautilus_trader.common import Environment
from nautilus_trader.config import LiveRiskEngineConfig
from nautilus_trader.live import LiveNode
from nautilus_trader.model import AccountId
from nautilus_trader.model import ClientId
from nautilus_trader.model import Currency
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Money
from nautilus_trader.model import Quantity
from nautilus_trader.model import StrategyId
from nautilus_trader.model import TimeInForce
from nautilus_trader.model import TraderId
from nautilus_trader.model import Venue
from nautilus_trader.testkit import ExecTesterConfig


BINANCE = "BINANCE"
TRADER_ID = TraderId.from_str("TESTER-001")
ACCOUNT_ID = AccountId.from_str("BINANCE-SANDBOX-001")
VENUE = Venue.from_str(BINANCE)
STRATEGY_ID = StrategyId.from_str("EXEC_TESTER-001")
INSTRUMENT_ID = InstrumentId.from_str(f"BTCUSDT.{BINANCE}")
ORDER_QTY = "0.001"
CURRENCY = "USDT"
STARTING_BALANCE = "100000"
TOB_OFFSET_TICKS = 500


def main() -> None:
    node = (
        LiveNode.builder("SANDBOX-EXEC-TESTER-001", TRADER_ID, Environment.SANDBOX)
        .with_reconciliation(False)
        .with_risk_engine_config(LiveRiskEngineConfig(bypass=True))
        .add_data_client(
            None,
            BinanceDataClientFactory(),
            BinanceDataClientConfig(
                product_type=BinanceProductType.SPOT,
                environment=BinanceEnvironment.LIVE,
                spot_market_data_mode=BinanceSpotMarketDataMode.Json,
            ),
        )
        .add_simulated_exec_client(
            BINANCE,
            SandboxExecutionClientFactory(),
            SandboxExecutionClientConfig(
                venue=VENUE,
                starting_balances=[
                    Money(float(STARTING_BALANCE), Currency.from_str(CURRENCY)),
                ],
                account_id=ACCOUNT_ID,
            ),
        )
        .build()
    )
    node.add_builtin_strategy(
        "ExecTester",
        ExecTesterConfig(
            strategy_id=STRATEGY_ID,
            instrument_id=INSTRUMENT_ID,
            client_id=ClientId.from_str(BINANCE),
            external_order_claims=[INSTRUMENT_ID],
            order_qty=Quantity.from_str(ORDER_QTY),
            subscribe_book=True,
            subscribe_quotes=True,
            subscribe_trades=True,
            open_position_on_start_qty=Decimal(ORDER_QTY),
            open_position_on_first_quote=True,
            open_position_time_in_force=TimeInForce.IOC,
            enable_limit_buys=True,
            enable_limit_sells=True,
            tob_offset_ticks=TOB_OFFSET_TICKS,
            use_post_only=True,
            cancel_orders_on_stop=True,
            close_positions_on_stop=True,
            reduce_only_on_stop=False,
            dry_run=False,  # Set True to log intended order flow without submitting orders
            log_data=False,
        ),
    )

    node.run()


if __name__ == "__main__":
    main()
