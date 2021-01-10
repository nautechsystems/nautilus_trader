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
import threading

from nautilus_trader.adapters.oanda.providers cimport OandaInstrumentProvider
from nautilus_trader.core.uuid cimport UUID
from nautilus_trader.live.data cimport LiveDataClient
from nautilus_trader.model.bar cimport Bar
from nautilus_trader.model.bar cimport BarType
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.tick cimport QuoteTick
from nautilus_trader.model.tick cimport TradeTick


cdef class OandaDataClient(LiveDataClient):
    cdef object _client
    cdef str _account_id
    cdef bint _is_connected
    cdef set _subscribed_instruments
    cdef dict _subscribed_quote_ticks
    cdef OandaInstrumentProvider _instrument_provider
    cdef object _update_instruments_handle

    cpdef void _load_instruments(self) except *
    cpdef void _request_instrument(self, Symbol symbol, UUID correlation_id) except *
    cpdef void _request_instruments(self, UUID correlation_id) except *
    cpdef void _subscribed_instruments_update(self) except *
    cpdef void _subscribed_instruments_load_and_send(self) except *
    cpdef void _request_bars(
        self,
        BarType bar_type,
        datetime from_datetime,
        datetime to_datetime,
        int limit,
        UUID correlation_id,
    ) except *
    cpdef void _stream_prices(self, Symbol symbol, event: threading.Event) except *
    cdef inline QuoteTick _parse_quote_tick(self, Symbol symbol, dict values)
    cdef inline Bar _parse_bar(self, Instrument instrument, dict values, PriceType price_type)

# -- PYTHON WRAPPERS -------------------------------------------------------------------------------

    cpdef void _handle_instrument_py(self, Instrument instrument) except *
    cpdef void _handle_quote_tick_py(self, QuoteTick tick) except *
    cpdef void _handle_trade_tick_py(self, TradeTick tick) except *
    cpdef void _handle_bar_py(self, BarType bar_type, Bar bar) except *
    cpdef void _handle_instruments_py(self, list instruments, UUID correlation_id) except *
    cpdef void _handle_quote_ticks_py(self, Symbol symbol, list ticks, UUID correlation_id) except *
    cpdef void _handle_trade_ticks_py(self, Symbol symbol, list ticks, UUID correlation_id) except *
    cpdef void _handle_bars_py(self, BarType bar_type, list bars, Bar partial, UUID correlation_id) except *
