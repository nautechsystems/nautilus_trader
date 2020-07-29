# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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

from cpython.datetime cimport date, datetime

from nautilus_trader.model.c_enums.currency cimport Currency
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.identifiers cimport Symbol, Venue
from nautilus_trader.model.objects cimport Tick, BarType, Bar, Instrument
from nautilus_trader.model.currency cimport ExchangeRateCalculator
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.trading.strategy cimport TradingStrategy


cdef class DataClient:
    cdef Clock _clock
    cdef UUIDFactory _uuid_factory
    cdef LoggerAdapter _log
    cdef dict _ticks
    cdef dict _tick_handlers
    cdef dict _spreads
    cdef dict _spreads_average
    cdef dict _bar_aggregators
    cdef dict _bar_handlers
    cdef dict _instrument_handlers
    cdef dict _instruments
    cdef ExchangeRateCalculator _exchange_calculator

    cdef readonly int tick_capacity

# -- ABSTRACT METHODS ------------------------------------------------------------------------------
    cpdef void connect(self) except *
    cpdef void disconnect(self) except *
    cpdef void reset(self) except *
    cpdef void dispose(self) except *
    cpdef void request_ticks(
        self,
        Symbol symbol,
        date from_date,
        date to_date,
        int limit,
        callback) except *
    cpdef void request_bars(
        self,
        BarType bar_type,
        date from_date,
        date to_date,
        int limit,
        callback) except *
    cpdef void request_instrument(self, Symbol symbol, callback) except *
    cpdef void request_instruments(self, Venue venue, callback) except *
    cpdef void subscribe_ticks(self, Symbol symbol, handler) except *
    cpdef void subscribe_bars(self, BarType bar_type, handler) except *
    cpdef void subscribe_instrument(self, Symbol symbol, handler) except *
    cpdef void unsubscribe_ticks(self, Symbol symbol, handler) except *
    cpdef void unsubscribe_bars(self, BarType bar_type, handler) except *
    cpdef void unsubscribe_instrument(self, Symbol symbol, handler) except *
    cpdef void update_instruments(self, Venue venue) except *
# ------------------------------------------------------------------------------------------------ #

    cpdef datetime time_now(self)
    cpdef list subscribed_ticks(self)
    cpdef list subscribed_bars(self)
    cpdef list subscribed_instruments(self)
    cpdef list instrument_symbols(self)
    cpdef void register_strategy(self, TradingStrategy strategy) except *
    cpdef dict get_instruments(self)
    cpdef Instrument get_instrument(self, Symbol symbol)
    cpdef bint has_ticks(self, Symbol symbol)
    cpdef double spread(self, Symbol symbol)
    cpdef double spread_average(self, Symbol symbol)
    cpdef double get_exchange_rate(
        self,
        Currency from_currency,
        Currency to_currency,
        PriceType price_type=*)

    cdef void _generate_bars(self, BarType bar_type, handler) except *
    cdef void _add_tick_handler(self, Symbol symbol, handler) except *
    cdef void _add_bar_handler(self, BarType bar_type, handler) except *
    cdef void _add_instrument_handler(self, Symbol symbol, handler) except *
    cdef void _remove_tick_handler(self, Symbol symbol, handler) except *
    cdef void _remove_bar_handler(self, BarType bar_type, handler) except *
    cdef void _remove_instrument_handler(self, Symbol symbol, handler) except *
    cpdef void _bulk_build_tick_bars(
        self,
        BarType bar_type,
        date from_date,
        date to_date,
        int limit,
        callback) except *
    cpdef void _handle_tick(self, Tick tick) except *
    cpdef void _handle_bar(self, BarType bar_type, Bar bar) except *
    cpdef void _handle_instrument(self, Instrument instrument) except *
    cpdef void _handle_instruments(self, list instruments) except *
    cpdef void _reset(self) except *
