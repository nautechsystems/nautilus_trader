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

from nautilus_trader.common.execution_engine cimport ExecutionEngine
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.model.commands cimport AccountInquiry
from nautilus_trader.model.commands cimport CancelOrder
from nautilus_trader.model.commands cimport ModifyOrder
from nautilus_trader.model.commands cimport SubmitBracketOrder
from nautilus_trader.model.commands cimport SubmitOrder
from nautilus_trader.model.identifiers cimport TraderId


cdef class ExecutionClient:
    cdef LoggerAdapter _log
    cdef ExecutionEngine _engine

    cdef readonly TraderId trader_id
    cdef readonly int command_count
    cdef readonly int event_count

    # -- ABSTRACT METHODS ------------------------------------------------------------------------------
    cpdef void connect(self) except *
    cpdef void disconnect(self) except *
    cpdef void reset(self) except *
    cpdef void dispose(self) except *
    cpdef void account_inquiry(self, AccountInquiry command) except *
    cpdef void submit_order(self, SubmitOrder command) except *
    cpdef void submit_bracket_order(self, SubmitBracketOrder command) except *
    cpdef void modify_order(self, ModifyOrder command) except *
    cpdef void cancel_order(self, CancelOrder command) except *
    # --------------------------------------------------------------------------------------------------
    cdef void _reset(self) except *
