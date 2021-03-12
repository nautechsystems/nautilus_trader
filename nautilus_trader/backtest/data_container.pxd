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

from nautilus_trader.model.c_enums.bar_aggregation cimport BarAggregation
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.instrument cimport Instrument


cdef class BacktestDataContainer:
    cdef readonly set venues
    cdef readonly set instrument_ids
    cdef readonly dict instruments
    cdef readonly dict quote_ticks
    cdef readonly dict trade_ticks
    cdef readonly dict bars_bid
    cdef readonly dict bars_ask

    cpdef void add_instrument(self, Instrument instrument) except *
    cpdef void add_quote_ticks(self, InstrumentId instrument_id, data) except *
    cpdef void add_trade_ticks(self, InstrumentId instrument_id, data) except *
    cpdef void add_bars(self, InstrumentId instrument_id, BarAggregation aggregation, PriceType price_type, data) except *
    cpdef void check_integrity(self) except *
    cpdef bint has_quote_data(self, InstrumentId instrument_id) except *
    cpdef bint has_trade_data(self, InstrumentId instrument_id) except *
    cpdef long total_data_size(self)
