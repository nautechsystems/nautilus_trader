# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.data.messages import RequestBars
from nautilus_trader.data.messages import RequestData
from nautilus_trader.data.messages import RequestInstrument
from nautilus_trader.data.messages import RequestInstruments
from nautilus_trader.data.messages import RequestOrderBookSnapshot
from nautilus_trader.data.messages import RequestQuoteTicks
from nautilus_trader.data.messages import RequestTradeTicks
from nautilus_trader.data.messages import SubscribeBars
from nautilus_trader.data.messages import SubscribeData
from nautilus_trader.data.messages import SubscribeFundingRates
from nautilus_trader.data.messages import SubscribeIndexPrices
from nautilus_trader.data.messages import SubscribeInstrument
from nautilus_trader.data.messages import SubscribeInstrumentClose
from nautilus_trader.data.messages import SubscribeInstruments
from nautilus_trader.data.messages import SubscribeInstrumentStatus
from nautilus_trader.data.messages import SubscribeMarkPrices
from nautilus_trader.data.messages import SubscribeOrderBook
from nautilus_trader.data.messages import SubscribeQuoteTicks
from nautilus_trader.data.messages import SubscribeTradeTicks
from nautilus_trader.data.messages import UnsubscribeBars
from nautilus_trader.data.messages import UnsubscribeData
from nautilus_trader.data.messages import UnsubscribeFundingRates
from nautilus_trader.data.messages import UnsubscribeIndexPrices
from nautilus_trader.data.messages import UnsubscribeInstrument
from nautilus_trader.data.messages import UnsubscribeInstrumentClose
from nautilus_trader.data.messages import UnsubscribeInstruments
from nautilus_trader.data.messages import UnsubscribeInstrumentStatus
from nautilus_trader.data.messages import UnsubscribeMarkPrices
from nautilus_trader.data.messages import UnsubscribeOrderBook
from nautilus_trader.data.messages import UnsubscribeQuoteTicks
from nautilus_trader.data.messages import UnsubscribeTradeTicks
from nautilus_trader.live.data_client import LiveDataClient
from nautilus_trader.live.data_client import LiveMarketDataClient


# The 'pragma: no cover' comment excludes a method from test coverage.
# https://coverage.readthedocs.io/en/coverage-4.3.3/excluding.html
# The reason for their use is to reduce redundant/needless tests which simply
# assert that a `NotImplementedError` is raised when calling abstract methods.
# These tests are expensive to maintain (as they must be kept in line with any
# refactorings), and offer little to no benefit in return. The intention
# is for all method implementations to be fully covered by tests.

# *** THESE PRAGMA: NO COVER COMMENTS MUST BE REMOVED IN ANY IMPLEMENTATION. ***


class TemplateLiveDataClient(LiveDataClient):
    """
    An example of a ``LiveDataClient`` highlighting the overridable abstract methods.

    A live data client generally handles non-market or custom data feeds and requests.

    +---------------------------------------+-------------+
    | Method                                | Requirement |
    +---------------------------------------+-------------+
    | _connect                              | required    |
    | _disconnect                           | required    |
    +---------------------------------------+-------------+
    | _subscribe                            | optional    |
    | _unsubscribe                          | optional    |
    +---------------------------------------+-------------+
    | _request                              | optional    |
    +---------------------------------------+-------------+

    """

    async def _connect(self) -> None:
        raise NotImplementedError(
            "method `_connect` must be implemented in the subclass",
        )  # pragma: no cover

    async def _disconnect(self) -> None:
        raise NotImplementedError(
            "method `_disconnect` must be implemented in the subclass",
        )  # pragma: no cover

    # -- SUBSCRIPTIONS ----------------------------------------------------------------------------

    async def _subscribe(self, command: SubscribeData) -> None:
        raise NotImplementedError(
            "method `_subscribe` must be implemented in the subclass",
        )  # pragma: no cover

    async def _unsubscribe(self, command: UnsubscribeData) -> None:
        raise NotImplementedError(
            "method `_unsubscribe` must be implemented in the subclass",
        )  # pragma: no cover

    # -- REQUESTS ---------------------------------------------------------------------------------

    async def _request(self, request: RequestData) -> None:
        raise NotImplementedError(
            "method `_request` must be implemented in the subclass",
        )  # pragma: no cover


class TemplateLiveMarketDataClient(LiveMarketDataClient):
    """
    An example of a ``LiveMarketDataClient`` highlighting the overridable abstract
    methods.

    A live market data client generally handles market data feeds and requests.

    +----------------------------------------+-------------+
    | Method                                 | Requirement |
    +----------------------------------------+-------------+
    | _connect                               | required    |
    | _disconnect                            | required    |
    +----------------------------------------+-------------+
    | _subscribe (adapter specific types)    | optional    |
    | _subscribe_instruments                 | optional    |
    | _subscribe_instrument                  | optional    |
    | _subscribe_order_book_deltas           | optional    |
    | _subscribe_order_book_snapshots        | optional    |
    | _subscribe_quote_ticks                 | optional    |
    | _subscribe_trade_ticks                 | optional    |
    | _subscribe_mark_prices                 | optional    |
    | _subscribe_index_prices                | optional    |
    | _subscribe_funding_rates               | optional    |
    | _subscribe_bars                        | optional    |
    | _subscribe_instrument_status           | optional    |
    | _subscribe_instrument_close            | optional    |
    | _unsubscribe (adapter specific types)  | optional    |
    | _unsubscribe_instruments               | optional    |
    | _unsubscribe_instrument                | optional    |
    | _unsubscribe_order_book_deltas         | optional    |
    | _unsubscribe_order_book_snapshots      | optional    |
    | _unsubscribe_quote_ticks               | optional    |
    | _unsubscribe_trade_ticks               | optional    |
    | _unsubscribe_mark_prices               | optional    |
    | _unsubscribe_index_prices              | optional    |
    | _unsubscribe_funding_rates             | optional    |
    | _unsubscribe_bars                      | optional    |
    | _unsubscribe_instrument_status         | optional    |
    | _unsubscribe_instrument_close          | optional    |
    +----------------------------------------+-------------+
    | _request                               | optional    |
    | _request_instrument                    | optional    |
    | _request_instruments                   | optional    |
    | _request_order_book_snapshot           | optional    |
    | _request_quote_ticks                   | optional    |
    | _request_trade_ticks                   | optional    |
    | _request_bars                          | optional    |
    +----------------------------------------+-------------+

    """

    async def _connect(self) -> None:
        raise NotImplementedError(
            "method `_connect` must be implemented in the subclass",
        )  # pragma: no cover

    async def _disconnect(self) -> None:
        raise NotImplementedError(
            "method `_disconnect` must be implemented in the subclass",
        )  # pragma: no cover

    # -- SUBSCRIPTIONS ----------------------------------------------------------------------------

    async def _subscribe(self, command: SubscribeData) -> None:
        raise NotImplementedError(
            "method `_subscribe` must be implemented in the subclass",
        )  # pragma: no cover

    async def _subscribe_instruments(self, command: SubscribeInstruments) -> None:
        raise NotImplementedError(
            "method `_subscribe_instruments` must be implemented in the subclass",
        )  # pragma: no cover

    async def _subscribe_instrument(self, command: SubscribeInstrument) -> None:
        raise NotImplementedError(
            "method `_subscribe_instrument` must be implemented in the subclass",
        )  # pragma: no cover

    async def _subscribe_order_book_deltas(self, command: SubscribeOrderBook) -> None:
        raise NotImplementedError(
            "method `_subscribe_order_book_deltas` must be implemented in the subclass",
        )  # pragma: no cover

    async def _subscribe_order_book_snapshots(self, command: SubscribeOrderBook) -> None:
        raise NotImplementedError(
            "method `_subscribe_order_book_snapshots` must be implemented in the subclass",
        )  # pragma: no cover

    async def _subscribe_quote_ticks(self, command: SubscribeQuoteTicks) -> None:
        raise NotImplementedError(
            "method `_subscribe_quote_ticks` must be implemented in the subclass",
        )  # pragma: no cover

    async def _subscribe_trade_ticks(self, command: SubscribeTradeTicks) -> None:
        raise NotImplementedError(
            "method `_subscribe_trade_ticks` must be implemented in the subclass",
        )  # pragma: no cover

    async def _subscribe_mark_prices(self, command: SubscribeMarkPrices) -> None:
        raise NotImplementedError(
            "method `_subscribe_mark_prices` must be implemented in the subclass",
        )  # pragma: no cover

    async def _subscribe_index_prices(self, command: SubscribeIndexPrices) -> None:
        raise NotImplementedError(
            "method `_subscribe_index_prices` must be implemented in the subclass",
        )  # pragma: no cover

    async def _subscribe_funding_rates(self, command: SubscribeFundingRates) -> None:
        raise NotImplementedError(
            "method `_subscribe_funding_rates` must be implemented in the subclass",
        )  # pragma: no cover

    async def _subscribe_bars(self, command: SubscribeBars) -> None:
        raise NotImplementedError(
            "method `_subscribe_bars` must be implemented in the subclass",
        )  # pragma: no cover

    async def _subscribe_instrument_status(self, command: SubscribeInstrumentStatus) -> None:
        raise NotImplementedError(
            "method `_subscribe_instrument_status` must be implemented in the subclass",
        )  # pragma: no cover

    async def _subscribe_instrument_close(self, command: SubscribeInstrumentClose) -> None:
        raise NotImplementedError(
            "method `_subscribe_instrument_close` must be implemented in the subclass",
        )  # pragma: no cover

    async def _unsubscribe(self, command: UnsubscribeData) -> None:
        raise NotImplementedError(
            "method `_unsubscribe` must be implemented in the subclass",
        )  # pragma: no cover

    async def _unsubscribe_instruments(self, command: UnsubscribeInstruments) -> None:
        raise NotImplementedError(
            "method `_unsubscribe_instruments` must be implemented in the subclass",
        )  # pragma: no cover

    async def _unsubscribe_instrument(self, command: UnsubscribeInstrument) -> None:
        raise NotImplementedError(
            "method `_unsubscribe_instrument` must be implemented in the subclass",
        )  # pragma: no cover

    async def _unsubscribe_order_book_deltas(self, command: UnsubscribeOrderBook) -> None:
        raise NotImplementedError(
            "method `_unsubscribe_order_book_deltas` must be implemented in the subclass",
        )  # pragma: no cover

    async def _unsubscribe_order_book_snapshots(self, command: UnsubscribeOrderBook) -> None:
        raise NotImplementedError(
            "method `_unsubscribe_order_book_snapshots` must be implemented in the subclass",
        )  # pragma: no cover

    async def _unsubscribe_quote_ticks(self, command: UnsubscribeQuoteTicks) -> None:
        raise NotImplementedError(
            "method `_unsubscribe_quote_tick` must be implemented in the subclass",
        )  # pragma: no cover

    async def _unsubscribe_trade_ticks(self, command: UnsubscribeTradeTicks) -> None:
        raise NotImplementedError(
            "method `_unsubscribe_trade_ticks` must be implemented in the subclass",
        )  # pragma: no cover

    async def _unsubscribe_mark_prices(self, command: UnsubscribeMarkPrices) -> None:
        raise NotImplementedError(
            "method `_unsubscribe_mark_prices` must be implemented in the subclass",
        )  # pragma: no cover

    async def _unsubscribe_index_prices(self, command: UnsubscribeIndexPrices) -> None:
        raise NotImplementedError(
            "method `_unsubscribe_index_prices` must be implemented in the subclass",
        )  # pragma: no cover

    async def _unsubscribe_funding_rates(self, command: UnsubscribeFundingRates) -> None:
        raise NotImplementedError(
            "method `_unsubscribe_funding_rates` must be implemented in the subclass",
        )  # pragma: no cover

    async def _unsubscribe_bars(self, command: UnsubscribeBars) -> None:
        raise NotImplementedError(
            "method `_unsubscribe_bars` must be implemented in the subclass",
        )  # pragma: no cover

    async def _unsubscribe_instrument_status(self, command: UnsubscribeInstrumentStatus) -> None:
        raise NotImplementedError(
            "method `_unsubscribe_instrument_status` must be implemented in the subclass",
        )  # pragma: no cover

    async def _unsubscribe_instrument_close(self, command: UnsubscribeInstrumentClose) -> None:
        raise NotImplementedError(
            "method `_unsubscribe_instrument_close` must be implemented in the subclass",
        )  # pragma: no cover

    # -- REQUESTS ---------------------------------------------------------------------------------

    async def _request(self, request: RequestData) -> None:
        raise NotImplementedError(
            "method `_request` must be implemented in the subclass",
        )  # pragma: no cover

    async def _request_instrument(self, request: RequestInstrument) -> None:
        raise NotImplementedError(
            "method `_request_instrument` must be implemented in the subclass",
        )  # pragma: no cover

    async def _request_instruments(self, request: RequestInstruments) -> None:
        raise NotImplementedError(
            "method `_request_instruments` must be implemented in the subclass",
        )  # pragma: no cover

    async def _request_order_book_snapshot(self, request: RequestOrderBookSnapshot) -> None:
        raise NotImplementedError(
            "method `_request_quote_tick` must be implemented in the subclass",
        )  # pragma: no cover

    async def _request_quote_ticks(self, request: RequestQuoteTicks) -> None:
        raise NotImplementedError(
            "method `_request_quote_tick` must be implemented in the subclass",
        )  # pragma: no cover

    async def _request_trade_ticks(self, request: RequestTradeTicks) -> None:
        raise NotImplementedError(
            "method `_request_trade_ticks` must be implemented in the subclass",
        )  # pragma: no cover

    async def _request_bars(self, request: RequestBars) -> None:
        raise NotImplementedError(
            "method `_request_bars` must be implemented in the subclass",
        )  # pragma: no cover
