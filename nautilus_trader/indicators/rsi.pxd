# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.indicators.average.moving_average cimport MovingAverage


cdef class RelativeStrengthIndex(Indicator):
    cdef double _rsi_max
    cdef MovingAverage _average_gain
    cdef MovingAverage _average_loss
    cdef double _last_point

    cdef readonly int period
    cdef readonly double value

    cpdef void update(self, double point)
    cpdef void reset(self)
