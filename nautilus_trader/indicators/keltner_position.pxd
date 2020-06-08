# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from nautilus_trader.indicators.keltner_channel cimport KeltnerChannel
from nautilus_trader.indicators.base.indicator cimport Indicator


cdef class KeltnerPosition(Indicator):
    cdef int _period
    cdef KeltnerChannel _kc

    cdef readonly double value

    cpdef void update(self, double high, double low, double close)
    cpdef void reset(self)
