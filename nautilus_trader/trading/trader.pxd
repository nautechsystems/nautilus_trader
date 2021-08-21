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

from typing import Any, Callable

from nautilus_trader.analysis.performance cimport PerformanceAnalyzer
from nautilus_trader.analysis.reports cimport ReportProvider
from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.actor cimport Actor
from nautilus_trader.common.component cimport Component
from nautilus_trader.data.engine cimport DataEngine
from nautilus_trader.execution.engine cimport ExecutionEngine
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.portfolio.portfolio cimport Portfolio
from nautilus_trader.risk.engine cimport RiskEngine
from nautilus_trader.trading.strategy cimport TradingStrategy


cdef class Trader(Component):
    cdef Cache _cache
    cdef Portfolio _portfolio
    cdef DataEngine _data_engine
    cdef RiskEngine _risk_engine
    cdef ExecutionEngine _exec_engine
    cdef ReportProvider _report_provider
    cdef list _strategies
    cdef list _components

    cdef readonly PerformanceAnalyzer analyzer
    """The traders performance analyzer.\n\n:returns: `PerformanceAnalyzer`"""

    cdef list strategies_c(self)
    cdef list components_c(self)

    cpdef list strategy_ids(self)
    cpdef list component_ids(self)
    cpdef dict strategy_states(self)
    cpdef dict component_states(self)
    cpdef list components(self)
    cpdef void add_strategy(self, TradingStrategy strategy) except *
    cpdef void add_strategies(self, list strategies) except *
    cpdef void add_component(self, Actor component) except *
    cpdef void add_components(self, list component) except *
    cpdef void clear_strategies(self) except *
    cpdef void clear_components(self) except *
    cpdef void subscribe(self, str topic, handler: Callable[[Any], None]) except *
    cpdef void unsubscribe(self, str topic, handler: Callable[[Any], None]) except *
    cpdef void start(self) except *
    cpdef void stop(self) except *
    cpdef void save(self) except *
    cpdef void load(self) except *
    cpdef void reset(self) except *
    cpdef void dispose(self) except *
    cpdef void check_residuals(self) except *
    cpdef object generate_orders_report(self)
    cpdef object generate_order_fills_report(self)
    cpdef object generate_positions_report(self)
    cpdef object generate_account_report(self, Venue venue)
