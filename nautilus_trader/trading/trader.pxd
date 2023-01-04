# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.actor cimport Actor
from nautilus_trader.common.component cimport Component
from nautilus_trader.data.engine cimport DataEngine
from nautilus_trader.execution.engine cimport ExecutionEngine
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.portfolio.portfolio cimport Portfolio
from nautilus_trader.risk.engine cimport RiskEngine
from nautilus_trader.trading.strategy cimport Strategy


cdef class Trader(Component):
    cdef object _loop
    cdef Cache _cache
    cdef Portfolio _portfolio
    cdef DataEngine _data_engine
    cdef RiskEngine _risk_engine
    cdef ExecutionEngine _exec_engine
    cdef list _actors
    cdef list _strategies

    cpdef list actors(self)
    cpdef list strategies(self)

    cpdef list actor_ids(self)
    cpdef list strategy_ids(self)
    cpdef dict actor_states(self)
    cpdef dict strategy_states(self)
    cpdef void add_actor(self, Actor actor) except *
    cpdef void add_actors(self, list actors) except *
    cpdef void add_strategy(self, Strategy strategy) except *
    cpdef void add_strategies(self, list strategies) except *
    cpdef void clear_actors(self) except *
    cpdef void clear_strategies(self) except *
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
