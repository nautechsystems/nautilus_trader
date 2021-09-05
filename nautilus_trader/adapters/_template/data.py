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

from nautilus_trader.core.uuid import UUID4
from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.model.data.bar import BarType
from nautilus_trader.model.data.base import DataType
from nautilus_trader.model.enums import BookLevel
from nautilus_trader.model.identifiers import InstrumentId


class TemplateLiveMarketDataClient(LiveMarketDataClient):
    def connect(self):  # pragma: no cover
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    def disconnect(self):  # pragma: no cover
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    def reset(self):  # pragma: no cover
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    def dispose(self):  # pragma: no cover
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    # -- SUBSCRIPTIONS ---------------------------------------------------------------------------------

    def subscribe(self, data_type: DataType):  # pragma: no cover
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    def unsubscribe(self, data_type: DataType):  # pragma: no cover
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    def subscribe_instrument(self, instrument_id: InstrumentId):  # pragma: no cover
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    def subscribe_order_book(  # pragma: no cover
        self, instrument_id: InstrumentId, level: BookLevel, depth: int = 0, kwargs: dict = None
    ):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    def subscribe_order_book_deltas(  # pragma: no cover
        self, instrument_id: InstrumentId, level: BookLevel, kwargs: dict = None
    ):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    def subscribe_quote_ticks(self, instrument_id: InstrumentId):  # pragma: no cover
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    def subscribe_trade_ticks(self, instrument_id: InstrumentId):  # pragma: no cover
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    def subscribe_venue_status_update(self, instrument_id: InstrumentId):  # pragma: no cover
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    def subscribe_instrument_status_updates(self, instrument_id: InstrumentId):  # pragma: no cover
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    def subscribe_instrument_close_prices(self, instrument_id: InstrumentId):  # pragma: no cover
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    def subscribe_bars(self, bar_type: BarType):  # pragma: no cover
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    def unsubscribe_instrument(self, instrument_id: InstrumentId):  # pragma: no cover
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    def unsubscribe_order_book(self, instrument_id: InstrumentId):  # pragma: no cover
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    def unsubscribe_order_book_deltas(self, instrument_id: InstrumentId):  # pragma: no cover
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    def unsubscribe_quote_ticks(self, instrument_id: InstrumentId):  # pragma: no cover
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    def unsubscribe_trade_ticks(self, instrument_id: InstrumentId):  # pragma: no cover
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    def unsubscribe_bars(self, bar_type: BarType):  # pragma: no cover
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    def unsubscribe_venue_status_update(self, instrument_id: InstrumentId):  # pragma: no cover
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    def unsubscribe_instrument_status_updates(
        self, instrument_id: InstrumentId
    ):  # pragma: no cover
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    def unsubscribe_instrument_close_prices(self, instrument_id: InstrumentId):  # pragma: no cover
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    # -- REQUESTS --------------------------------------------------------------------------------------

    def request(self, datatype: DataType, correlation_id: UUID4):  # pragma: no cover
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    def request_instrument(
        self, instrument_id: InstrumentId, correlation_id: UUID4
    ):  # pragma: no cover
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    def request_instruments(self, correlation_id: UUID4):  # pragma: no cover
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")
