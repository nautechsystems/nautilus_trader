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

from decimal import Decimal

from cpython.datetime cimport datetime

from nautilus_trader.adapters.ccxt.providers cimport CCXTInstrumentProvider
from nautilus_trader.core.uuid cimport UUID
from nautilus_trader.live.data cimport LiveDataClient
from nautilus_trader.model.bar cimport Bar
from nautilus_trader.model.bar cimport BarType
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.tick cimport QuoteTick
from nautilus_trader.model.tick cimport TradeTick


cdef class CCXTDataClient(LiveDataClient):
    cdef object _client
    cdef CCXTInstrumentProvider _instrument_provider
    cdef bint _is_connected

    cdef set _subscribed_instruments

    cpdef void _request_instrument(self, Symbol symbol, UUID correlation_id) except *
    cpdef void _request_instruments(self, UUID correlation_id) except *
    cpdef void _subscribed_instruments_update(self) except *
    cpdef void _subscribed_instruments_load_and_send(self) except *
    cpdef void _request_trade_ticks(
        self,
        Symbol symbol,
        datetime from_datetime,
        datetime to_datetime,
        int limit,
        UUID correlation_id,
    ) except *
    cpdef void _request_bars(
        self,
        BarType bar_type,
        datetime from_datetime,
        datetime to_datetime,
        int limit,
        UUID correlation_id,
    ) except *
    cpdef void _request_time_bars(
        self,
        Instrument instrument,
        BarType bar_type,
        datetime from_datetime,
        datetime to_datetime,
        int limit,
        UUID correlation_id,
    ) except *
    cpdef void _on_trade_tick(
        self,
        str feed,
        str pair,
        int order_id,
        double timestamp,
        str side,
        amount: Decimal,
        price: Decimal,
        double receipt_timestamp,
    ) except *
    cdef inline TradeTick _parse_trade_tick(self, Instrument instrument, dict trade)
    cdef inline Bar _parse_bar(self, Instrument instrument, list values)

# -- PYTHON WRAPPERS -------------------------------------------------------------------------------

    cpdef void _handle_instrument_py(self, Instrument instrument) except *
    cpdef void _handle_quote_tick_py(self, QuoteTick tick) except *
    cpdef void _handle_trade_tick_py(self, TradeTick tick) except *
    cpdef void _handle_bar_py(self, BarType bar_type, Bar bar) except *
    cpdef void _handle_instruments_py(self, list instruments, UUID correlation_id) except *
    cpdef void _handle_quote_ticks_py(self, Symbol symbol, list ticks, UUID correlation_id) except *
    cpdef void _handle_trade_ticks_py(self, Symbol symbol, list ticks, UUID correlation_id) except *
    cpdef void _handle_bars_py(self, BarType bar_type, list bars, Bar partial, UUID correlation_id) except *
