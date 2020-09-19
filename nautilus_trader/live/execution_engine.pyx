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

import queue
import threading

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.execution_database cimport ExecutionDatabase
from nautilus_trader.common.execution_engine cimport ExecutionEngine
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.portfolio cimport Portfolio
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.message cimport Message
from nautilus_trader.core.message cimport MessageType
from nautilus_trader.model.commands cimport Command
from nautilus_trader.model.events cimport Event
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport TraderId


cdef class LiveExecutionEngine(ExecutionEngine):
    """
    Provides a process and thread safe execution engine utilizing Redis.
    """

    def __init__(self,
                 TraderId trader_id not None,
                 AccountId account_id not None,
                 ExecutionDatabase database not None,
                 Portfolio portfolio not None,
                 Clock clock not None,
                 UUIDFactory uuid_factory not None,
                 Logger logger not None):
        """
        Initialize a new instance of the LiveExecutionEngine class.

        :param trader_id: The trader_id for the engine.
        :param account_id: The account_id for the engine.
        :param database: The execution database for the engine.
        :param portfolio: The portfolio for the engine.
        :param clock: The clock for the engine.
        :param uuid_factory: The uuid factory for the engine.
        :param logger: The logger for the engine.
        """
        super().__init__(
            trader_id=trader_id,
            account_id=account_id,
            database=database,
            portfolio=portfolio,
            clock=clock,
            uuid_factory=uuid_factory,
            logger=logger)

        self._queue = queue.Queue()
        self._thread = threading.Thread(target=self._process, daemon=True)
        self._thread.start()

    cpdef void execute_command(self, Command command) except *:
        """
        Execute the given command by inserting it into the message bus for processing.

        :param command: The command to execute.
        """
        Condition.not_none(command, "command")

        self._queue.put(command)

    cpdef void handle_event(self, Event event) except *:
        """
        Handle the given event by inserting it into the message bus for processing.

        :param event: The event to handle
        """
        Condition.not_none(event, "event")

        self._queue.put(event)

    cpdef void _process(self) except *:
        self._log.info("Running...")

        cdef Message message
        while True:
            message = self._queue.get()

            if message.message_type == MessageType.EVENT:
                self._handle_event(message)
            elif message.message_type == MessageType.COMMAND:
                self._execute_command(message)
            else:
                self._log.error(f"Invalid message type on queue ({repr(message)}).")
