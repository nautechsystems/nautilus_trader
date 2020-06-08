# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from nautilus_trader.indicators.base.indicator cimport Indicator


cdef class HilbertSignalNoiseRatio(Indicator):
    cdef double _i_mult
    cdef double _q_mult
    cdef double _range_floor
    cdef double _amplitude_floor
    cdef object _inputs
    cdef object _detrended_prices
    cdef object _in_phase
    cdef object _quadrature
    cdef double _previous_range
    cdef double _previous_amplitude
    cdef double _previous_value
    cdef double _range
    cdef double _amplitude

    cdef readonly int period
    cdef readonly double value

    cpdef void update(self, double high, double low)
    cdef void _calc_hilbert_transform(self)
    cdef double _calc_amplitude(self)
    cdef double _calc_signal_noise_ratio(self)
    cpdef void reset(self)
