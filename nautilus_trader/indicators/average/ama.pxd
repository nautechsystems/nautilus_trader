# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from nautilus_trader.indicators.average.moving_average cimport MovingAverage
from nautilus_trader.indicators.efficiency_ratio cimport EfficiencyRatio


cdef class AdaptiveMovingAverage(MovingAverage):
    cdef int _period_er
    cdef int _period_alpha_fast
    cdef int _period_alpha_slow
    cdef double _alpha_fast
    cdef double _alpha_slow
    cdef double _alpha_diff
    cdef EfficiencyRatio _efficiency_ratio
    cdef double _prior_value

    cpdef void update(self, double point)
    cpdef void reset(self)
