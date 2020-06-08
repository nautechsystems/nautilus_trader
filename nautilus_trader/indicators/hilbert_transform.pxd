# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from nautilus_trader.indicators.base.indicator cimport Indicator


cdef class HilbertTransform(Indicator):
    cdef double _i_mult
    cdef double _q_mult
    cdef object _inputs
    cdef object _detrended_prices
    cdef object _in_phase
    cdef object _quadrature

    cdef readonly int period
    cdef readonly double value_in_phase
    cdef readonly double value_quad

    cpdef void update(self, double price)
    cpdef void reset(self)
