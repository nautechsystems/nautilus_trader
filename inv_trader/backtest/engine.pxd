#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="engine.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False

from cpython.datetime cimport datetime, timedelta
from inv_trader.common.clock cimport Clock
from inv_trader.common.logger cimport Logger
from inv_trader.backtest.data cimport BacktestDataClient
from inv_trader.backtest.execution cimport BacktestExecClient
from inv_trader.trader cimport Trader


cdef class BacktestConfig:
    """
    Represents a configuration for a BacktestEngine.
    """
    cdef readonly object level_console
    cdef readonly object level_file
    cdef readonly bint console_prints
    cdef readonly bint log_to_file
    cdef readonly str log_file_path


cdef class BacktestEngine:
    """
    Provides a backtest engine to run a trader on historical data.
    """
    cdef readonly Clock clock
    cdef readonly Clock test_clock
    cdef readonly Logger log
    cdef readonly Logger test_log
    cdef readonly BacktestDataClient data_client
    cdef readonly BacktestExecClient exec_client
    cdef readonly Trader trader
    cdef readonly datetime first_timestamp
    cdef readonly datetime last_timestamp

    cpdef void run(self, datetime start, datetime stop, timedelta time_step=*)
