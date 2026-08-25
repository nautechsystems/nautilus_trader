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
Run a top-of-book imbalance strategy on the Architect AX sandbox.

Running this example connects to the AX sandbox and places live sandbox orders on
imbalance triggers. The strategy has no claimed alpha and is not intended for production
trading.

"""

from decimal import Decimal

from strategies import OrderBookImbalance
from strategies import OrderBookImbalanceConfig

from nautilus_trader.adapters.architect_ax import AX
from nautilus_trader.adapters.architect_ax import AxDataClientConfig
from nautilus_trader.adapters.architect_ax import AxDataClientFactory
from nautilus_trader.adapters.architect_ax import AxEnvironment
from nautilus_trader.adapters.architect_ax import AxExecutionClientConfig
from nautilus_trader.adapters.architect_ax import AxExecutionClientFactory
from nautilus_trader.common import Environment
from nautilus_trader.config import LiveExecutionEngineConfig
from nautilus_trader.config import LiveRiskEngineConfig
from nautilus_trader.live import LiveNode
from nautilus_trader.model import AccountId
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import StrategyId
from nautilus_trader.model import TraderId


DRY_RUN = False  # Set True to log intended trades without submitting orders
TRADER_ID = TraderId.from_str("TESTER-001")
ACCOUNT_ID = AccountId.from_str("AX-001")
STRATEGY_ID = StrategyId.from_str("AX-BOOK-IMBALANCE-001")
INSTRUMENT_ID = InstrumentId.from_str(f"XAU-PERP.{AX}")
MAX_TRADE_SIZE = Decimal(1)
TRIGGER_MIN_SIZE = Decimal(1)
TRIGGER_IMBALANCE_RATIO = Decimal("0.10")
MIN_SECONDS_BETWEEN_TRIGGERS = 5.0


def main() -> None:
    node = (
        LiveNode.builder("AX-BOOK-IMBALANCE-001", TRADER_ID, Environment.LIVE)
        .with_exec_engine_config(
            LiveExecutionEngineConfig(
                reconciliation_instrument_ids=[str(INSTRUMENT_ID)],
            ),
        )
        .with_reconciliation(True)
        .with_risk_engine_config(LiveRiskEngineConfig(bypass=True))
        .with_timeout_connection(20)
        .with_timeout_reconciliation(10)
        .with_timeout_portfolio(10)
        .with_timeout_disconnection_secs(10)
        .with_delay_post_stop_secs(5)
        .add_data_client(
            None,
            AxDataClientFactory(),
            AxDataClientConfig(environment=AxEnvironment.SANDBOX),
        )
        .add_exec_client(
            None,
            AxExecutionClientFactory(),
            AxExecutionClientConfig(
                account_id=ACCOUNT_ID,
                environment=AxEnvironment.SANDBOX,
            ),
        )
        .build()
    )
    node.add_strategy(
        OrderBookImbalance(
            OrderBookImbalanceConfig(
                instrument_id=INSTRUMENT_ID,
                max_trade_size=MAX_TRADE_SIZE,
                trigger_min_size=TRIGGER_MIN_SIZE,
                trigger_imbalance_ratio=TRIGGER_IMBALANCE_RATIO,
                min_seconds_between_triggers=MIN_SECONDS_BETWEEN_TRIGGERS,
                dry_run=DRY_RUN,
                strategy_id=STRATEGY_ID,
            ),
        ),
    )

    node.run()


if __name__ == "__main__":
    main()
