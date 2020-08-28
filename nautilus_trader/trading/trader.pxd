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

from nautilus_trader.analysis.performance cimport PerformanceAnalyzer
from nautilus_trader.analysis.reports cimport ReportProvider
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.data cimport DataClient
from nautilus_trader.common.execution cimport ExecutionEngine
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.portfolio cimport Portfolio
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.core.fsm cimport FiniteStateMachine
from nautilus_trader.model.c_enums.component_state cimport ComponentState
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport TraderId


cdef class Trader:
    cdef Clock _clock
    cdef UUIDFactory _uuid_factory
    cdef LoggerAdapter _log
    cdef DataClient _data_client
    cdef ExecutionEngine _exec_engine
    cdef ReportProvider _report_provider
    cdef FiniteStateMachine _fsm

    cdef readonly TraderId id
    cdef readonly AccountId account_id
    cdef readonly Portfolio portfolio
    cdef readonly PerformanceAnalyzer analyzer
    cdef readonly list strategies

    cpdef void initialize_strategies(self, list strategies) except *
    cpdef void start(self) except *
    cpdef void stop(self) except *
    cpdef void check_residuals(self) except *
    cpdef void save(self) except *
    cpdef void load(self) except *
    cpdef void reset(self) except *
    cpdef void dispose(self) except *

    cpdef void account_inquiry(self) except *

    cpdef ComponentState state(self)
    cpdef dict strategy_states(self)
    cpdef object generate_orders_report(self)
    cpdef object generate_order_fills_report(self)
    cpdef object generate_positions_report(self)
    cpdef object generate_account_report(self)
