#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="analyzers.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

from inv_trader.model.objects cimport Tick


cdef class SpreadAnalyzer:
    """
    Provides a means of analyzing the spread of a market and track various metrics.
    """
    cdef int _decimal_precision
    cdef int _average_spread_capacity
    cdef list _spreads
    cdef object _average_spreads

    cdef readonly bint initialized
    cdef readonly object average

    cpdef void update(self, Tick tick)
    cpdef void snapshot_average(self)
    cpdef list get_average_spreads(self)
    cpdef void reset(self)

    cdef void _calculate_average(self)


cdef class LiquidityAnalyzer:
    """
    Provides a means of analyzing the liquidity of a market and track various metrics.
    """
    cdef readonly float liquidity_threshold
    cdef readonly float value
    cdef readonly bint initialized
    cdef readonly bint is_liquid
    cdef readonly bint is_not_liquid

    cpdef void update(self, average_spread, float volatility)
    cpdef void reset(self)
