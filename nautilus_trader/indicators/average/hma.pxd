# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from nautilus_trader.indicators.average.moving_average cimport MovingAverage


cdef class HullMovingAverage(MovingAverage):
    cdef int _period_halved
    cdef int _period_sqrt
    cdef list _w1
    cdef list _w2
    cdef list _w3
    cdef MovingAverage _ma1
    cdef MovingAverage _ma2
    cdef MovingAverage _ma3

    cdef list _get_weights(self, int size)
    cpdef void update(self, double point)
    cpdef void reset(self)
