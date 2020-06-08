# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from nautilus_trader.indicators.average.moving_average cimport MovingAverage
from nautilus_trader.indicators.atr cimport AverageTrueRange
from nautilus_trader.indicators.base.indicator cimport Indicator


cdef class Pressure(Indicator):
    cdef AverageTrueRange _atr
    cdef MovingAverage _average_volume

    cdef readonly int period
    cdef readonly double value
    cdef readonly double value_cumulative

    cpdef void update(self, double high, double low, double close, double volume)
    cpdef void reset(self)
