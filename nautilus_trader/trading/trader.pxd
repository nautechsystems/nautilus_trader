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

from nautilus_trader.analysis.performance cimport PerformanceAnalyzer
from nautilus_trader.analysis.reports cimport ReportProvider
from nautilus_trader.common.component cimport Component
from nautilus_trader.data.engine cimport DataEngine
from nautilus_trader.execution.engine cimport ExecutionEngine
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.msgbus.message_bus cimport MessageBus
from nautilus_trader.risk.engine cimport RiskEngine
from nautilus_trader.trading.portfolio cimport Portfolio


cdef class Trader(Component):
    cdef MessageBus _msgbus
    cdef Portfolio _portfolio
    cdef DataEngine _data_engine
    cdef RiskEngine _risk_engine
    cdef ExecutionEngine _exec_engine
    cdef ReportProvider _report_provider
    cdef list _strategies

    cdef readonly TraderId id
    """The trader ID.\n\n:returns: `TraderId`"""
    cdef readonly PerformanceAnalyzer analyzer
    """The traders performance analyzer.\n\n:returns: `PerformanceAnalyzer`"""

    cdef list strategies_c(self)

    cpdef list strategy_ids(self)
    cpdef void initialize_strategies(self, list strategies, bint warn_no_strategies) except *
    cpdef void start(self) except *
    cpdef void stop(self) except *
    cpdef void check_residuals(self) except *
    cpdef void save(self) except *
    cpdef void load(self) except *
    cpdef void reset(self) except *
    cpdef void dispose(self) except *
    cpdef dict strategy_states(self)
    cpdef object generate_orders_report(self)
    cpdef object generate_order_fills_report(self)
    cpdef object generate_positions_report(self)
    cpdef object generate_account_report(self, Venue venue)
