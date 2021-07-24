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

from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logging cimport LiveLogger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.live.data_engine cimport LiveDataEngine
from nautilus_trader.live.execution_engine cimport LiveExecutionEngine
from nautilus_trader.msgbus.message_bus cimport MessageBus


cdef class TradingNodeBuilder:
    cdef LiveClock _clock
    cdef LiveLogger _logger
    cdef LoggerAdapter _log
    cdef MessageBus _msgbus
    cdef Cache _cache
    cdef LiveDataEngine _data_engine
    cdef LiveExecutionEngine _exec_engine
    cdef object _loop
    cdef dict _data_factories
    cdef dict _exec_factories

    cpdef void add_data_client_factory(self, str name, factory) except *
    cpdef void add_exec_client_factory(self, str name, factory) except *
    cpdef void build_data_clients(self, dict config) except *
    cpdef void build_exec_clients(self, dict config) except *
