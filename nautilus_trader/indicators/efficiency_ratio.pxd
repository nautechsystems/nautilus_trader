# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from nautilus_trader.indicators.base.indicator cimport Indicator


cdef class EfficiencyRatio(Indicator):
    cdef object _inputs
    cdef object _deltas

    cdef readonly int period
    cdef readonly double value

    cpdef void update(self, double price)
    cpdef void reset(self)
