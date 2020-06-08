# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.indicators.atr cimport AverageTrueRange


cdef class VolatilityCompressionRatio(Indicator):
    cdef int _fast_period
    cdef int _slow_period
    cdef AverageTrueRange _atr_fast
    cdef AverageTrueRange _atr_slow

    cdef readonly double value

    cpdef void update(self, double high, double low, double close)
    cpdef void update_mid(self, double close)
    cdef void _check_initialized(self)
    cpdef void reset(self)
