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
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.data.venue import InstrumentClosePrice
from nautilus_trader.model.data.venue import InstrumentStatusUpdate
from nautilus_trader.model.orderbook.book import L2OrderBook
from nautilus_trader.model.orderbook.data import OrderBookDelta
from nautilus_trader.model.orderbook.data import OrderBookDeltas
from nautilus_trader.model.orderbook.data import OrderBookSnapshot
from tests.integration_tests.adapters.betfair.test_kit import BetfairDataProvider
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs


def test_betfair_orderbook(betfair_data_client, provider):
    provider.load_all()
    book = L2OrderBook(
        instrument_id=BetfairTestStubs.instrument_id(),
        price_precision=2,
        size_precision=2,
    )
    for update in BetfairDataProvider.raw_market_updates():
        for message in on_market_update(instrument_provider=provider, update=update):
            try:
                if isinstance(message, OrderBookSnapshot):
                    book.apply_snapshot(message)
                elif isinstance(message, OrderBookDeltas):
                    book.apply_deltas(message)
                elif isinstance(message, OrderBookDelta):
                    book.apply_delta(message)
                elif isinstance(message, (TradeTick, InstrumentStatusUpdate, InstrumentClosePrice)):
                    pass
                else:
                    raise NotImplementedError(str(type(message)))
            except Exception as ex:
                print(str(type(ex)) + " " + str(ex))
