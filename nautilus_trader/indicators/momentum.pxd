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

from nautilus_trader.core.rust.model cimport PriceType
from nautilus_trader.indicators.averages cimport MovingAverage
from nautilus_trader.indicators.base cimport Indicator
from nautilus_trader.model.data cimport Bar


cdef class RelativeStrengthIndex(Indicator):
    cdef MovingAverage _average_gain
    cdef MovingAverage _average_loss
    cdef double _last_value
    cdef double _rsi_max

    cdef readonly int period
    """The window period.\n\n:returns: `int`"""
    cdef readonly double value
    """The current value.\n\n:returns: `double`"""

    cpdef void update_raw(self, double value)


cdef class RateOfChange(Indicator):
    cdef int _use_log
    cdef object _prices

    cdef readonly int period
    """The window period.\n\n:returns: `int`"""
    cdef readonly double value
    """The current value.\n\n:returns: `double`"""

    cpdef void update_raw(self, double value)


cdef class ChandeMomentumOscillator(Indicator):
    cdef MovingAverage _average_gain
    cdef MovingAverage _average_loss
    cdef double _previous_close

    cdef readonly int period
    """The window period.\n\n:returns: `int`"""
    cdef readonly double value
    """The current value.\n\n:returns: `double`"""

    cpdef void update_raw(self, double close)


cdef class Stochastics(Indicator):
    cdef object _highs
    cdef object _lows
    cdef object _c_sub_l
    cdef object _h_sub_l

    cdef readonly int period_k
    """The k period.\n\n:returns: `int`"""
    cdef readonly int period_d
    """The d period.\n\n:returns: `int`"""
    cdef readonly double value_k
    """The k value.\n\n:returns: `double`"""
    cdef readonly double value_d
    """The d value.\n\n:returns: `double`"""

    cpdef void update_raw(self, double high, double low, double close)


cdef class CommodityChannelIndex(Indicator):
    cdef MovingAverage _ma
    cdef object _prices

    cdef readonly int period
    cdef readonly double scalar
    """The positive float to scale the bands.\n\n:returns: `double`"""
    cdef readonly double _mad
    """The current price mean absolute deviation.\n\n:returns: `double`"""
    cdef readonly double value
    """The current value.\n\n:returns: `double`"""

    cpdef void handle_bar(self, Bar bar)
    cpdef void update_raw(self, double high, double low, double close)


cdef class EfficiencyRatio(Indicator):
    cdef object _inputs
    cdef object _deltas

    cdef readonly int period
    """The window period.\n\n:returns: `int`"""
    cdef readonly double value
    """The current value.\n\n:returns: `double`"""

    cpdef void update_raw(self, double price)


cdef class RelativeVolatilityIndex(Indicator):
    cdef MovingAverage _ma
    cdef MovingAverage _pos_ma
    cdef MovingAverage _neg_ma
    cdef object _prices

    cdef readonly int period
    cdef readonly double scalar
    cdef readonly double _previous_close
    cdef readonly double _std
    cdef readonly double value

    cpdef void handle_bar(self, Bar bar)
    cpdef void update_raw(self, double close)


cdef class PsychologicalLine(Indicator):
    cdef MovingAverage _ma

    cdef readonly int period
    cdef readonly double _diff
    cdef readonly double _previous_close
    cdef readonly double value

    cpdef void update_raw(self, double close)
