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
from nautilus_trader.model.data.bar cimport Bar
from nautilus_trader.model.data.bar cimport BarType
from nautilus_trader.model.data.tick cimport QuoteTick
from nautilus_trader.model.data.tick cimport TradeTick
from nautilus_trader.model.instruments.base cimport Instrument


cdef class QuoteTickDataWrangler:
    cdef object _data_quotes
    cdef dict _data_bars_ask
    cdef dict _data_bars_bid

    cdef readonly Instrument instrument
    cdef readonly processed_data
    cdef readonly BarAggregation resolution

    cpdef QuoteTick _build_tick_from_values(self, str[:] values, double timestamp)


cdef class TradeTickDataWrangler:
    cdef object _data_trades

    cdef readonly Instrument instrument
    cdef readonly processed_data

    cpdef TradeTick _build_tick_from_values(self, str[:] values, double timestamp)


cdef class BarDataWrangler:
    cdef BarType _bar_type
    cdef int _price_precision
    cdef int _size_precision
    cdef object _data

    cpdef Bar _build_bar(self, double[:] values, double timestamp)
