# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from nautilus_trader.indicators.base.indicator cimport Indicator


cdef class HilbertPeriod(Indicator):
    cdef double _i_mult
    cdef double _q_mult
    cdef double _amplitude_floor
    cdef object _inputs
    cdef object _detrended_prices
    cdef object _in_phase
    cdef object _quadrature
    cdef object _phase
    cdef object _delta_phase

    cdef readonly int period
    cdef readonly double value

    cpdef void update(self, double high, double low)
    cpdef void _calc_hilbert_transform(self)
    cpdef void reset(self)
