# -------------------------------------------------------------------------------------------------
# <copyright file="analyzers.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.model.objects cimport Tick


cdef class SpreadAnalyzer:
    cdef int _decimal_precision
    cdef int _average_spread_capacity
    cdef list _spreads
    cdef object _average_spreads

    cdef readonly bint initialized
    cdef readonly object current_spread
    cdef readonly object average_spread
    cdef readonly object maximum_spread
    cdef readonly object minimum_spread

    cpdef void update(self, Tick tick)
    cpdef void calculate_metrics(self)
    cpdef list get_average_spreads(self)
    cpdef void reset(self)

    cdef void _calculate_and_set_metrics(self)


cdef class LiquidityAnalyzer:
    cdef readonly float liquidity_threshold
    cdef readonly float value
    cdef readonly bint initialized
    cdef readonly bint is_liquid
    cdef readonly bint not_liquid

    cpdef void update(self, average_spread, float volatility)
    cpdef void reset(self)
    cdef void _set_is_liquid(self)
    cdef void _set_not_liquid(self)
