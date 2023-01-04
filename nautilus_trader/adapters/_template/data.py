# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from typing import Optional

import pandas as pd

from nautilus_trader.core.uuid import UUID4
from nautilus_trader.live.data_client import LiveDataClient
from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.model.data.bar import BarType
from nautilus_trader.model.data.base import DataType
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Venue


# The 'pragma: no cover' comment excludes a method from test coverage.
# https://coverage.readthedocs.io/en/coverage-4.3.3/excluding.html
# The reason for their use is to reduce redundant/needless tests which simply
# assert that a `NotImplementedError` is raised when calling abstract methods.
# These tests are expensive to maintain (as they must be kept in line with any
# refactorings), and offer little to no benefit in return. However, the intention
# is for all method implementations to be fully covered by tests.

# *** THESE PRAGMA: NO COVER COMMENTS MUST BE REMOVED IN ANY IMPLEMENTATION. ***


class TemplateLiveDataClient(LiveDataClient):
    """
    An example of a ``LiveDataClient`` highlighting the overridable abstract methods.

    A live data client general handles non-market or custom data feeds and requests.

    +---------------------------------------+-------------+
    | Method                                | Requirement |
    +---------------------------------------+-------------+
    | _connect                              | required    |
    | _disconnect                           | required    |
    | reset                                 | optional    |
    | dispose                               | optional    |
    +---------------------------------------+-------------+
    | _subscribe                            | optional    |
    | _unsubscribe                          | optional    |
    +---------------------------------------+-------------+
    | _request                              | optional    |
    +---------------------------------------+-------------+
    """

    async def _connect(self) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    async def _disconnect(self) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def reset(self) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def dispose(self) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    # -- SUBSCRIPTIONS ----------------------------------------------------------------------------

    async def _subscribe(self, data_type: DataType) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    async def _unsubscribe(self, data_type: DataType) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    # -- REQUESTS ---------------------------------------------------------------------------------

    async def _request(self, data_type: DataType, correlation_id: UUID4) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover


class TemplateLiveMarketDataClient(LiveMarketDataClient):
    """
    An example of a ``LiveMarketDataClient`` highlighting the overridable abstract methods.

    A live market data client general handles market data feeds and requests.

    +----------------------------------------+-------------+
    | Method                                 | Requirement |
    +----------------------------------------+-------------+
    | _connect                               | required    |
    | _disconnect                            | required    |
    | reset                                  | optional    |
    | dispose                                | optional    |
    +----------------------------------------+-------------+
    | _subscribe (adapter specific types)    | optional    |
    | _subscribe_instruments                 | optional    |
    | _subscribe_instrument                  | optional    |
    | _subscribe_order_book_deltas           | optional    |
    | _subscribe_order_book_snapshots        | optional    |
    | _subscribe_ticker                      | optional    |
    | _subscribe_quote_ticks                 | optional    |
    | _subscribe_trade_ticks                 | optional    |
    | _subscribe_bars                        | optional    |
    | _subscribe_instrument_status_updates   | optional    |
    | _subscribe_instrument_close            | optional    |
    | _unsubscribe (adapter specific types)  | optional    |
    | _unsubscribe_instruments               | optional    |
    | _unsubscribe_instrument                | optional    |
    | _unsubscribe_order_book_deltas         | optional    |
    | _unsubscribe_order_book_snapshots      | optional    |
    | _unsubscribe_ticker                    | optional    |
    | _unsubscribe_quote_ticks               | optional    |
    | _unsubscribe_trade_ticks               | optional    |
    | _unsubscribe_bars                      | optional    |
    | _unsubscribe_instrument_status_updates | optional    |
    | _unsubscribe_instrument_close          | optional    |
    +----------------------------------------+-------------+
    | _request                               | optional    |
    | _request_instrument                    | optional    |
    | _request_instruments                   | optional    |
    | _request_quote_ticks                   | optional    |
    | _request_trade_ticks                   | optional    |
    | _request_bars                          | optional    |
    +----------------------------------------+-------------+
    """

    async def _connect(self) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    async def _disconnect(self) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def reset(self) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def dispose(self) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    # -- SUBSCRIPTIONS ----------------------------------------------------------------------------

    async def _subscribe(self, data_type: DataType) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    async def _subscribe_instruments(self) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    async def _subscribe_instrument(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    async def _subscribe_order_book_deltas(
        self,
        instrument_id: InstrumentId,
        book_type: BookType,
        depth: Optional[int] = None,
        kwargs: dict = None,
    ) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    async def _subscribe_order_book_snapshots(
        self,
        instrument_id: InstrumentId,
        book_type: BookType,
        depth: Optional[int] = None,
        kwargs: dict = None,
    ) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    async def _subscribe_ticker(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    async def _subscribe_quote_ticks(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    async def _subscribe_trade_ticks(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    async def _subscribe_bars(self, bar_type: BarType) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    async def _subscribe_instrument_status_updates(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    async def _subscribe_instrument_close(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    async def _unsubscribe(self, data_type: DataType) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    async def _unsubscribe_instruments(self) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    async def _unsubscribe_instrument(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    async def _unsubscribe_order_book_deltas(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    async def _unsubscribe_order_book_snapshots(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    async def _unsubscribe_ticker(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    async def _unsubscribe_quote_ticks(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    async def _unsubscribe_trade_ticks(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    async def _unsubscribe_bars(self, bar_type: BarType) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    async def _unsubscribe_instrument_status_updates(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    async def _unsubscribe_instrument_close(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    # -- REQUESTS ---------------------------------------------------------------------------------

    async def _request(self, data_type: DataType, correlation_id: UUID4) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    async def _request_instrument(self, instrument_id: InstrumentId, correlation_id: UUID4):
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    async def _request_instruments(self, venue: Venue, correlation_id: UUID4):
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    async def _request_quote_ticks(
        self,
        instrument_id: InstrumentId,
        limit: int,
        correlation_id: UUID4,
        from_datetime: Optional[pd.Timestamp] = None,
        to_datetime: Optional[pd.Timestamp] = None,
    ) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    async def _request_trade_ticks(
        self,
        instrument_id: InstrumentId,
        limit: int,
        correlation_id: UUID4,
        from_datetime: Optional[pd.Timestamp] = None,
        to_datetime: Optional[pd.Timestamp] = None,
    ) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    async def _request_bars(
        self,
        bar_type: BarType,
        limit: int,
        correlation_id: UUID4,
        from_datetime: Optional[pd.Timestamp] = None,
        to_datetime: Optional[pd.Timestamp] = None,
    ) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover
