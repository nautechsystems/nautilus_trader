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

from nautilus_trader.model.bar cimport Bar
from nautilus_trader.model.bar cimport BarType
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.order cimport Order
from nautilus_trader.model.tick cimport Tick


cdef class TrailingStopSignal:
    cdef bint is_signal
    cdef Price price


cdef class TrailingStopAlgorithm:
    cdef Order order

    cdef object _calculate
    cdef object generate

    cdef TrailingStopSignal _generate_buy(self, Price update_price)
    cdef TrailingStopSignal _generate_sell(self, Price update_price)


cdef class TickTrailingStopAlgorithm(TrailingStopAlgorithm):
    cdef readonly Symbol symbol

    cpdef void update(self, Tick tick) except *
    cpdef TrailingStopSignal calculate_buy(self, Tick tick)
    cpdef TrailingStopSignal calculate_sell(self, Tick tick)


cdef class BarTrailingStopAlgorithm(TrailingStopAlgorithm):
    cdef readonly BarType bar_type

    cpdef void update(self, Bar bar) except *
    cpdef TrailingStopSignal calculate_buy(self, Bar bar)
    cpdef TrailingStopSignal calculate_sell(self, Bar bar)


cdef class BarsBackTrail(BarTrailingStopAlgorithm):
    cdef int _bars_back
    cdef float _sl_atr_multiple
    cdef list _bars
    cdef object _atr
