# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
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

    cpdef void update(self, double high, double low, datetime timestamp)
    cdef void _calculate_swing_logic(self, double high, double low, datetime timestamp)
    cdef void _swing_changed(self)
    cpdef void reset(self)
