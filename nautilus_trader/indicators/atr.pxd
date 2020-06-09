# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.indicators.average.moving_average cimport MovingAverage


cdef class AverageTrueRange(Indicator):
    cdef MovingAverage _moving_average
    cdef bint _use_previous
    cdef double _value_floor
    cdef double _previous_close

    cdef readonly int period
    cdef readonly double value


    cpdef void update(self, double high, double low, double close)
    cpdef void update_mid(self, double close)
    cdef void _floor_value(self)
    cdef void _check_initialized(self)
    cpdef void reset(self)
