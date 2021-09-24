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

from nautilus_trader.cache.base cimport CacheFacade
from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.core.data cimport Data
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.data.engine cimport DataEngine
from nautilus_trader.execution.engine cimport ExecutionEngine
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.msgbus.bus cimport MessageBus
from nautilus_trader.portfolio.base cimport PortfolioFacade
from nautilus_trader.portfolio.portfolio cimport Portfolio
from nautilus_trader.risk.engine cimport RiskEngine
from nautilus_trader.trading.trader cimport Trader


cdef class BacktestEngine:
    cdef object _config
    cdef Clock _clock
    cdef Clock _test_clock
    cdef UUIDFactory _uuid_factory
    cdef MessageBus _msgbus
    cdef Cache _cache
    cdef Portfolio _portfolio
    cdef DataEngine _data_engine
    cdef ExecutionEngine _exec_engine
    cdef RiskEngine _risk_engine
    cdef LoggerAdapter _log
    cdef Logger _logger
    cdef Logger _test_logger

    cdef dict _exchanges
    cdef list _data
    cdef int64_t _data_len
    cdef int64_t _index
    cdef datetime _run_started
    cdef datetime _backtest_start

    cdef readonly Trader trader
    """The trader for the backtest.\n\n:returns: `Trader`"""
    cdef readonly TraderId trader_id
    """The trader ID associated with the engine.\n\n:returns: `TraderId`"""
    cdef readonly str machine_id
    """The backtest engine machine ID.\n\n:returns: `str`"""
    cdef readonly UUID4 instance_id
    """The backtest engine instance ID.\n\n:returns: `UUID4`"""
    cdef readonly int iteration
    """The backtest engine iteration count.\n\n:returns: `int`"""
    cdef readonly CacheFacade cache
    """The backtest engine cache.\n\n:returns: `CacheFacade`"""
    cdef readonly PortfolioFacade portfolio
    """The backtest engine portfolio.\n\n:returns: `PortfolioFacade`"""
    cdef readonly analyzer
    """The performance analyzer for the backtest.\n\n:returns: `PerformanceAnalyzer`"""

    cdef Data _next(self)
    cdef void _advance_time(self, int64_t now_ns) except *
