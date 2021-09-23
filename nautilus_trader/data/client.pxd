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

from cpython.datetime cimport datetime

from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.component cimport Component
from nautilus_trader.core.data cimport Data
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.model.c_enums.book_type cimport BookType
from nautilus_trader.model.data.bar cimport Bar
from nautilus_trader.model.data.bar cimport BarType
from nautilus_trader.model.data.base cimport DataType
from nautilus_trader.model.identifiers cimport InstrumentId


cdef class DataClient(Component):
    cdef Cache _cache

    cdef dict _feeds_generic_data

    cdef readonly bint is_connected
    """If the client is connected.\n\n:returns: `bool`"""

# -- SUBSCRIPTIONS ---------------------------------------------------------------------------------

    cpdef list subscribed_generic_data(self)

    cpdef void subscribe(self, DataType data_type) except *
    cpdef void unsubscribe(self, DataType data_type) except *

# -- REQUEST HANDLERS ------------------------------------------------------------------------------

    cpdef void request(self, DataType data_type, UUID4 correlation_id) except *

# -- DATA HANDLERS ---------------------------------------------------------------------------------

    cdef void _handle_data(self, Data data) except *
    cdef void _handle_data_response(self, DataType data_type, Data data, UUID4 correlation_id) except *


cdef class MarketDataClient(DataClient):
    cdef dict _feeds_order_book_delta
    cdef dict _feeds_order_book_snapshot
    cdef dict _feeds_ticker
    cdef dict _feeds_quote_tick
    cdef dict _feeds_trade_tick
    cdef dict _feeds_bar
    cdef dict _feeds_instrument_status_update
    cdef dict _feeds_instrument_close_price

    cdef set _feeds_instrument
    cdef object _update_instruments_task

    cpdef list unavailable_methods(self)

# -- SUBSCRIPTIONS ---------------------------------------------------------------------------------

    cpdef list subscribed_instruments(self)
    cpdef list subscribed_order_book_deltas(self)
    cpdef list subscribed_order_book_snapshots(self)
    cpdef list subscribed_tickers(self)
    cpdef list subscribed_quote_ticks(self)
    cpdef list subscribed_trade_ticks(self)
    cpdef list subscribed_bars(self)
    cpdef list subscribed_instrument_status_updates(self)
    cpdef list subscribed_instrument_close_prices(self)

    cpdef void subscribe_instruments(self) except *
    cpdef void subscribe_instrument(self, InstrumentId instrument_id) except *
    cpdef void subscribe_order_book_deltas(self, InstrumentId instrument_id, BookType book_type, dict kwargs=*) except *
    cpdef void subscribe_order_book_snapshots(self, InstrumentId instrument_id, BookType book_type, int depth=*, dict kwargs=*) except *
    cpdef void subscribe_ticker(self, InstrumentId instrument_id) except *
    cpdef void subscribe_quote_ticks(self, InstrumentId instrument_id) except *
    cpdef void subscribe_trade_ticks(self, InstrumentId instrument_id) except *
    cpdef void subscribe_bars(self, BarType bar_type) except *
    cpdef void subscribe_venue_status_updates(self, InstrumentId instrument_id) except *
    cpdef void subscribe_instrument_status_updates(self, InstrumentId instrument_id) except *
    cpdef void subscribe_instrument_close_prices(self, InstrumentId instrument_id) except *
    cpdef void unsubscribe_instruments(self) except *
    cpdef void unsubscribe_instrument(self, InstrumentId instrument_id) except *
    cpdef void unsubscribe_order_book_deltas(self, InstrumentId instrument_id) except *
    cpdef void unsubscribe_order_book_snapshots(self, InstrumentId instrument_id) except *
    cpdef void unsubscribe_ticker(self, InstrumentId instrument_id) except *
    cpdef void unsubscribe_quote_ticks(self, InstrumentId instrument_id) except *
    cpdef void unsubscribe_trade_ticks(self, InstrumentId instrument_id) except *
    cpdef void unsubscribe_bars(self, BarType bar_type) except *
    cpdef void unsubscribe_venue_status_updates(self, InstrumentId instrument_id) except *
    cpdef void unsubscribe_instrument_status_updates(self, InstrumentId instrument_id) except *
    cpdef void unsubscribe_instrument_close_prices(self, InstrumentId instrument_id) except *

# -- REQUEST HANDLERS ------------------------------------------------------------------------------

    cpdef void request_quote_ticks(
        self,
        InstrumentId instrument_id,
        datetime from_datetime,
        datetime to_datetime,
        int limit,
        UUID4 correlation_id,
    ) except *
    cpdef void request_trade_ticks(
        self,
        InstrumentId instrument_id,
        datetime from_datetime,
        datetime to_datetime,
        int limit,
        UUID4 correlation_id,
    ) except *
    cpdef void request_bars(
        self,
        BarType bar_type,
        datetime from_datetime,
        datetime to_datetime,
        int limit,
        UUID4 correlation_id,
    ) except *

# -- DATA HANDLERS ---------------------------------------------------------------------------------

    cdef void _handle_quote_ticks(self, InstrumentId instrument_id, list ticks, UUID4 correlation_id) except *
    cdef void _handle_trade_ticks(self, InstrumentId instrument_id, list ticks, UUID4 correlation_id) except *
    cdef void _handle_bars(self, BarType bar_type, list bars, Bar partial, UUID4 correlation_id) except *
