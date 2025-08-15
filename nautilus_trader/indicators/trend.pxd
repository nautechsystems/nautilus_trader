# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.indicators.averages cimport MovingAverage
from nautilus_trader.indicators.base cimport Indicator
from nautilus_trader.indicators.volatility cimport AverageTrueRange
from nautilus_trader.model.data cimport Bar


cdef class ArcherMovingAveragesTrends(Indicator):
    cdef MovingAverage _fast_ma
    cdef MovingAverage _slow_ma
    cdef object _fast_ma_price
    cdef object _slow_ma_price

    cdef readonly int fast_period
    cdef readonly int slow_period
    cdef readonly int signal_period
    cdef readonly long long_run
    cdef readonly long short_run

    cpdef void update_raw(self, double value)


cdef class AroonOscillator(Indicator):
    cdef object _high_inputs
    cdef object _low_inputs

    cdef readonly int period
    """The window period.\n\n:returns: `int`"""
    cdef readonly double value
    """The current value.\n\n:returns: `double`"""
    cdef readonly double aroon_up
    """The current aroon up value.\n\n:returns: `double`"""
    cdef readonly double aroon_down
    """The current aroon down value.\n\n:returns: `double`"""

    cpdef void update_raw(self, double high, double low)
    cdef void _check_initialized(self)


cdef class DirectionalMovement(Indicator):
    cdef AverageTrueRange _atr
    cdef MovingAverage _pos_ma
    cdef MovingAverage _neg_ma
    cdef double _previous_high
    cdef double _previous_low

    cdef readonly int period
    cdef readonly double value
    cdef readonly double pos
    cdef readonly double neg

    cpdef void update_raw(self, double high, double low)


cdef class MovingAverageConvergenceDivergence(Indicator):
    cdef MovingAverage _fast_ma
    cdef MovingAverage _slow_ma
    cdef object price_type

    cdef readonly int fast_period
    cdef readonly int slow_period
    cdef readonly double value

    cpdef void update_raw(self, double value)


cdef class LinearRegression(Indicator):
    cdef object _inputs

    cdef readonly int period
    cdef readonly double slope
    cdef readonly double intercept
    cdef readonly double degree
    cdef readonly double cfo
    cdef readonly double R2
    cdef readonly double value

    cpdef void update_raw(self, double value)


cdef class Bias(Indicator):
    cdef MovingAverage _ma

    cdef readonly int period
    cdef readonly double value

    cpdef void update_raw(self, double close)
    cdef void _check_initialized(self)


cdef class Swings(Indicator):
    cdef object _high_inputs
    cdef object _low_inputs

    cdef readonly int period
    cdef readonly int direction
    cdef readonly bint changed
    cdef readonly datetime high_datetime
    cdef readonly datetime low_datetime
    cdef readonly double high_price
    cdef readonly double low_price
    cdef readonly double length
    cdef readonly int duration
    cdef readonly int since_high
    cdef readonly int since_low

    cpdef void handle_bar(self, Bar bar)
    cpdef void update_raw(self, double high, double low, datetime timestamp)
