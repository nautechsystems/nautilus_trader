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

"""Bitget public market-data tester."""

from nautilus_trader.adapters.bitget.config import BitgetDataClientConfig
from nautilus_trader.adapters.bitget.constants import BITGET_VENUE
from nautilus_trader.adapters.bitget.factories import BitgetLiveDataClientFactory
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.data import BarType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.test_kit.strategies.tester_data import DataTester
from nautilus_trader.test_kit.strategies.tester_data import DataTesterConfig


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***

INSTRUMENT_ID = InstrumentId.from_str("BTCUSDT-PERP.BITGET")

config_node = TradingNodeConfig(
    trader_id=TraderId("TESTER-001"),
    logging=LoggingConfig(
        log_level="INFO",
        use_pyo3=True,
    ),
    data_clients={
        BITGET_VENUE: BitgetDataClientConfig(
            instrument_provider=InstrumentProviderConfig(
                load_all=False,
                load_ids=frozenset({INSTRUMENT_ID}),
            ),
            demo=False,
        ),
    },
    timeout_connection=20.0,
    timeout_reconciliation=10.0,
    timeout_portfolio=10.0,
    timeout_disconnection=10.0,
    timeout_post_stop=1.0,
)

node = TradingNode(config=config_node)

config_tester = DataTesterConfig(
    instrument_ids=[INSTRUMENT_ID],
    bar_types=[BarType.from_str(f"{INSTRUMENT_ID.value}-1-MINUTE-LAST-EXTERNAL")],
    subscribe_instrument=True,
    subscribe_quotes=True,
    subscribe_trades=True,
    subscribe_book_deltas=True,
    subscribe_mark_prices=True,
    subscribe_index_prices=True,
    subscribe_funding_rates=True,
    subscribe_bars=True,
)

tester = DataTester(config=config_tester)
node.trader.add_actor(tester)

node.add_data_client_factory(BITGET_VENUE, BitgetLiveDataClientFactory)
node.build()


if __name__ == "__main__":
    try:
        node.run()
    finally:
        node.dispose()
