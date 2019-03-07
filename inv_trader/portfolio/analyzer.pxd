#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="analyzer.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

from cpython.datetime cimport date, timedelta


cdef class Analyzer:
    """
    Represents a trading portfolio analyzer for generating performance metrics
    and statistics.
    """
    cdef bint _log_returns
    cdef timedelta _time_step
    cdef object _returns
    cdef list _positions_symbols
    cdef list _positions_columns
    cdef object _positions
    cdef object _transactions

    cpdef void add_return(self, date d, float value)
    cpdef void add_daily_positions(self, date d, list positions, float cash)
    cpdef object get_returns(self)
    cpdef object get_positions(self)
    cpdef object get_transactions(self)
    cpdef void get_returns_tearsheet(self)
    cpdef void get_full_tearsheet(self)
