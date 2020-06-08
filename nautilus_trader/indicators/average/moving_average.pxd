# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from nautilus_trader.indicators.base.indicator cimport Indicator


cdef class MovingAverage(Indicator):
    cdef readonly int period
    cdef readonly int count
    cdef readonly double value

    cdef void _update(self, double point)
    cdef void _reset_ma(self)
