#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="engine.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

from cpython.datetime cimport datetime

from inv_trader.common.account cimport Account
from inv_trader.common.clock cimport Clock
from inv_trader.common.logger cimport Logger, LoggerAdapter
from inv_trader.backtest.config cimport BacktestConfig
from inv_trader.backtest.data cimport BacktestDataClient
from inv_trader.backtest.execution cimport BacktestExecClient
from inv_trader.backtest.models cimport FillModel
from inv_trader.portfolio.portfolio cimport Portfolio
from inv_trader.trader cimport Trader


cdef class BacktestEngine:
    """
    Provides a backtest engine to run a trader on historical data.
    """
    cdef readonly Clock clock
    cdef readonly Clock test_clock
    cdef readonly BacktestConfig config
    cdef readonly LoggerAdapter log
    cdef readonly Logger logger
    cdef readonly Logger test_logger
    cdef readonly datetime created_time
    cdef readonly float time_to_initialize
    cdef readonly Account account
    cdef readonly Portfolio portfolio
    cdef readonly list instruments
    cdef readonly BacktestDataClient data_client
    cdef readonly BacktestExecClient exec_client
    cdef readonly Trader trader
    cdef readonly list data_minute_index
    cdef readonly int iteration

    cpdef void change_strategies(self, list strategies)
    cpdef void run(self, datetime start, datetime stop, int time_step_mins=*, FillModel fill_model=*, bint print_log_store=*)
    cpdef void create_returns_tear_sheet(self)
    cpdef void create_full_tear_sheet(self)
    cpdef dict get_performance_stats(self)
    cpdef list get_log_store(self)
    cpdef void print_log_store(self)
    cpdef void reset(self)
    cpdef void dispose(self)

    cdef void _engine_header(self)
    cdef void _backtest_header(self, datetime run_started, datetime start, datetime stop, int time_step_mins)
    cdef void _backtest_footer(self, datetime run_started, datetime start, datetime stop)
    cdef void _change_strategy_clocks_and_loggers(self, list strategies)
