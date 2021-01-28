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

import asyncio
from asyncio import AbstractEventLoop
from asyncio import CancelledError

from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.queue cimport Queue
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.message cimport Message
from nautilus_trader.core.message cimport MessageType
from nautilus_trader.execution.client cimport ExecutionClient
from nautilus_trader.execution.database cimport ExecutionDatabase
from nautilus_trader.execution.engine cimport ExecutionEngine
from nautilus_trader.model.commands cimport TradingCommand
from nautilus_trader.model.events cimport Event
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.trading.portfolio cimport Portfolio


cdef class LiveExecutionEngine(ExecutionEngine):
    """
    Provides a high-performance asynchronous live execution engine.
    """

    def __init__(
        self,
        loop not None: AbstractEventLoop,
        ExecutionDatabase database not None,
        Portfolio portfolio not None,
        LiveClock clock not None,
        Logger logger not None,
        dict config=None,
    ):
        """
        Initialize a new instance of the `LiveExecutionEngine` class.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the engine.
        database : ExecutionDatabase
            The execution database for the engine.
        portfolio : Portfolio
            The portfolio for the engine.
        clock : Clock
            The clock for the engine.
        logger : Logger
            The logger for the engine.
        config : dict[str, object], optional
            The configuration options.

        """
        if config is None:
            config = {}
        super().__init__(
            database,
            portfolio,
            clock,
            logger,
            config,
        )

        self._loop = loop
        self._queue = Queue(maxsize=config.get("qsize", 10000))

        self._run_queue_task = None
        self.is_running = True

    cpdef object get_event_loop(self):
        """
        Return the internal event loop for the engine.

        Returns
        -------
        AbstractEventLoop

        """
        return self._loop

    cpdef object get_run_queue_task(self):
        """
        Return the internal run queue task for the engine.

        Returns
        -------
        asyncio.Task

        """
        return self._run_queue_task

    cpdef int qsize(self) except *:
        """
        Return the number of messages buffered on the internal queue.

        Returns
        -------
        int

        """
        return self._queue.qsize()

    cpdef void kill(self) except *:
        """
        Kill the engine by abruptly cancelling the queue task and calling stop.
        """
        if self._run_queue_task:
            self._run_queue_task.cancel()
        self.stop()

    cpdef void execute(self, TradingCommand command) except *:
        """
        Execute the given command.

        If the internal queue is already full then will log a warning and block
        until queue size reduces.

        Parameters
        ----------
        command : TradingCommand
            The command to execute.

        Warnings
        --------
        This method should only be called from the same thread the event loop is
        running on.

        """
        Condition.not_none(command, "command")
        # Do not allow None through (None is a sentinel value which stops the queue)

        try:
            self._queue.put_nowait(command)
        except asyncio.QueueFull:
            self._log.warning(f"Blocking on `_queue.put` as queue full at {self._queue.qsize()} items.")
            self._queue.put(command)  # Block until qsize reduces below maxsize

    cpdef void process(self, Event event) except *:
        """
        Process the given event.

        If the internal queue is already full then will log a warning and block
        until queue size reduces.

        Parameters
        ----------
        event : Event
            The event to process.

        Warnings
        --------
        This method should only be called from the same thread the event loop is
        running on.

        """
        Condition.not_none(event, "event")
        # Do not allow None through (None is a sentinel value which stops the queue)

        try:
            self._queue.put_nowait(event)
        except asyncio.QueueFull:
            self._log.warning(f"Blocking on `_queue.put` as queue full at {self._queue.qsize()} items.")
            self._queue.put(event)  # Block until qsize reduces below maxsize

    cpdef void _on_start(self) except *:
        if not self._loop.is_running():
            self._log.warning("Started when loop is not running.")

        self.is_running = True  # Queue will continue to process
        self._run_queue_task = self._loop.create_task(self._run())

        self._log.debug(f"Scheduled {self._run_queue_task}")

    cpdef void _on_stop(self) except *:
        self.is_running = False
        self._queue.put_nowait(None)  # Sentinel message pattern
        self._log.debug(f"Sentinel message placed on message queue.")

    async def _run(self):
        self._log.debug(f"Message queue processing starting (qsize={self.qsize()})...")
        cdef Message message
        try:
            while self.is_running:
                message = await self._queue.get()
                if message is None:  # Sentinel message (fast C-level check)
                    continue         # Returns to the top to check `self.is_running`
                if message.type == MessageType.EVENT:
                    self._handle_event(message)
                elif message.type == MessageType.COMMAND:
                    self._execute_command(message)
                else:
                    self._log.error(f"Cannot handle unrecognized message {message}.")
        except CancelledError:
            if self.qsize() > 0:
                self._log.warning(f"Running cancelled "
                                  f"with {self.qsize()} message(s) on queue.")
            else:
                self._log.debug(f"Message queue processing stopped (qsize={self.qsize()}).")


cdef class LiveExecutionClient(ExecutionClient):
    """
    The abstract base class for all live execution clients.

    This class should not be used directly, but through its concrete subclasses.
    """

    def __init__(
        self,
        Venue venue not None,
        AccountId account_id not None,
        LiveExecutionEngine engine not None,
        LiveClock clock not None,
        Logger logger not None,
        dict config=None,
    ):
        """
        Initialize a new instance of the `LiveExecutionClient` class.

        Parameters
        ----------
        venue : Venue
            The venue for the client.
        account_id : AccountId
            The account identifier for the client.
        engine : LiveDataEngine
            The data engine for the client.
        clock : LiveClock
            The clock for the client.
        logger : Logger
            The logger for the client.
        config : dict[str, object], optional
            The configuration options.

        """
        super().__init__(
            venue,
            account_id,
            engine,
            clock,
            logger,
            config,
        )

        self._loop: asyncio.AbstractEventLoop = engine.get_event_loop()
