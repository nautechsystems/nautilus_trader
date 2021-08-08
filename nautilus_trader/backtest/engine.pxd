# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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
from libc.stdint cimport int64_t

from nautilus_trader.analysis.performance cimport PerformanceAnalyzer
from nautilus_trader.backtest.data_producer cimport DataProducerFacade
from nautilus_trader.cache.base cimport CacheFacade
from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.core.uuid cimport UUID
from nautilus_trader.data.engine cimport DataEngine
from nautilus_trader.execution.engine cimport ExecutionEngine
from nautilus_trader.msgbus.message_bus cimport MessageBus
from nautilus_trader.risk.engine cimport RiskEngine
from nautilus_trader.trading.portfolio cimport Portfolio
from nautilus_trader.trading.portfolio cimport PortfolioFacade
from nautilus_trader.trading.trader cimport Trader


cdef class BacktestEngine:
    cdef Clock _clock
    cdef Clock _test_clock
    cdef UUIDFactory _uuid_factory
    cdef MessageBus _msgbus
    cdef Cache _cache
    cdef Portfolio _portfolio
    cdef DataEngine _data_engine
    cdef ExecutionEngine _exec_engine
    cdef RiskEngine _risk_engine
    cdef DataProducerFacade _data_producer
    cdef LoggerAdapter _log
    cdef Logger _logger
    cdef Logger _test_logger
    cdef bint _log_to_file
    cdef bint _cache_db_flush
    cdef bint _use_data_cache
    cdef bint _run_analysis

    cdef dict _exchanges
    cdef list _generic_data
    cdef list _data
    cdef list _order_book_data
    cdef dict _quote_ticks
    cdef dict _trade_ticks
    cdef dict _bars_bid
    cdef dict _bars_ask

    cdef readonly Trader trader
    """The trader for the backtest.\n\n:returns: `Trader`"""
    cdef readonly UUID system_id
    """The backtest engine system ID.\n\n:returns: `UUID`"""
    cdef readonly datetime created_time
    """The backtest engine created time.\n\n:returns: `datetime`"""
    cdef readonly timedelta time_to_initialize
    """The backtest engine time to initialize.\n\n:returns: `timedelta`"""
    cdef readonly int iteration
    """The backtest engine iteration count.\n\n:returns: `int`"""
    cdef readonly CacheFacade cache
    """The backtest engine cache.\n\n:returns: `CacheFacade`"""
    cdef readonly PortfolioFacade portfolio
    """The backtest engine portfolio.\n\n:returns: `PortfolioFacade`"""
    cdef readonly PerformanceAnalyzer analyzer
    """The performance analyzer for the backtest.\n\n:returns: `PerformanceAnalyzer`"""

    cdef void _advance_time(self, int64_t now_ns) except *
    cdef void _process_modules(self, int64_t now_ns) except *
    cdef void _pre_run(
        self,
        datetime run_started,
        datetime start,
        datetime stop,
    ) except *
    cdef void _post_run(
        self,
        datetime run_started,
        datetime run_finished,
        datetime start,
        datetime stop,
    ) except *
    cpdef list_venues(self)
