# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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
    | connect                               | required    |
    | disconnect                            | required    |
    | reset                                 | optional    |
    | dispose                               | optional    |
    +---------------------------------------+-------------+
    | subscribe                             | optional    |
    | unsubscribe                           | optional    |
    +---------------------------------------+-------------+
    | request                               | optional    |
    +---------------------------------------+-------------+

    """

    def connect(self) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def disconnect(self) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def reset(self) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def dispose(self) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    # -- SUBSCRIPTIONS ----------------------------------------------------------------------------

    def subscribe(self, data_type: DataType) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def unsubscribe(self, data_type: DataType) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    # -- REQUESTS ---------------------------------------------------------------------------------

    def request(self, datatype: DataType, correlation_id: UUID4) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover


class TemplateLiveMarketDataClient(LiveMarketDataClient):
    """
    An example of a ``LiveMarketDataClient`` highlighting the overridable abstract methods.

    A live market data client general handles market data feeds and requests.

    +---------------------------------------+-------------+
    | Method                                | Requirement |
    +---------------------------------------+-------------+
    | connect                               | required    |
    | disconnect                            | required    |
    | reset                                 | optional    |
    | dispose                               | optional    |
    +---------------------------------------+-------------+
    | subscribe (adapter specific types)    | optional    |
    | subscribe_instruments                 | optional    |
    | subscribe_instrument                  | optional    |
    | subscribe_order_book_deltas           | optional    |
    | subscribe_order_book_snapshots        | optional    |
    | subscribe_ticker                      | optional    |
    | subscribe_quote_ticks                 | optional    |
    | subscribe_trade_ticks                 | optional    |
    | subscribe_bars                        | optional    |
    | subscribe_instrument_status_updates   | optional    |
    | subscribe_instrument_close_prices     | optional    |
    | unsubscribe (adapter specific types)  | optional    |
    | unsubscribe_instruments               | optional    |
    | unsubscribe_instrument                | optional    |
    | unsubscribe_order_book_deltas         | optional    |
    | unsubscribe_order_book_snapshots      | optional    |
    | unsubscribe_ticker                    | optional    |
    | unsubscribe_quote_ticks               | optional    |
    | unsubscribe_trade_ticks               | optional    |
    | unsubscribe_bars                      | optional    |
    | unsubscribe_instrument_status_updates | optional    |
    | unsubscribe_instrument_close_prices   | optional    |
    +---------------------------------------+-------------+
    | request_instrument                    | optional    |
    | request_quote_ticks                   | optional    |
    | request_trade_ticks                   | optional    |
    | request_bars                          | optional    |
    +---------------------------------------+-------------+

    """

    def connect(self) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def disconnect(self) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def reset(self) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def dispose(self) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    # -- SUBSCRIPTIONS ----------------------------------------------------------------------------

    def subscribe(self, data_type: DataType) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def subscribe_instruments(self) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def subscribe_instrument(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def subscribe_order_book_deltas(
        self,
        instrument_id: InstrumentId,
        book_type: BookType,
        depth: Optional[int] = None,
        kwargs: dict = None,
    ) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def subscribe_order_book_snapshots(
        self,
        instrument_id: InstrumentId,
        book_type: BookType,
        depth: Optional[int] = None,
        kwargs: dict = None,
    ) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def subscribe_ticker(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def subscribe_quote_ticks(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def subscribe_trade_ticks(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def subscribe_bars(self, bar_type: BarType) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def subscribe_instrument_status_updates(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def subscribe_instrument_close_prices(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def unsubscribe(self, data_type: DataType) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def unsubscribe_instruments(self) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def unsubscribe_instrument(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def unsubscribe_order_book_deltas(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def unsubscribe_order_book_snapshots(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def unsubscribe_ticker(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def unsubscribe_quote_ticks(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def unsubscribe_trade_ticks(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def unsubscribe_bars(self, bar_type: BarType) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def unsubscribe_instrument_status_updates(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def unsubscribe_instrument_close_prices(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    # -- REQUESTS ---------------------------------------------------------------------------------

    def request_instrument(self, instrument_id: InstrumentId, correlation_id: UUID4):
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def request_quote_ticks(
        self,
        instrument_id: InstrumentId,
        limit: int,
        correlation_id: UUID4,
        from_datetime: Optional[pd.Timestamp] = None,
        to_datetime: Optional[pd.Timestamp] = None,
    ) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def request_trade_ticks(
        self,
        instrument_id: InstrumentId,
        limit: int,
        correlation_id: UUID4,
        from_datetime: Optional[pd.Timestamp] = None,
        to_datetime: Optional[pd.Timestamp] = None,
    ) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def request_bars(
        self,
        bar_type: BarType,
        limit: int,
        correlation_id: UUID4,
        from_datetime: Optional[pd.Timestamp] = None,
        to_datetime: Optional[pd.Timestamp] = None,
    ) -> None:
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover
