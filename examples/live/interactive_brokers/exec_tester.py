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
Test Interactive Brokers execution with the built-in ExecTester strategy.

WARNING: This example connects to TWS or IB Gateway and places live orders on the
configured account (paper port 7497 by default). On start it opens a position with
an IOC order, then maintains post-only limit quotes on both sides of the book. On
stop it cancels all orders and closes all positions. Run only against an account
you intend to test. The strategy has no alpha advantage whatsoever and is not
intended for production trading.
Set `TWS_ACCOUNT` to the account that will receive the orders.

"""

from __future__ import annotations

import os
from decimal import Decimal

from nautilus_trader.adapters.interactive_brokers import InteractiveBrokersDataClientConfig
from nautilus_trader.adapters.interactive_brokers import InteractiveBrokersDataClientFactory
from nautilus_trader.adapters.interactive_brokers import InteractiveBrokersExecutionClientConfig
from nautilus_trader.adapters.interactive_brokers import InteractiveBrokersExecutionClientFactory
from nautilus_trader.adapters.interactive_brokers import InteractiveBrokersInstrumentProviderConfig
from nautilus_trader.adapters.interactive_brokers import MarketDataType
from nautilus_trader.adapters.interactive_brokers import SymbologyMethod
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


IB = "IB"
TRADER_ID = TraderId.from_str("TESTER-001")
ACCOUNT_ID = AccountId.from_str("IB-001")
STRATEGY_ID = StrategyId.from_str("EXEC_TESTER-001")
HOST = "127.0.0.1"
PORT = 7497
CLIENT_ID = 101
INSTRUMENT_ID = InstrumentId.from_str("AAPL=STK.SMART")
ORDER_QTY = "1"
TOB_OFFSET_TICKS = 500


def main() -> None:
    """
    Run the example.
    """
    ib_account_id = os.getenv("TWS_ACCOUNT")
    if not ib_account_id:
        raise SystemExit("TWS_ACCOUNT must be set to the target IB account")

    provider_config = InteractiveBrokersInstrumentProviderConfig(
        symbology_method=SymbologyMethod.RAW,
        load_ids={INSTRUMENT_ID},
    )

    node = (
        LiveNode.builder("IB-EXEC-TESTER-001", TRADER_ID, Environment.LIVE)
        .with_reconciliation(reconciliation=True)
        .with_risk_engine_config(LiveRiskEngineConfig(bypass=True))
        .add_data_client(
            None,
            InteractiveBrokersDataClientFactory(),
            InteractiveBrokersDataClientConfig(
                host=HOST,
                port=PORT,
                client_id=CLIENT_ID,
                market_data_type=MarketDataType.DELAYED,
                instrument_provider=provider_config,
            ),
        )
        .add_exec_client(
            None,
            InteractiveBrokersExecutionClientFactory(),
            InteractiveBrokersExecutionClientConfig(
                host=HOST,
                port=PORT,
                client_id=CLIENT_ID,
                account_id=ib_account_id,
                instrument_provider=provider_config,
            ),
        )
        .build()
    )
    node.add_builtin_strategy(
        "ExecTester",
        ExecTesterConfig(
            strategy_id=STRATEGY_ID,
            instrument_id=INSTRUMENT_ID,
            client_id=ClientId.from_str(IB),
            external_order_claims=[INSTRUMENT_ID],
            order_qty=Quantity.from_str(ORDER_QTY),
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
