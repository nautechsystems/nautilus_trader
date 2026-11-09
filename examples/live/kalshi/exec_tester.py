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
Test Kalshi demo execution with the built-in ExecTester strategy.

WARNING: With DRY_RUN = False, this example places demo orders and bypasses all
shared pre-trade risk checks. Risk-enabled submission, order lists, and amendments
are unsupported. On stop it cancels its orders but leaves filled positions open.
Do not use this setup in production.

Edit the instrument constant to target an open demo market on shard 0 with a
single-step price grid. Keep the bid more than 5 ticks above zero.

Demo credentials are read from the environment when both config fields are absent:

- ``KALSHI_DEMO_API_KEY_ID``
- ``KALSHI_DEMO_PRIVATE_KEY``

"""

from __future__ import annotations

from nautilus_trader.adapters.kalshi import KALSHI
from nautilus_trader.adapters.kalshi import KalshiDataClientConfig
from nautilus_trader.adapters.kalshi import KalshiDataClientFactory
from nautilus_trader.adapters.kalshi import KalshiEnvironment
from nautilus_trader.adapters.kalshi import KalshiExecutionClientConfig
from nautilus_trader.adapters.kalshi import KalshiExecutionClientFactory
from nautilus_trader.common import Environment
from nautilus_trader.config import LiveExecutionEngineConfig
from nautilus_trader.config import LiveRiskEngineConfig
from nautilus_trader.live import LiveNode
from nautilus_trader.model import ClientId
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Quantity
from nautilus_trader.model import StrategyId
from nautilus_trader.model import TraderId
from nautilus_trader.testkit import ExecTesterConfig


# WARNING: With DRY_RUN = False, this tester submits orders to the demo environment.
# Set DRY_RUN = True to connect without submitting orders or sending shutdown
# cancel commands.
DRY_RUN = False
TRADER_ID = TraderId.from_str("TESTER-001")
STRATEGY_ID = StrategyId.from_str("EXEC_TESTER-001")
ENVIRONMENT = KalshiEnvironment.DEMO
EXCHANGE_INDEX = 0
INSTRUMENT_ID = InstrumentId.from_str(f"KXNBA1HTOTAL-26NOV01LACGSW-125.{KALSHI}")
ORDER_QTY = "0.01"
TOB_OFFSET_TICKS = 5


def main() -> None:
    """
    Run the example.
    """
    node = (
        LiveNode.builder("KALSHI-EXEC-TESTER-001", TRADER_ID, Environment.LIVE)
        .with_reconciliation(reconciliation=True)
        .with_exec_engine_config(
            LiveExecutionEngineConfig(
                reconciliation_lookback_mins=180,
                reconciliation_instrument_ids=[str(INSTRUMENT_ID)],
                open_check_interval_secs=10,
                position_check_interval_secs=30,
            ),
        )
        .with_risk_engine_config(LiveRiskEngineConfig(bypass=True))
        .with_timeout_disconnection_secs(30)
        .with_delay_post_stop_secs(30)
        .add_data_client(
            None,
            KalshiDataClientFactory(),
            KalshiDataClientConfig(
                environment=ENVIRONMENT,
                instrument_ids=[INSTRUMENT_ID],
            ),
        )
        .add_exec_client(
            None,
            KalshiExecutionClientFactory(),
            KalshiExecutionClientConfig(
                environment=ENVIRONMENT,
                exchange_index=EXCHANGE_INDEX,
            ),
        )
        .build()
    )
    node.add_builtin_strategy(
        "ExecTester",
        ExecTesterConfig(
            strategy_id=STRATEGY_ID,
            instrument_id=INSTRUMENT_ID,
            client_id=ClientId.from_str(KALSHI),
            external_order_instrument_ids=[INSTRUMENT_ID],
            order_qty=Quantity.from_str(ORDER_QTY),
            subscribe_quotes=True,
            subscribe_trades=True,
            enable_limit_buys=True,
            enable_limit_sells=True,
            batch_submit_limit_pair=True,
            enable_stop_buys=False,
            enable_stop_sells=False,
            tob_offset_ticks=TOB_OFFSET_TICKS,
            use_post_only=True,
            cancel_orders_on_stop=True,
            close_positions_on_stop=False,
            dry_run=DRY_RUN,
            log_data=False,
        ),
    )
    node.run()


if __name__ == "__main__":
    main()
