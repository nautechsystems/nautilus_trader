# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from nautilus_trader.indicators.base.indicator cimport Indicator


cdef class FuzzyCandlesticks(Indicator):
    cdef double _threshold1
    cdef double _threshold2
    cdef double _threshold3
    cdef double _threshold4
    cdef object _lengths
    cdef object _body_percents
    cdef object _upper_wick_percents
    cdef object _lower_wick_percents
    cdef double _last_open
    cdef double _last_high
    cdef double _last_low
    cdef double _last_close
    cdef object _value
    cdef long[:] _value_array
    cdef long[:] _value_price_comparison

    cdef readonly int period

    cpdef void update(self, double open_price, double high_price, double low_price, double close_price)
    cpdef int price_comparison(self, double price1, double price2)
    cdef object _fuzzify_size(self, double length, double mean_length, double sd_lengths)
    cdef object _fuzzify_body_size(self, double body_percent, double mean_body_percent, double sd_body_percents)
    cdef object _fuzzify_wick_size(self, double wick_percent, double mean_wick_percent, double sd_wick_percents)
    cpdef void reset(self)
