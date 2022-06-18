# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

from libc.stdint cimport uint64_t

from nautilus_trader.cache.base cimport CacheFacade
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.data.engine cimport DataEngine
from nautilus_trader.execution.engine cimport ExecutionEngine
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.msgbus.bus cimport MessageBus
from nautilus_trader.portfolio.base cimport PortfolioFacade
from nautilus_trader.risk.engine cimport RiskEngine
from nautilus_trader.trading.trader cimport Trader


cdef class NautilusKernel:
    cdef readonly object environment
    """The kernels environment context { ``BACKTEST``, ``SANDBOX``, ``LIVE`` }.\n\n:returns: `Environment`"""
    cdef readonly object loop
    """The kernels event loop.\n\n:returns: `AbstractEventLoop` or ``None``"""
    cdef readonly object loop_sig_callback
    """The kernels signal handling callback.\n\n:returns: `Callable` or ``None``"""
    cdef readonly object executor
    """The kernels default executor.\n\n:returns: `ThreadPoolExecutor` or ``None``"""
    cdef readonly str name
    """The kernels name.\n\n:returns: `str`"""
    cdef readonly TraderId trader_id
    """The kernels trader ID.\n\n:returns: `TraderId`"""
    cdef readonly str machine_id
    """The kernels machine ID.\n\n:returns: `str`"""
    cdef readonly UUID4 instance_id
    """The kernels instance ID.\n\n:returns: `UUID4`"""
    cdef readonly uint64_t ts_created
    """The UNIX timestamp (nanoseconds) when the kernel was created.\n\n:returns: `uint64_t`"""
    cdef readonly Clock clock
    """The kernels clock.\n\n:returns: `Clock`"""
    cdef readonly LoggerAdapter log
    """The kernels logger adapter.\n\n:returns: `LoggerAdapter`"""
    cdef readonly Logger logger
    """The kernels logger.\n\n:returns: `Logger`"""
    cdef readonly MessageBus msgbus
    """The kernels message bus.\n\n:returns: `MessageBus`"""
    cdef readonly CacheFacade cache
    """The kernels read-only cache instance.\n\n:returns: `CacheFacade`"""
    cdef readonly PortfolioFacade portfolio
    """The kernels read-only portfolio instance.\n\n:returns: `PortfolioFacade`"""
    cdef readonly DataEngine data_engine
    """The kernels data engine.\n\n:returns: `DataEngine`"""
    cdef readonly RiskEngine risk_engine
    """The kernels risk engine.\n\n:returns: `RiskEngine`"""
    cdef readonly ExecutionEngine exec_engine
    """The kernels execution engine.\n\n:returns: `ExecutionEngine`"""
    cdef readonly Trader trader
    """The kernels trader instance.\n\n:returns: `Trader`"""
    cdef readonly object writer
    """The kernels writer.\n\n:returns: `StreamingFeatherWriter` or ``None``"""
