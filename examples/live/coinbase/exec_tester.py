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
Test Coinbase spot execution with the built-in ExecTester strategy.

WARNING: This example connects to Coinbase and places REAL orders with REAL funds.
On start it opens a position with an IOC order, then maintains post-only limit buy
quotes below the top of the book. On stop it cancels all orders and closes all
positions. Run only against a funded account you intend to test. The strategy has
no alpha advantage whatsoever and is not intended for production trading.

"""

from __future__ import annotations

from decimal import Decimal

from nautilus_trader.adapters.coinbase import COINBASE
from nautilus_trader.adapters.coinbase import CoinbaseDataClientConfig
from nautilus_trader.adapters.coinbase import CoinbaseDataClientFactory
from nautilus_trader.adapters.coinbase import CoinbaseEnvironment
from nautilus_trader.adapters.coinbase import CoinbaseExecutionClientConfig
from nautilus_trader.adapters.coinbase import CoinbaseExecutionClientFactory
from nautilus_trader.common import Environment
from nautilus_trader.config import LiveRiskEngineConfig
from nautilus_trader.live import LiveNode
from nautilus_trader.model import AccountId
from nautilus_trader.model import AccountType
from nautilus_trader.model import ClientId
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Quantity
from nautilus_trader.model import StrategyId
from nautilus_trader.model import TimeInForce
from nautilus_trader.model import TraderId
from nautilus_trader.testkit import ExecTesterConfig


# WARNING: With DRY_RUN = False, this tester submits orders to the configured
# environment and may use real funds. Set DRY_RUN = True to connect without
# submitting orders or sending shutdown cancel/close commands.
DRY_RUN = False
TRADER_ID = TraderId.from_str("TESTER-001")
ACCOUNT_ID = AccountId.from_str("COINBASE-001")
STRATEGY_ID = StrategyId.from_str("EXEC_TESTER-001")
COINBASE_ENVIRONMENT = CoinbaseEnvironment.LIVE
ACCOUNT_TYPE = AccountType.CASH
INSTRUMENT_ID = InstrumentId.from_str(f"BTC-USDC.{COINBASE}")
ORDER_QTY = "0.0001"
RETAIL_PORTFOLIO_ID = None
TOB_OFFSET_TICKS = 500


def main() -> None:
    """
    Run the example.
    """
    node = (
        LiveNode.builder("COINBASE-EXEC-TESTER-001", TRADER_ID, Environment.LIVE)
        .with_reconciliation(reconciliation=True)
        .with_risk_engine_config(LiveRiskEngineConfig(bypass=True))
        .add_data_client(
            None,
            CoinbaseDataClientFactory(),
            CoinbaseDataClientConfig(environment=COINBASE_ENVIRONMENT),
        )
        .add_exec_client(
            None,
            CoinbaseExecutionClientFactory(),
            CoinbaseExecutionClientConfig(
                account_id=ACCOUNT_ID,
                environment=COINBASE_ENVIRONMENT,
                account_type=ACCOUNT_TYPE,
                retail_portfolio_id=RETAIL_PORTFOLIO_ID,
            ),
        )
        .build()
    )
    node.add_builtin_strategy(
        "ExecTester",
        ExecTesterConfig(
            strategy_id=STRATEGY_ID,
            instrument_id=INSTRUMENT_ID,
            client_id=ClientId.from_str(COINBASE),
            external_order_instrument_ids=[INSTRUMENT_ID],
            order_qty=Quantity.from_str(ORDER_QTY),
            subscribe_quotes=True,
            subscribe_trades=True,
            open_position_on_start_qty=Decimal(ORDER_QTY),
            open_position_on_first_quote=True,
            open_position_time_in_force=TimeInForce.IOC,
            enable_limit_buys=True,
            enable_limit_sells=False,  # Spot sells require holding the base currency
            tob_offset_ticks=TOB_OFFSET_TICKS,
            use_post_only=True,
            cancel_orders_on_stop=True,
            close_positions_on_stop=True,
            reduce_only_on_stop=False,
            dry_run=DRY_RUN,
            log_data=False,
        ),
    )

    node.run()


if __name__ == "__main__":
    main()
