# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from nautilus_trader.indicators.average.moving_average cimport MovingAverage
from nautilus_trader.indicators.base.indicator cimport Indicator


cdef class MovingAverageConvergenceDivergence(Indicator):
    cdef int _fast_period
    cdef int _slow_period
    cdef MovingAverage _fast_ma
    cdef MovingAverage _slow_ma

    cdef readonly double value

    cpdef void update(self, double point)
    cpdef void reset(self)
