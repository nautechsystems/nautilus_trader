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
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.portfolio cimport Portfolio
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.message cimport Message
from nautilus_trader.core.message cimport MessageType
from nautilus_trader.execution.database cimport ExecutionDatabase
from nautilus_trader.execution.engine cimport ExecutionEngine
from nautilus_trader.model.commands cimport Command
from nautilus_trader.model.events cimport Event
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport TraderId


cdef class LiveExecutionEngine(ExecutionEngine):
    """
    Provides a process and thread safe high performance execution engine.
    """

    def __init__(
            self,
            TraderId trader_id not None,
            AccountId account_id not None,
            ExecutionDatabase database not None,
            Portfolio portfolio not None,
            Clock clock not None,
            UUIDFactory uuid_factory not None,
            Logger logger not None,
    ):
        """
        Initialize a new instance of the LiveExecutionEngine class.

        Parameters
        ----------
        trader_id : TraderId
            The trader identifier for the engine.
        account_id : AccountId
            The account_id for the engine.
        database : ExecutionDatabase
            The execution database for the engine.
        portfolio : Portfolio
            The portfolio for the engine.
        clock : Clock
            The clock for the engine.
        uuid_factory : UUIDFactory
            The uuid factory for the engine.
        logger : Logger
            The logger for the engine.

        """
        super().__init__(
            trader_id=trader_id,
            account_id=account_id,
            database=database,
            portfolio=portfolio,
            clock=clock,
            uuid_factory=uuid_factory,
            logger=logger,
        )

        self._queue = queue.Queue()
        self._thread = threading.Thread(target=self._loop, daemon=True)
        self._thread.start()

    cpdef void execute(self, Command command) except *:
        """
        Execute the given command by inserting it into the message bus for processing.

        Parameters
        ----------
        command : Command
            The command to execute.

        """
        Condition.not_none(command, "command")

        self._queue.put(command)

    cpdef void process(self, Event event) except *:
        """
        Handle the given event by inserting it into the message bus for processing.

        Parameters
        ----------
        event : Event
            The event to process.

        """
        Condition.not_none(event, "event")

        self._queue.put(event)

    cpdef void _loop(self) except *:
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
