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
from nautilus_trader.indicators.base.indicator cimport Indicator


cdef class Swings(Indicator):
    cdef object _high_inputs
    cdef object _low_inputs

    cdef readonly int period
    cdef readonly int value
    cdef readonly int direction
    cdef readonly bint changed
    cdef readonly datetime high_datetime
    cdef readonly datetime low_datetime
    cdef readonly list lengths
    cdef readonly list durations
    cdef readonly double high_price
    cdef readonly double low_price
    cdef readonly double length_last
    cdef readonly double length_current
    cdef readonly int duration_last
    cdef readonly int duration_current
    cdef readonly int since_high
    cdef readonly int since_low

    cpdef void update(self, double high, double low, datetime timestamp) except *
    cdef void _calculate_swing_logic(self, double high, double low, datetime timestamp) except *
    cdef void _swing_changed(self) except *
    cpdef void reset(self) except *
