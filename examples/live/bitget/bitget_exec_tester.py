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

"""Bitget private-stream tester.

This example authenticates the execution client and listens for private
account/order/fill/position updates. It intentionally does not submit,
modify, or cancel orders even though the Bitget trading REST surface is
implemented, because this example is focused on private-stream smoke testing.
"""

from nautilus_trader.adapters.bitget.config import BitgetExecClientConfig
from nautilus_trader.adapters.bitget.constants import BITGET_VENUE
from nautilus_trader.adapters.bitget.factories import BitgetLiveExecClientFactory
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.live.config import LiveRiskEngineConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.portfolio.config import PortfolioConfig


INSTRUMENT_ID = InstrumentId.from_str("BTCUSDT.BITGET")

config_node = TradingNodeConfig(
    trader_id=TraderId("TESTER-001"),
    logging=LoggingConfig(
        log_level="INFO",
        use_pyo3=True,
    ),
    exec_engine=LiveExecEngineConfig(
        reconciliation=False,
        graceful_shutdown_on_exception=True,
    ),
    risk_engine=LiveRiskEngineConfig(bypass=True),
    portfolio=PortfolioConfig(min_account_state_logging_interval_ms=1_000),
    exec_clients={
        BITGET_VENUE: BitgetExecClientConfig(
            instrument_provider=InstrumentProviderConfig(
                load_all=False,
                load_ids=frozenset({INSTRUMENT_ID}),
            ),
            demo=True,
        ),
    },
    timeout_connection=20.0,
    timeout_reconciliation=10.0,
    timeout_portfolio=10.0,
    timeout_disconnection=10.0,
    timeout_post_stop=1.0,
)

node = TradingNode(config=config_node)
node.add_exec_client_factory(BITGET_VENUE, BitgetLiveExecClientFactory)
node.build()


if __name__ == "__main__":
    try:
        node.run()
    finally:
        node.dispose()
