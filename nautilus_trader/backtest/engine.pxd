# -------------------------------------------------------------------------------------------------
# <copyright file="engine.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime, timedelta

from nautilus_trader.model.identifiers cimport TraderId, AccountId
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.guid cimport GuidFactory
from nautilus_trader.common.logger cimport Logger, LoggerAdapter
from nautilus_trader.common.execution cimport ExecutionDatabase, ExecutionEngine
from nautilus_trader.common.portfolio cimport Portfolio
from nautilus_trader.analysis.performance cimport PerformanceAnalyzer
from nautilus_trader.trading.trader cimport Trader
from nautilus_trader.backtest.config cimport BacktestConfig
from nautilus_trader.backtest.data cimport BacktestDataClient
from nautilus_trader.backtest.execution cimport BacktestExecClient
from nautilus_trader.backtest.models cimport FillModel


cdef class BacktestEngine:
    cdef readonly Clock clock
    cdef readonly Clock test_clock
    cdef readonly GuidFactory guid_factory
    cdef readonly BacktestConfig config
    cdef readonly BacktestDataClient data_client
    cdef readonly BacktestExecClient exec_client
    cdef readonly ExecutionDatabase exec_db
    cdef readonly ExecutionEngine exec_engine
    cdef readonly LoggerAdapter log
    cdef readonly Logger logger
    cdef readonly Logger test_logger
    cdef readonly TraderId trader_id
    cdef readonly AccountId account_id
    cdef readonly Portfolio portfolio
    cdef readonly PerformanceAnalyzer analyzer
    cdef readonly Trader trader
    cdef readonly datetime created_time
    cdef readonly timedelta time_to_initialize
    cdef readonly int iteration

    cpdef void run(self, datetime start=*, datetime stop=*, FillModel fill_model=*, list strategies=*, bint print_log_store=*) except *
    cpdef list get_log_store(self)
    cpdef void print_log_store(self) except *
    cpdef void reset(self) except *
    cpdef void dispose(self) except *

    cdef void _backtest_memory(self) except *
    cdef void _backtest_header(self, datetime run_started, datetime start, datetime stop) except *
    cdef void _backtest_footer(self, datetime run_started, datetime run_finished, datetime start, datetime stop) except *
    cdef void _change_clocks_and_loggers(self, list strategies) except *
