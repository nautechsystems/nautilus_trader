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

from cpython.datetime cimport datetime

from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.component cimport Component
from nautilus_trader.core.data cimport Data
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.model.data.bar cimport Bar
from nautilus_trader.model.data.bar cimport BarType
from nautilus_trader.model.data.base cimport DataType
from nautilus_trader.model.enums_c cimport BookType
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instruments.base cimport Instrument


cdef class DataClient(Component):
    cdef readonly Cache _cache
    cdef set _subscriptions_generic

    cdef readonly Venue venue
    """The clients venue ID (if not a routing client).\n\n:returns: `Venue` or ``None``"""
    cdef readonly bint is_connected
    """If the client is connected.\n\n:returns: `bool`"""

    cpdef void _set_connected(self, bint value=*) except *

# -- SUBSCRIPTIONS --------------------------------------------------------------------------------

    cpdef list subscribed_generic_data(self)

    cpdef void subscribe(self, DataType data_type) except *
    cpdef void unsubscribe(self, DataType data_type) except *

    cpdef void _add_subscription(self, DataType data_type) except *
    cpdef void _remove_subscription(self, DataType data_type) except *

# -- REQUEST HANDLERS -----------------------------------------------------------------------------

    cpdef void request(self, DataType data_type, UUID4 correlation_id) except *

# -- DATA HANDLERS --------------------------------------------------------------------------------

    cpdef void _handle_data(self, Data data) except *
    cpdef void _handle_data_response(self, DataType data_type, data, UUID4 correlation_id) except *


cdef class MarketDataClient(DataClient):
    cdef set _subscriptions_order_book_delta
    cdef set _subscriptions_order_book_snapshot
    cdef set _subscriptions_ticker
    cdef set _subscriptions_quote_tick
    cdef set _subscriptions_trade_tick
    cdef set _subscriptions_bar
    cdef set _subscriptions_venue_status_update
    cdef set _subscriptions_instrument_status_update
    cdef set _subscriptions_instrument_close
    cdef set _subscriptions_instrument

    cdef object _update_instruments_task

# -- SUBSCRIPTIONS --------------------------------------------------------------------------------

    cpdef list subscribed_instruments(self)
    cpdef list subscribed_order_book_deltas(self)
    cpdef list subscribed_order_book_snapshots(self)
    cpdef list subscribed_tickers(self)
    cpdef list subscribed_quote_ticks(self)
    cpdef list subscribed_trade_ticks(self)
    cpdef list subscribed_bars(self)
    cpdef list subscribed_venue_status_updates(self)
    cpdef list subscribed_instrument_status_updates(self)
    cpdef list subscribed_instrument_close(self)

    cpdef void subscribe_instruments(self) except *
    cpdef void subscribe_instrument(self, InstrumentId instrument_id) except *
    cpdef void subscribe_order_book_deltas(self, InstrumentId instrument_id, BookType book_type, int depth=*, dict kwargs=*) except *
    cpdef void subscribe_order_book_snapshots(self, InstrumentId instrument_id, BookType book_type, int depth=*, dict kwargs=*) except *
    cpdef void subscribe_ticker(self, InstrumentId instrument_id) except *
    cpdef void subscribe_quote_ticks(self, InstrumentId instrument_id) except *
    cpdef void subscribe_trade_ticks(self, InstrumentId instrument_id) except *
    cpdef void subscribe_bars(self, BarType bar_type) except *
    cpdef void subscribe_venue_status_updates(self, Venue venue) except *
    cpdef void subscribe_instrument_status_updates(self, InstrumentId instrument_id) except *
    cpdef void subscribe_instrument_close(self, InstrumentId instrument_id) except *
    cpdef void unsubscribe_instruments(self) except *
    cpdef void unsubscribe_instrument(self, InstrumentId instrument_id) except *
    cpdef void unsubscribe_order_book_deltas(self, InstrumentId instrument_id) except *
    cpdef void unsubscribe_order_book_snapshots(self, InstrumentId instrument_id) except *
    cpdef void unsubscribe_ticker(self, InstrumentId instrument_id) except *
    cpdef void unsubscribe_quote_ticks(self, InstrumentId instrument_id) except *
    cpdef void unsubscribe_trade_ticks(self, InstrumentId instrument_id) except *
    cpdef void unsubscribe_bars(self, BarType bar_type) except *
    cpdef void unsubscribe_instrument_status_updates(self, InstrumentId instrument_id) except *
    cpdef void unsubscribe_venue_status_updates(self, Venue venue) except *
    cpdef void unsubscribe_instrument_close(self, InstrumentId instrument_id) except *

    cpdef void _add_subscription_instrument(self, InstrumentId instrument_id) except *
    cpdef void _add_subscription_order_book_deltas(self, InstrumentId instrument_id) except *
    cpdef void _add_subscription_order_book_snapshots(self, InstrumentId instrument_id) except *
    cpdef void _add_subscription_ticker(self, InstrumentId instrument_id) except *
    cpdef void _add_subscription_quote_ticks(self, InstrumentId instrument_id) except *
    cpdef void _add_subscription_trade_ticks(self, InstrumentId instrument_id) except *
    cpdef void _add_subscription_bars(self, BarType bar_type) except *
    cpdef void _add_subscription_venue_status_updates(self, Venue venue) except *
    cpdef void _add_subscription_instrument_status_updates(self, InstrumentId instrument_id) except *
    cpdef void _add_subscription_instrument_close(self, InstrumentId instrument_id) except *
    cpdef void _remove_subscription_instrument(self, InstrumentId instrument_id) except *
    cpdef void _remove_subscription_order_book_deltas(self, InstrumentId instrument_id) except *
    cpdef void _remove_subscription_order_book_snapshots(self, InstrumentId instrument_id) except *
    cpdef void _remove_subscription_ticker(self, InstrumentId instrument_id) except *
    cpdef void _remove_subscription_quote_ticks(self, InstrumentId instrument_id) except *
    cpdef void _remove_subscription_trade_ticks(self, InstrumentId instrument_id) except *
    cpdef void _remove_subscription_bars(self, BarType bar_type) except *
    cpdef void _remove_subscription_venue_status_updates(self, Venue venue) except *
    cpdef void _remove_subscription_instrument_status_updates(self, InstrumentId instrument_id) except *
    cpdef void _remove_subscription_instrument_close(self, InstrumentId instrument_id) except *

# -- REQUEST HANDLERS -----------------------------------------------------------------------------

    cpdef void request_instrument(self, InstrumentId instrument_id, UUID4 correlation_id) except *
    cpdef void request_instruments(self, Venue venue, UUID4 correlation_id) except *
    cpdef void request_quote_ticks(
        self,
        InstrumentId instrument_id,
        int limit,
        UUID4 correlation_id,
        datetime from_datetime=*,
        datetime to_datetime=*,
    ) except *
    cpdef void request_trade_ticks(
        self,
        InstrumentId instrument_id,
        int limit,
        UUID4 correlation_id,
        datetime from_datetime=*,
        datetime to_datetime=*,
    ) except *
    cpdef void request_bars(
        self,
        BarType bar_type,
        int limit,
        UUID4 correlation_id,
        datetime from_datetime=*,
        datetime to_datetime=*,
    ) except *

# -- DATA HANDLERS --------------------------------------------------------------------------------

    cpdef void _handle_instrument(self, Instrument instrument, UUID4 correlation_id) except *
    cpdef void _handle_instruments(self, Venue venue, list instruments, UUID4 correlation_id) except *
    cpdef void _handle_quote_ticks(self, InstrumentId instrument_id, list ticks, UUID4 correlation_id) except *
    cpdef void _handle_trade_ticks(self, InstrumentId instrument_id, list ticks, UUID4 correlation_id) except *
    cpdef void _handle_bars(self, BarType bar_type, list bars, Bar partial, UUID4 correlation_id) except *
