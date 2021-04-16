# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.adapters.betfair.data import on_market_update
from nautilus_trader.model.c_enums.orderbook_level import OrderBookLevel
from nautilus_trader.model.events import InstrumentClosePrice
from nautilus_trader.model.events import InstrumentStatusEvent
from nautilus_trader.model.orderbook.book import OrderBook
from nautilus_trader.model.orderbook.book import OrderBookDelta
from nautilus_trader.model.orderbook.book import OrderBookDeltas
from nautilus_trader.model.orderbook.book import OrderBookSnapshot
from nautilus_trader.model.tick import TradeTick
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs


def test_betfair_orderbook(betfair_data_client, provider):
    provider.load_all()
    book = OrderBook(
        instrument_id=BetfairTestStubs.instrument_id(), level=OrderBookLevel.L2
    )
    for update in BetfairTestStubs.raw_market_updates():
        for message in on_market_update(self=betfair_data_client, update=update):
            if isinstance(message, OrderBookSnapshot):
                book.apply_snapshot(message)
            elif isinstance(message, OrderBookDeltas):
                book.apply_deltas(message)
            elif isinstance(message, OrderBookDelta):
                book.apply_delta(message)
            elif isinstance(
                message, (TradeTick, InstrumentStatusEvent, InstrumentClosePrice)
            ):
                pass
            else:
                raise NotImplementedError(str(type(message)))
