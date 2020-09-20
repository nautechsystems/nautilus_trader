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

from nautilus_trader.backtest.logging cimport TestLogger
from nautilus_trader.backtest.simulated_broker cimport SimulatedBroker
from nautilus_trader.common.execution_client cimport ExecutionClient
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.commands cimport AccountInquiry
from nautilus_trader.model.commands cimport CancelOrder
from nautilus_trader.model.commands cimport ModifyOrder
from nautilus_trader.model.commands cimport SubmitBracketOrder
from nautilus_trader.model.commands cimport SubmitOrder


cdef class BacktestExecClient(ExecutionClient):
    """
    Provides an execution client for the BacktestEngine.
    """

    def __init__(self, SimulatedBroker broker not None, TestLogger logger not None):
        """
        Initialize a new instance of the BacktestExecClient class.

        :param broker: The simulated brokerage for the backtest.
        :param logger: The logger for the component.
        """
        super().__init__(broker.exec_engine, logger)

        self._broker = broker

    cpdef void connect(self) except *:
        """
        Connect to the execution service.
        """
        self._log.info("Connected.")
        # Do nothing else

    cpdef void disconnect(self) except *:
        """
        Disconnect from the execution service.
        """
        self._log.info("Disconnected.")
        # Do nothing else

    cpdef void reset(self) except *:
        """
        Return the client to its initial state preserving tick data.
        """
        self._log.debug(f"Resetting...")

        self._reset()

        self._log.info("Reset.")

    cpdef void dispose(self) except *:
        """
        Dispose of the execution client.
        """
        pass

    # -- COMMAND EXECUTION -----------------------------------------------------------------------------

    cpdef void account_inquiry(self, AccountInquiry command) except *:
        Condition.not_none(command, "command")

        self._broker.handle_account_inquiry(command)

    cpdef void submit_order(self, SubmitOrder command) except *:
        Condition.not_none(command, "command")

        self._broker.handle_submit_order(command)

    cpdef void submit_bracket_order(self, SubmitBracketOrder command) except *:
        Condition.not_none(command, "command")

        self._broker.handle_submit_bracket_order(command)

    cpdef void cancel_order(self, CancelOrder command) except *:
        Condition.not_none(command, "command")

        self._broker.handle_cancel_order(command)

    cpdef void modify_order(self, ModifyOrder command) except *:
        Condition.not_none(command, "command")

        self._broker.handle_modify_order(command)
