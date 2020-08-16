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

from nautilus_trader.model.c_enums.bar_structure cimport BarStructure
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.tick cimport QuoteTick
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.common.data cimport DataClient


cdef class BacktestDataContainer:
    cdef readonly set symbols
    cdef readonly dict instruments
    cdef readonly dict ticks
    cdef readonly dict bars_bid
    cdef readonly dict bars_ask

    cpdef void add_instrument(self, Instrument instrument) except *
    cpdef void add_quote_ticks(self, Symbol symbol, data) except *
    cpdef void add_bars(self, Symbol symbol, BarStructure structure, PriceType price_type, data) except *
    cpdef void check_integrity(self) except *
    cpdef long total_data_size(self)


cdef class BacktestDataClient(DataClient):
    cdef BacktestDataContainer _data
    cdef object _tick_data
    cdef unsigned short[:] _symbols
    cdef double[:, :] _price_volume
    cdef datetime[:] _timestamps
    cdef dict _symbol_index
    cdef dict _price_precisions
    cdef dict _size_precisions
    cdef int _index
    cdef int _index_last

    cdef readonly list execution_resolutions
    cdef readonly datetime min_timestamp
    cdef readonly datetime max_timestamp
    cdef readonly bint has_data

    cpdef void setup(self, datetime start, datetime stop) except *
    cdef QuoteTick generate_tick(self)

    cpdef void process_tick(self, QuoteTick tick) except *
    cpdef void reset(self) except *
