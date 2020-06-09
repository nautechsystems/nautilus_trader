# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.indicators.average.moving_average cimport MovingAverage
from nautilus_trader.indicators.atr cimport AverageTrueRange


cdef class KeltnerChannel(Indicator):
    cdef MovingAverage _moving_average
    cdef AverageTrueRange _atr

    cdef readonly int period
    cdef readonly double k_multiplier
    cdef readonly double value_upper_band
    cdef readonly double value_middle_band
    cdef readonly double value_lower_band

    cpdef void update(self, double high, double low, double close)
    cpdef void reset(self)
