#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="trader.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False

from inv_trader.common.clock cimport Clock
from inv_trader.common.logger cimport LoggerAdapter
from inv_trader.common.data cimport DataClient
from inv_trader.common.execution cimport ExecutionClient
from inv_trader.model.identifiers cimport Label, GUID
from inv_trader.common.account cimport Account
from inv_trader.portfolio.portfolio cimport Portfolio


cdef class Trader:
    """
    Represents a trader for managing a portfolio of trade strategies.
    """
    cdef Clock _clock
    cdef LoggerAdapter _log
    cdef DataClient _data_client
    cdef ExecutionClient _exec_client

    cdef readonly Label name
    cdef readonly GUID id
    cdef readonly Account account
    cdef readonly Portfolio portfolio
    cdef readonly list strategies
    cdef readonly list started_datetimes
    cdef readonly list stopped_datetimes
    cdef readonly bint is_running

    cpdef int strategy_count(self)
    cpdef void start(self)
    cpdef void stop(self)
    cpdef void create_returns_tear_sheet(self)
    cpdef void create_full_tear_sheet(self)
    cpdef void change_strategies(self, list strategies)
    cpdef void reset(self)
    cpdef void dispose(self)

    cdef void _load_strategies(self)
