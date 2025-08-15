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

from nautilus_trader.indicators.averages cimport MovingAverage
from nautilus_trader.indicators.base cimport Indicator
from nautilus_trader.model.data cimport Bar


cdef class AverageTrueRange(Indicator):
    cdef MovingAverage _ma
    cdef bint _use_previous
    cdef double _value_floor
    cdef double _previous_close

    cdef readonly int period
    """The window period.\n\n:returns: `int`"""
    cdef readonly double value
    """The current value.\n\n:returns: `double`"""

    cpdef void update_raw(self, double high, double low, double close)
    cdef void _floor_value(self)
    cdef void _check_initialized(self)


cdef class BollingerBands(Indicator):
    cdef object _prices
    cdef MovingAverage _ma

    cdef readonly int period
    """The period for the moving average.\n\n:returns: `int`"""
    cdef readonly double k
    """The standard deviation multiple.\n\n:returns: `double`"""
    cdef readonly double upper
    """The current value of the upper band.\n\n:returns: `double`"""
    cdef readonly double middle
    """The current value of the middle band.\n\n:returns: `double`"""
    cdef readonly double lower
    """The current value of the lower band.\n\n:returns: `double`"""

    cpdef void update_raw(self, double high, double low, double close)


cdef class DonchianChannel(Indicator):
    cdef object _upper_prices
    cdef object _lower_prices

    cdef readonly int period
    cdef readonly double upper
    cdef readonly double middle
    cdef readonly double lower

    cpdef void update_raw(self, double high, double low)


cdef class KeltnerChannel(Indicator):
    cdef MovingAverage _ma
    cdef AverageTrueRange _atr

    cdef readonly int period
    cdef readonly double k_multiplier
    cdef readonly double upper
    cdef readonly double middle
    cdef readonly double lower

    cpdef void update_raw(self, double high, double low, double close)


cdef class VerticalHorizontalFilter(Indicator):
    cdef MovingAverage _ma
    cdef object _prices
    cdef double _previous_close

    cdef readonly int period
    cdef readonly double value

    cpdef void update_raw(self, double close)
    cdef void _check_initialized(self)


cdef class VolatilityRatio(Indicator):
    cdef AverageTrueRange _atr_fast
    cdef AverageTrueRange _atr_slow

    cdef readonly int fast_period
    cdef readonly int slow_period
    cdef readonly double value

    cpdef void update_raw(self, double high, double low, double close)
    cdef void _check_initialized(self)


cdef class KeltnerPosition(Indicator):
    cdef KeltnerChannel _kc

    cdef readonly int period
    cdef readonly double k_multiplier
    cdef readonly double value

    cpdef void update_raw(self, double high, double low, double close)
