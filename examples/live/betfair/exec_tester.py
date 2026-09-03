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
Test Betfair execution with the built-in ExecTester strategy.

WARNING: This example connects to Betfair and places REAL bets with REAL funds.
On start it opens a position with an AT_THE_CLOSE order; on stop it cancels all
orders. Run only against a funded account you intend to test. The strategy has no
alpha advantage whatsoever and is not intended for production trading.
Set `BETFAIR_MARKET_ID` to an active market and `BETFAIR_INSTRUMENT_ID` to the
runner used for the test.

"""

from __future__ import annotations

import os
from decimal import Decimal

from nautilus_trader.adapters.betfair import BetfairDataClientConfig
from nautilus_trader.adapters.betfair import BetfairDataClientFactory
from nautilus_trader.adapters.betfair import BetfairExecutionClientConfig
from nautilus_trader.adapters.betfair import BetfairExecutionClientFactory
from nautilus_trader.common import Environment
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


# WARNING: With DRY_RUN = False, this tester submits orders to the configured
# environment and may use real funds. Set DRY_RUN = True to connect without
# submitting orders or sending shutdown cancel/close commands.
DRY_RUN = False
BETFAIR = "BETFAIR"
TRADER_ID = TraderId.from_str("TESTER-001")
ACCOUNT_ID = AccountId.from_str("BETFAIR-001")
STRATEGY_ID = StrategyId.from_str("EXEC_TESTER-001")
ACCOUNT_CURRENCY = "GBP"
ORDER_QTY = "2.00"


def main() -> None:
    """
    Run the example.
    """
    market_id, instrument_id = load_market_target()
    node = (
        LiveNode.builder("BETFAIR-EXEC-TESTER-001", TRADER_ID, Environment.LIVE)
        .with_reconciliation(reconciliation=True)
        .with_risk_engine_config(LiveRiskEngineConfig(bypass=True))
        .add_data_client(
            None,
            BetfairDataClientFactory(),
            BetfairDataClientConfig(
                account_currency=ACCOUNT_CURRENCY,
                market_ids=[market_id],
                stream_conflate_ms=0,
            ),
        )
        .add_exec_client(
            None,
            BetfairExecutionClientFactory(),
            BetfairExecutionClientConfig(
                account_id=ACCOUNT_ID,
                account_currency=ACCOUNT_CURRENCY,
                stream_market_ids_filter=[market_id],
                ignore_external_orders=True,
                reconcile_market_ids_only=True,
                reconcile_market_ids=[market_id],
            ),
        )
        .build()
    )
    node.add_builtin_strategy(
        "ExecTester",
        ExecTesterConfig(
            strategy_id=STRATEGY_ID,
            instrument_id=instrument_id,
            client_id=ClientId.from_str(BETFAIR),
            external_order_instrument_ids=[instrument_id],
            order_qty=Quantity.from_str(ORDER_QTY),
            subscribe_quotes=False,
            subscribe_trades=False,
            open_position_on_start_qty=Decimal(ORDER_QTY),
            open_position_time_in_force=TimeInForce.AT_THE_CLOSE,
            enable_limit_buys=False,
            enable_limit_sells=False,
            cancel_orders_on_stop=True,
            close_positions_on_stop=False,
            reduce_only_on_stop=False,
            dry_run=DRY_RUN,
            can_unsubscribe=False,
            log_data=False,
        ),
    )

    node.run()


def load_market_target() -> tuple[str, InstrumentId]:
    """
    Load market target.
    """
    market_id = os.getenv("BETFAIR_MARKET_ID")
    instrument_id = os.getenv("BETFAIR_INSTRUMENT_ID")

    if not market_id:
        raise SystemExit("BETFAIR_MARKET_ID must be set to an active Betfair market")
    if not instrument_id:
        raise SystemExit("BETFAIR_INSTRUMENT_ID must be set to a runner in BETFAIR_MARKET_ID")
    if not instrument_id.startswith(f"{market_id}-") or not instrument_id.endswith(f".{BETFAIR}"):
        raise SystemExit("BETFAIR_INSTRUMENT_ID must belong to BETFAIR_MARKET_ID")

    return market_id, InstrumentId.from_str(instrument_id)


if __name__ == "__main__":
    main()
