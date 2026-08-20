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
Test Lighter execution with the built-in ExecTester strategy.

WARNING: Running this script connects to the configured Lighter environment and
places REAL orders immediately. On start it opens a position with an IOC market
order, then maintains post-only limit quotes on both sides of the book. On stop it
cancels all orders and closes all positions. With the default testnet environment no
real funds are at risk; with `LighterEnvironment.MAINNET` the orders use real funds.
Run only against an account you intend to test. The strategy has no alpha advantage
whatsoever and is not intended for production trading.

Settings are the module-level constants below. Credentials resolve from the
`LIGHTER_TESTNET_*` or `LIGHTER_*` environment variables matching the configured
environment.

"""

from __future__ import annotations

from decimal import Decimal

from nautilus_trader.adapters.lighter import LIGHTER
from nautilus_trader.adapters.lighter import LighterDataClientConfig
from nautilus_trader.adapters.lighter import LighterDataClientFactory
from nautilus_trader.adapters.lighter import LighterEnvironment
from nautilus_trader.adapters.lighter import LighterExecClientConfig
from nautilus_trader.adapters.lighter import LighterExecutionClientFactory
from nautilus_trader.common import Environment
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.config import LiveRiskEngineConfig
from nautilus_trader.live import LiveNode
from nautilus_trader.model import AccountId
from nautilus_trader.model import ClientId
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Quantity
from nautilus_trader.model import StrategyId
from nautilus_trader.model import TimeInForce
from nautilus_trader.model import TraderId
from nautilus_trader.testkit import ExecTesterConfig


LIGHTER_ENVIRONMENT = LighterEnvironment.TESTNET
TRADER_ID = TraderId.from_str("TESTER-001")
ACCOUNT_ID = AccountId.from_str("LIGHTER-001")
STRATEGY_ID = StrategyId.from_str("EXEC_TESTER-001")
INSTRUMENT_ID = InstrumentId.from_str(f"DOGE-PERP.{LIGHTER}")

ORDER_QTY = "200"  # DOGE contracts, above the 100 contract minimum
OPEN_POSITION_ON_START_QTY = Decimal(ORDER_QTY)
TOB_OFFSET_TICKS = 500


def main() -> None:
    node = (
        LiveNode.builder("LIGHTER-EXEC-TESTER-001", TRADER_ID, Environment.LIVE)
        .with_reconciliation(True)
        .with_exec_engine_config(
            LiveExecEngineConfig(
                reconciliation_lookback_mins=60,
                reconciliation_instrument_ids=[str(INSTRUMENT_ID)],
            ),
        )
        .with_risk_engine_config(LiveRiskEngineConfig(bypass=True))
        .add_data_client(
            None,
            LighterDataClientFactory(),
            LighterDataClientConfig(environment=LIGHTER_ENVIRONMENT),
        )
        .add_exec_client(
            None,
            LighterExecutionClientFactory(),
            LighterExecClientConfig(
                trader_id=TRADER_ID,
                account_id=ACCOUNT_ID,
                environment=LIGHTER_ENVIRONMENT,
            ),
        )
        .build()
    )
    node.add_builtin_strategy(
        "ExecTester",
        ExecTesterConfig(
            strategy_id=STRATEGY_ID,
            instrument_id=INSTRUMENT_ID,
            client_id=ClientId.from_str(LIGHTER),
            external_order_claims=[INSTRUMENT_ID],
            order_qty=Quantity.from_str(ORDER_QTY),
            subscribe_quotes=True,
            subscribe_trades=False,
            open_position_on_start_qty=OPEN_POSITION_ON_START_QTY,
            open_position_on_first_quote=True,
            open_position_time_in_force=TimeInForce.IOC,
            enable_limit_buys=True,
            enable_limit_sells=True,
            tob_offset_ticks=TOB_OFFSET_TICKS,
            use_post_only=True,
            cancel_orders_on_stop=True,
            close_positions_on_stop=True,
            reduce_only_on_stop=True,
            dry_run=False,  # Set True to log intended order flow without submitting orders
            log_data=False,
        ),
    )

    node.run()


if __name__ == "__main__":
    main()
