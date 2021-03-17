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

from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.core.constants cimport *  # str constants only
from nautilus_trader.data.base cimport DataCacheFacade
from nautilus_trader.model.bar cimport Bar
from nautilus_trader.model.bar cimport BarType
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.order_book_old cimport OrderBook
from nautilus_trader.model.tick cimport QuoteTick
from nautilus_trader.model.tick cimport TradeTick
from nautilus_trader.trading.calculators cimport ExchangeRateCalculator


cdef class DataCache(DataCacheFacade):
    cdef LoggerAdapter _log
    cdef dict _xrate_symbols
    cdef dict _instruments
    cdef dict _quote_ticks
    cdef dict _trade_ticks
    cdef dict _order_books
    cdef dict _bars
    cdef ExchangeRateCalculator _xrate_calculator

    cdef readonly int tick_capacity
    """The caches tick capacity.\n\n:returns: `int`"""
    cdef readonly int bar_capacity
    """The caches bar capacity.\n\n:returns: `int`"""

    cpdef void reset(self) except *

    cpdef void add_instrument(self, Instrument instrument) except *
    cpdef void add_order_book(self, OrderBook order_book) except *
    cpdef void add_quote_tick(self, QuoteTick tick) except *
    cpdef void add_trade_tick(self, TradeTick tick) except *
    cpdef void add_bar(self, BarType bar_type, Bar bar) except *
    cpdef void add_quote_ticks(self, list ticks) except *
    cpdef void add_trade_ticks(self, list ticks) except *
    cpdef void add_bars(self, BarType bar_type, list bars) except *

    cdef inline tuple _build_quote_table(self, Venue venue)
    cdef inline bint _is_crypto_spot_or_swap(self, Instrument instrument) except *
    cdef inline bint _is_fx_spot(self, Instrument instrument) except *
