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

from cpython.datetime cimport datetime

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.model.bar cimport Bar
from nautilus_trader.model.bar cimport BarType
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.tick cimport QuoteTick
from nautilus_trader.model.tick cimport TradeTick
from nautilus_trader.data.engine cimport DataEngine


cdef class DataClient:
    cdef Clock _clock
    cdef UUIDFactory _uuid_factory
    cdef LoggerAdapter _log
    cdef DataEngine _engine

    cdef readonly Venue venue

# -- ABSTRACT METHODS ------------------------------------------------------------------------------

    cpdef void connect(self) except *
    cpdef void disconnect(self) except *
    cpdef void reset(self) except *
    cpdef void dispose(self) except *
    cpdef void request_quote_ticks(
        self,
        Symbol symbol,
        datetime from_datetime,
        datetime to_datetime,
        int limit,
        callback,
    ) except *
    cpdef void request_trade_ticks(
        self,
        Symbol symbol,
        datetime from_datetime,
        datetime to_datetime,
        int limit,
        callback,
    ) except *
    cpdef void request_bars(
        self,
        BarType bar_type,
        datetime from_datetime,
        datetime to_datetime,
        int limit,
        callback,
    ) except *
    cpdef void request_instrument(self, Symbol symbol, callback) except *
    cpdef void request_instruments(self, callback) except *
    cpdef void subscribe_quote_ticks(self, Symbol symbol) except *
    cpdef void subscribe_trade_ticks(self, Symbol symbol) except *
    cpdef void subscribe_bars(self, BarType bar_type) except *
    cpdef void subscribe_instrument(self, Symbol symbol) except *
    cpdef void unsubscribe_quote_ticks(self, Symbol symbol) except *
    cpdef void unsubscribe_trade_ticks(self, Symbol symbol) except *
    cpdef void unsubscribe_bars(self, BarType bar_type) except *
    cpdef void unsubscribe_instrument(self, Symbol symbol) except *

# -- HANDLER METHODS -------------------------------------------------------------------------------

    cpdef void handle_quote_tick(self, QuoteTick tick) except *
    cpdef void handle_quote_ticks(self, list ticks) except *
    cpdef void handle_trade_tick(self, TradeTick tick) except *
    cpdef void handle_trade_ticks(self, list ticks) except *
    cpdef void handle_bar(self, BarType bar_type, Bar bar) except *
    cpdef void handle_bars(self, BarType bar_type, list bars) except *
    cpdef void handle_instrument(self, Instrument instrument) except *
    cpdef void handle_instruments(self, list instruments) except *
