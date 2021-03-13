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

from nautilus_trader.common.component cimport Component
from nautilus_trader.execution.engine cimport ExecutionEngine
from nautilus_trader.model.commands cimport SubmitBracketOrder
from nautilus_trader.model.commands cimport SubmitOrder
from nautilus_trader.model.order.base cimport Order
from nautilus_trader.trading.portfolio cimport Portfolio


cdef class RiskEngine(Component):
    cdef Portfolio _portfolio
    cdef ExecutionEngine _exec_engine

    cdef readonly bint block_all_orders

    cpdef void set_block_all_orders(self, bint value=*) except *
    cpdef void approve_order(self, SubmitOrder command) except *
    cpdef void approve_bracket(self, SubmitBracketOrder command) except *
    cdef list _check_submit_order_risk(self, SubmitOrder command)
    cdef list _check_submit_bracket_order_risk(self, SubmitBracketOrder command)
    cdef void _deny_order(self, Order order, str reason) except *
