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

from asyncio import AbstractEventLoop
import asyncio

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.message cimport Message
from nautilus_trader.core.message cimport MessageType
from nautilus_trader.execution.database cimport ExecutionDatabase
from nautilus_trader.execution.engine cimport ExecutionEngine
from nautilus_trader.model.commands cimport Command
from nautilus_trader.model.events cimport Event
from nautilus_trader.trading.portfolio cimport Portfolio


cdef class LiveExecutionEngine(ExecutionEngine):
    """
    Provides a high-performance asynchronous live execution engine.
    """

    def __init__(
            self,
            loop: AbstractEventLoop,
            ExecutionDatabase database not None,
            Portfolio portfolio not None,
            Clock clock not None,
            UUIDFactory uuid_factory not None,
            Logger logger not None,
            dict config=None,
    ):
        """
        Initialize a new instance of the `LiveExecutionEngine` class.

        Parameters
        ----------
        loop : AbstractEventLoop
            The event loop for the engine.
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
        config : dict, option
            The configuration options.

        """
        super().__init__(
            database=database,
            portfolio=portfolio,
            clock=clock,
            uuid_factory=uuid_factory,
            logger=logger,
            config=config,
        )

        self._loop = loop
        self._queue = asyncio.Queue()
        self._task_queue = None
        self._is_running = False

    cpdef void on_start(self) except *:
        self._log.info("Starting queue processing...")
        if not self._loop.is_running():
            self._log.warning("Started when loop is not running.")
        self._is_running = True
        self._task_queue = self._loop.create_task(self._run_queue())

        self._log.info(f"Scheduled {self._task_queue}")

    cpdef void on_stop(self) except *:
        self._log.info("Shutting down queue processing...")
        self._is_running = False
        self._queue.put_nowait(None)  # None message pattern
        self._loop.run_until_complete(self._task_queue)

    async def _run_queue(self):
        cdef Message message
        while self._is_running:
            message = await self._queue.get()
            if message is None:
                continue
            if message.type == MessageType.EVENT:
                self._handle_event(message)
            elif message.type == MessageType.COMMAND:
                self._execute_command(message)
            else:
                self._log.error(f"Cannot handle unrecognized message {message}.")

        self._log.info("Finished processing message queue.")

    cpdef int qsize(self) except *:
        """
        Return the number of messages buffered on the internal queue.

        Returns
        -------
        int

        """
        return self._queue.qsize()

    cpdef void execute(self, Command command) except *:
        """
        Execute the given command.

        Parameters
        ----------
        command : Command
            The command to execute.

        """
        Condition.not_none(command, "command")

        self._queue.put_nowait(command)

    cpdef void process(self, Event event) except *:
        """
        Process the given event.

        Parameters
        ----------
        event : Event
            The event to process.

        """
        Condition.not_none(event, "event")

        self._queue.put_nowait(event)
