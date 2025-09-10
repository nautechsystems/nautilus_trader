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

import numpy as np
cimport numpy as np

from nautilus_trader.core.rust.model cimport PriceType
from nautilus_trader.indicators.base cimport Indicator
from nautilus_trader.indicators.momentum cimport ChandeMomentumOscillator
from nautilus_trader.indicators.momentum cimport EfficiencyRatio


cpdef enum MovingAverageType:
    SIMPLE = 0
    EXPONENTIAL = 1
    DOUBLE_EXPONENTIAL = 2
    WILDER = 3
    HULL = 4
    ADAPTIVE = 5
    WEIGHTED = 6
    VARIABLE_INDEX_DYNAMIC = 7


cdef class MovingAverage(Indicator):
    cdef readonly int period
    """The moving average period.\n\n:returns: `PriceType`"""
    cdef readonly PriceType price_type
    """The specified price type for extracting values from quotes.\n\n:returns: `PriceType`"""
    cdef readonly int count
    """The count of inputs received by the indicator.\n\n:returns: `int`"""
    cdef readonly double value
    """The current output value.\n\n:returns: `double`"""

    cpdef void update_raw(self, double value)
    cpdef void _increment_count(self)
    cpdef void _reset_ma(self)


cdef class SimpleMovingAverage(MovingAverage):
    cdef object _inputs


cdef class ExponentialMovingAverage(MovingAverage):
    cdef readonly double alpha
    """The moving average alpha value.\n\n:returns: `double`"""


cdef class DoubleExponentialMovingAverage(MovingAverage):
    cdef ExponentialMovingAverage _ma1
    cdef ExponentialMovingAverage _ma2


cdef class WeightedMovingAverage(MovingAverage):
    cdef object _inputs
    cdef readonly object weights


cdef class HullMovingAverage(MovingAverage):
    cdef int _period_sqrt
    cdef np.ndarray _w1
    cdef np.ndarray _w2
    cdef np.ndarray _w3
    cdef MovingAverage _ma1
    cdef MovingAverage _ma2
    cdef MovingAverage _ma3

    cdef np.ndarray _get_weights(self, int size)


cdef class AdaptiveMovingAverage(MovingAverage):
    cdef EfficiencyRatio _efficiency_ratio
    cdef double _prior_value

    cdef readonly int period_er
    """The period of the internal `EfficiencyRatio` indicator.\n\n:returns: `double`"""
    cdef readonly int period_alpha_fast
    """The period of the fast smoothing constant.\n\n:returns: `double`"""
    cdef readonly int period_alpha_slow
    """The period of the slow smoothing constant.\n\n:returns: `double`"""
    cdef readonly double alpha_fast
    """The alpha fast value.\n\n:returns: `double`"""
    cdef readonly double alpha_slow
    """The alpha slow value.\n\n:returns: `double`"""
    cdef readonly double alpha_diff
    """The alpha difference value.\n\n:returns: `double`"""


cdef class WilderMovingAverage(MovingAverage):
    cdef readonly double alpha
    """The moving average alpha value.\n\n:returns: `double`"""


cdef class VariableIndexDynamicAverage(MovingAverage):
    cdef ChandeMomentumOscillator cmo

    cdef readonly double alpha
    """The moving average alpha value.\n\n:returns: `double`"""
    cdef readonly double cmo_pct
    """The normal cmo value.\n\n:returns: `double`"""


cdef class MovingAverageFactory:
    pass
