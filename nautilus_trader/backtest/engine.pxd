# -------------------------------------------------------------------------------------------------
# <copyright file="engine.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime, timedelta

from nautilus_trader.common.account cimport Account
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logger cimport Logger, LoggerAdapter
from nautilus_trader.trade.portfolio cimport Portfolio
from nautilus_trader.trade.trader cimport Trader
from nautilus_trader.backtest.config cimport BacktestConfig
from nautilus_trader.backtest.data cimport BacktestDataClient
from nautilus_trader.backtest.execution cimport BacktestExecClient
from nautilus_trader.backtest.models cimport FillModel


cdef class BacktestEngine:
    """
    Provides a backtest engine to run a trader on historical data.
    """
    cdef readonly Clock clock
    cdef readonly Clock test_clock
    cdef readonly BacktestConfig config
    cdef readonly BacktestDataClient data_client
    cdef readonly BacktestExecClient exec_client
    cdef readonly LoggerAdapter log
    cdef readonly Logger logger
    cdef readonly Logger test_logger
    cdef readonly Account account
    cdef readonly Portfolio portfolio
    cdef readonly Trader trader
    cdef readonly datetime created_time
    cdef readonly timedelta time_to_initialize
    cdef readonly list instruments
    cdef readonly int iteration

    cpdef run(self, datetime start=*, datetime stop=*, timedelta time_step=*, FillModel fill_model=*, list strategies=*, bint print_log_store=*)
    cdef void _run_with_tick_execution(self, datetime time, datetime stop, timedelta time_step)
    cdef void _run_with_bar_execution(self, datetime time, datetime stop, timedelta time_step)

    cpdef void create_returns_tear_sheet(self)
    cpdef void create_full_tear_sheet(self)
    cpdef dict get_performance_stats(self)
    cpdef object get_orders_report(self)
    cpdef object get_order_fills_report(self)
    cpdef object get_positions_report(self)
    cpdef list get_log_store(self)
    cpdef void print_log_store(self)
    cpdef void reset(self)
    cpdef void dispose(self)

    cdef void _engine_header(self)
    cdef void _backtest_header(self, datetime run_started, datetime start, datetime stop, timedelta time_step)
    cdef void _backtest_footer(self, datetime run_started, datetime start, datetime stop, timedelta time_step)
    cdef void _change_clocks_and_loggers(self, list strategies)
