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
Run a Lighter NVDA RWA composite market maker with the built-in CompositeMarketMaker
strategy: Databento ``NVDA.EQUS`` quotes drive the signal and ``NVDA-PERP.LIGHTER``
is the quoted target. This is the Python counterpart of the Rust tutorial binary
``examples/tutorials/src/bin/lighter_nvda_composite_mm.rs``.

WARNING: Running this script connects to the configured Lighter environment and
places REAL post-only orders immediately. With the default testnet environment no
real funds are at risk; with `LighterEnvironment.MAINNET` the orders use real funds.
Run only against an account you intend to test. The strategy is a demonstration and
is not intended for production trading.

Settings are the module-level constants below. Required environment variables:
- DATABENTO_API_KEY.
- LIGHTER_TESTNET_ACCOUNT_INDEX, LIGHTER_TESTNET_API_KEY_INDEX, and
  LIGHTER_TESTNET_API_SECRET for the testnet environment (the default).
- LIGHTER_ACCOUNT_INDEX, LIGHTER_API_KEY_INDEX, and LIGHTER_API_SECRET for mainnet.

"""

from __future__ import annotations

import os
from pathlib import Path

from nautilus_trader.adapters.databento import DatabentoDataClientFactory
from nautilus_trader.adapters.databento import DatabentoLiveClientConfig
from nautilus_trader.adapters.lighter import LIGHTER
from nautilus_trader.adapters.lighter import LighterDataClientConfig
from nautilus_trader.adapters.lighter import LighterDataClientFactory
from nautilus_trader.adapters.lighter import LighterEnvironment
from nautilus_trader.adapters.lighter import LighterExecClientConfig
from nautilus_trader.adapters.lighter import LighterExecutionClientFactory
from nautilus_trader.common import Environment
from nautilus_trader.live import LiveNode
from nautilus_trader.model import AccountId
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Quantity
from nautilus_trader.model import StrategyId
from nautilus_trader.model import TraderId
from nautilus_trader.trading import CompositeMarketMakerConfig


LIGHTER_ENVIRONMENT = LighterEnvironment.TESTNET
TRADER_ID = TraderId.from_str("TESTER-001")
ACCOUNT_ID = AccountId.from_str("LIGHTER-001")
STRATEGY_ID = StrategyId.from_str("NVDA_COMPOSITE_MM-001")
INSTRUMENT_ID = InstrumentId.from_str(f"NVDA-PERP.{LIGHTER}")
SIGNAL_INSTRUMENT_ID = InstrumentId.from_str("NVDA.EQUS")

MAX_POSITION = "0.20"
TRADE_SIZE = "0.05"
HALF_SPREAD_BPS = 25
INVENTORY_SKEW_FACTOR = 2.0
SIGNAL_SKEW_FACTOR = 55.0
REQUOTE_THRESHOLD_BPS = 5
ON_CANCEL_RESUBMIT = False

DATABENTO_API_KEY = os.environ.get("DATABENTO_API_KEY", "")
PUBLISHERS_FILEPATH = (
    Path(__file__).resolve().parents[3] / "crates/adapters/databento/publishers.json"
)


def main() -> None:
    if not DATABENTO_API_KEY:
        raise SystemExit("DATABENTO_API_KEY must be set")

    node = (
        LiveNode.builder("LIGHTER-NVDA-COMPOSITE-MM-001", TRADER_ID, Environment.LIVE)
        .with_reconciliation(True)
        .with_delay_post_stop_secs(5)
        .add_data_client(
            None,
            DatabentoDataClientFactory(),
            DatabentoLiveClientConfig(
                api_key=DATABENTO_API_KEY,
                publishers_filepath=PUBLISHERS_FILEPATH,
                use_exchange_as_venue=True,
            ),
        )
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
        "CompositeMarketMaker",
        CompositeMarketMakerConfig(
            instrument_id=INSTRUMENT_ID,
            signal_instrument_id=SIGNAL_INSTRUMENT_ID,
            max_position=Quantity.from_str(MAX_POSITION),
            strategy_id=STRATEGY_ID,
            order_id_tag="001",
            trade_size=Quantity.from_str(TRADE_SIZE),
            half_spread_bps=HALF_SPREAD_BPS,
            inventory_skew_factor=INVENTORY_SKEW_FACTOR,
            signal_skew_factor=SIGNAL_SKEW_FACTOR,
            requote_threshold_bps=REQUOTE_THRESHOLD_BPS,
            on_cancel_resubmit=ON_CANCEL_RESUBMIT,
        ),
    )
    node.run()


if __name__ == "__main__":
    main()
