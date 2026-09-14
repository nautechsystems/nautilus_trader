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
Test Polymarket execution with the built-in ExecTester strategy.

WARNING: This example connects to Polymarket and places REAL orders with REAL
funds. On start it opens a position with an IOC market order quoted in pUSD; on
stop it cancels all orders and closes all positions. Run only against a funded
account you intend to test. The strategy has no alpha advantage whatsoever and is
not intended for production trading.

"""

from __future__ import annotations

from decimal import Decimal

from nautilus_trader.adapters.polymarket import PolymarketDataClientConfig
from nautilus_trader.adapters.polymarket import PolymarketDataClientFactory
from nautilus_trader.adapters.polymarket import PolymarketExecutionClientConfig
from nautilus_trader.adapters.polymarket import PolymarketExecutionClientFactory
from nautilus_trader.adapters.polymarket import PolymarketInstrumentProviderConfig
from nautilus_trader.adapters.polymarket import SignatureType
from nautilus_trader.common import Environment
from nautilus_trader.config import LiveExecutionEngineConfig
from nautilus_trader.config import LiveRiskEngineConfig
from nautilus_trader.live import LiveNode
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
POLYMARKET = "POLYMARKET"
TRADER_ID = TraderId.from_str("TESTER-001")
ACCOUNT_ID = "POLYMARKET-001"
STRATEGY_ID = StrategyId.from_str("EXEC_TESTER-001")
EVENT_SLUG = "fed-decision-in-september-762"
INSTRUMENT_ID = InstrumentId.from_str(
    "0xac02cbb049e46d6a3627c0fdf52fa554982a9025d45968207b362acb6ca4b830-"
    f"28239418772633645184924651434956000849078365566842629564562475378531350731731.{POLYMARKET}",
)
ORDER_QTY = "5"  # In pUSD for the quote-quantity market order
TOB_OFFSET_TICKS = 5


def main() -> None:
    """
    Run the example.
    """
    instrument_config = PolymarketInstrumentProviderConfig(
        event_slugs=[EVENT_SLUG],
        load_ids=[INSTRUMENT_ID],
        use_gamma_markets=True,
    )

    node = (
        LiveNode.builder("POLYMARKET-EXEC-TESTER-001", TRADER_ID, Environment.LIVE)
        .with_reconciliation(reconciliation=True)
        .with_exec_engine_config(
            LiveExecutionEngineConfig(
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
            PolymarketDataClientFactory(),
            PolymarketDataClientConfig(
                instrument_config=instrument_config,
            ),
        )
        .add_exec_client(
            None,
            PolymarketExecutionClientFactory(),
            PolymarketExecutionClientConfig(
                account_id=ACCOUNT_ID,
                signature_type=SignatureType.PolyGnosisSafe,
                instrument_config=instrument_config,
            ),
        )
        .build()
    )
    node.add_builtin_strategy(
        "ExecTester",
        ExecTesterConfig(
            strategy_id=STRATEGY_ID,
            instrument_id=INSTRUMENT_ID,
            client_id=ClientId.from_str(POLYMARKET),
            external_order_instrument_ids=[INSTRUMENT_ID],
            use_uuid_client_order_ids=True,
            order_qty=Quantity.from_str(ORDER_QTY),
            subscribe_quotes=True,
            subscribe_trades=True,
            open_position_on_start_qty=Decimal(ORDER_QTY),
            open_position_on_first_quote=True,
            open_position_time_in_force=TimeInForce.IOC,
            enable_limit_buys=False,  # Set True for passive limit quoting (order_qty in shares)
            enable_limit_sells=False,
            enable_stop_buys=False,
            enable_stop_sells=False,
            tob_offset_ticks=TOB_OFFSET_TICKS,
            use_post_only=False,  # Set True together with enable_limit_buys
            use_quote_quantity=True,
            cancel_orders_on_stop=True,
            close_positions_on_stop=True,
            close_positions_qty_precision=2,
            close_positions_time_in_force=TimeInForce.IOC,
            reduce_only_on_stop=False,
            dry_run=DRY_RUN,
            log_data=False,
        ),
    )

    node.run()


if __name__ == "__main__":
    main()
