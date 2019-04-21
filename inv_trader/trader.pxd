#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="trader.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

from inv_trader.common.clock cimport Clock
from inv_trader.common.logger cimport LoggerAdapter
from inv_trader.common.data cimport DataClient
from inv_trader.common.execution cimport ExecutionClient
from inv_trader.model.identifiers cimport ValidString, TraderId
from inv_trader.common.account cimport Account
from inv_trader.portfolio.portfolio cimport Portfolio
from inv_trader.reports cimport ReportProvider


cdef class Trader:
    """
    Provides a trader for managing a portfolio of trade strategies.
    """
    cdef Clock _clock
    cdef LoggerAdapter _log
    cdef ReportProvider _report_provider

    cdef readonly TraderId id
    cdef readonly ValidString order_id_tag
    cdef readonly bint is_running
    cdef readonly list started_datetimes
    cdef readonly list stopped_datetimes

    cdef DataClient _data_client
    cdef ExecutionClient _exec_client
    cdef readonly Account account
    cdef readonly Portfolio portfolio
    cdef readonly list strategies

    cpdef void start(self)
    cpdef void stop(self)
    cpdef void create_returns_tear_sheet(self)
    cpdef void create_full_tear_sheet(self)
    cpdef object get_order_fills_report(self)
    cpdef object get_trades_report(self)
    cpdef void change_strategies(self, list strategies)
    cpdef void reset(self)
    cpdef void dispose(self)

    cpdef dict strategy_status(self)

    cdef void _initialize_strategies(self)
