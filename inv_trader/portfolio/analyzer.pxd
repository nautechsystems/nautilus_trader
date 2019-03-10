#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="analyzer.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

from cpython.datetime cimport datetime

from inv_trader.model.objects cimport Money
from inv_trader.model.events cimport OrderEvent


cdef class Analyzer:
    """
    Represents a trading portfolio analyzer for generating performance metrics
    and statistics.
    """
    cdef bint _log_returns
    cdef object _returns
    cdef object _positions
    cdef object _transactions

    cpdef void add_return(self, datetime time, float value)
    cpdef void add_transaction(self, OrderEvent event)
    cpdef void add_positions(self, datetime time, list positions, Money cash_balance)
    cpdef object get_returns(self)
    cpdef object get_transactions(self)
    cpdef object get_positions(self)
    cpdef void create_returns_tear_sheet(self)
    cpdef void create_full_tear_sheet(self)
