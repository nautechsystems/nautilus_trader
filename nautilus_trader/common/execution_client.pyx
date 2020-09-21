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

from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.model.commands cimport AccountInquiry
from nautilus_trader.model.commands cimport CancelOrder
from nautilus_trader.model.commands cimport ModifyOrder
from nautilus_trader.model.commands cimport SubmitBracketOrder
from nautilus_trader.model.commands cimport SubmitOrder


cdef class ExecutionClient:
    """
    The base class for all execution clients.
    """

    def __init__(self, ExecutionEngine exec_engine not None, Logger logger not None):
        """
        Initialize a new instance of the ExecutionClient class.

        :param exec_engine: The execution engine to connect to the client.
        :param logger: The logger for the component.
        """
        self._exec_engine = exec_engine
        self._log = LoggerAdapter(self.__class__.__name__, logger)

        self.trader_id = exec_engine.trader_id
        self.command_count = 0
        self.event_count = 0

        self._log.info(f"Initialized.")

    # -- ABSTRACT METHODS ------------------------------------------------------------------------------

    cpdef void connect(self) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void disconnect(self) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void reset(self) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void dispose(self) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void account_inquiry(self, AccountInquiry command) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void submit_order(self, SubmitOrder command) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void submit_bracket_order(self, SubmitBracketOrder command) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void modify_order(self, ModifyOrder command) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void cancel_order(self, CancelOrder command) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")
    # -----------------------------------------------------------------------------#

    cdef void _reset(self) except *:
        # Reset the class to its initial state
        self.command_count = 0
        self.event_count = 0
