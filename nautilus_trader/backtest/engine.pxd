# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime
from cpython.datetime cimport timedelta

from nautilus_trader.analysis.performance cimport PerformanceAnalyzer
from nautilus_trader.backtest.config cimport BacktestConfig
from nautilus_trader.backtest.data cimport BacktestDataEngine
from nautilus_trader.backtest.execution cimport BacktestExecClient
from nautilus_trader.backtest.market cimport SimulatedMarket
from nautilus_trader.backtest.models cimport FillModel
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.portfolio cimport Portfolio
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.execution.engine cimport ExecutionEngine
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.trading.trader cimport Trader


cdef class BacktestEngine:
    cdef readonly Clock clock
    cdef readonly Clock test_clock
    cdef readonly UUIDFactory uuid_factory
    cdef readonly BacktestConfig config
    cdef readonly BacktestDataEngine data_engine
    cdef readonly BacktestExecClient exec_client
    cdef readonly SimulatedMarket market
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

    cpdef void run(
        self,
        datetime start=*,
        datetime stop=*,
        FillModel fill_model=*,
        list strategies=*,
        bint print_log_store=*,
    ) except *
    cpdef void advance_time(self, datetime timestamp) except *
    cpdef list get_log_store(self)
    cpdef void print_log_store(self) except *
    cpdef void reset(self) except *
    cpdef void dispose(self) except *
    cdef void _backtest_memory(self) except *
    cdef void _backtest_header(
        self,
        datetime run_started,
        datetime start,
        datetime stop,
    ) except *
    cdef void _backtest_footer(
        self,
        datetime run_started,
        datetime run_finished,
        datetime start,
        datetime stop,
    ) except *
