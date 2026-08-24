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
Run a Bollinger Band mean reversion strategy on the Architect AX sandbox.

Running this example connects to the AX sandbox and places live sandbox orders on mean
reversion signals. The strategy has no claimed alpha and is not intended for production
trading.

"""

from decimal import Decimal

from strategies import BBMeanReversion
from strategies import BBMeanReversionConfig

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
from nautilus_trader.model import BarType
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import StrategyId
from nautilus_trader.model import TraderId


TRADER_ID = TraderId.from_str("TESTER-001")
ACCOUNT_ID = AccountId.from_str("AX-001")
STRATEGY_ID = StrategyId.from_str("AX-MEAN-REVERSION-001")
INSTRUMENT_ID = InstrumentId.from_str(f"EURUSD-PERP.{AX}")
BAR_TYPE = BarType.from_str(f"{INSTRUMENT_ID}-1-MINUTE-MID-INTERNAL")
TRADE_SIZE = Decimal(1)
BB_PERIOD = 20
BB_STD = 2.0
RSI_PERIOD = 14
RSI_BUY_THRESHOLD = 0.30
RSI_SELL_THRESHOLD = 0.70


def main() -> None:
    node = (
        LiveNode.builder("AX-MEAN-REVERSION-001", TRADER_ID, Environment.LIVE)
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
        BBMeanReversion(
            BBMeanReversionConfig(
                instrument_id=INSTRUMENT_ID,
                bar_type=BAR_TYPE,
                trade_size=TRADE_SIZE,
                bb_period=BB_PERIOD,
                bb_std=BB_STD,
                rsi_period=RSI_PERIOD,
                rsi_buy_threshold=RSI_BUY_THRESHOLD,
                rsi_sell_threshold=RSI_SELL_THRESHOLD,
                strategy_id=STRATEGY_ID,
            ),
        ),
    )

    node.run()


if __name__ == "__main__":
    main()
