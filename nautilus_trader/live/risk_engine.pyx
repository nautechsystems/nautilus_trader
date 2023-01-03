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

import asyncio
from typing import Optional

from nautilus_trader.config import LiveRiskEngineConfig

from nautilus_trader.cache.base cimport CacheFacade
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.queue cimport Queue
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.message cimport Command
from nautilus_trader.core.message cimport Event
from nautilus_trader.core.message cimport Message
from nautilus_trader.core.message cimport MessageCategory
from nautilus_trader.msgbus.bus cimport MessageBus
from nautilus_trader.portfolio.base cimport PortfolioFacade


cdef class LiveRiskEngine(RiskEngine):
    """
    Provides a high-performance asynchronous live risk engine.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the engine.
    portfolio : PortfolioFacade
        The portfolio for the engine.
    msgbus : MessageBus
        The message bus for the engine.
    cache : CacheFacade
        The read-only cache for the engine.
    clock : Clock
        The clock for the engine.
    logger : Logger
        The logger for the engine.
    config : LiveRiskEngineConfig
        The configuration for the instance.

    Raises
    ------
    TypeError
        If `config` is not of type `LiveRiskEngineConfig`.
    """

    def __init__(
        self,
        loop not None: asyncio.AbstractEventLoop,
        PortfolioFacade portfolio not None,
        MessageBus msgbus not None,
        CacheFacade cache not None,
        LiveClock clock not None,
        Logger logger not None,
        config: Optional[LiveRiskEngineConfig] = None,
    ):
        """
        Initialize a new instance of the ``LiveRiskEngine`` class.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the engine.
        portfolio : PortfolioFacade
            The portfolio for the engine.
        msgbus : MessageBus
            The message bus for the engine.
        cache : CacheFacade
            The read-only cache for the engine.
        clock : Clock
            The clock for the engine.
        logger : Logger
            The logger for the engine.
        config : LiveRiskEngineConfig
            The configuration for the instance.

        Raises
        ------
        TypeError
            If `config` is not of type `LiveRiskEngineConfig`.

        """
        if config is None:
            config = LiveRiskEngineConfig()
        Condition.type(config, LiveRiskEngineConfig, "config")
        super().__init__(
            portfolio=portfolio,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            config=config,
        )

        self._loop = loop
        self._queue = Queue(maxsize=config.qsize)

        self._run_queue_task = None
        self.is_running = False

    cpdef object get_event_loop(self):
        """
        Return the internal event loop for the engine.

        Returns
        -------
        asyncio.AbstractEventLoop

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

# -- COMMANDS -------------------------------------------------------------------------------------

    cpdef void kill(self) except *:
        """
        Kill the engine by abruptly cancelling the queue task and calling stop.
        """
        self._log.warning("Killing engine...")
        if self._run_queue_task:
            self._log.debug("Canceling run_queue_task...")
            self._run_queue_task.cancel()
        if self.is_running:
            self.is_running = False  # Avoids sentinel messages for queues
            self.stop()

    cpdef void execute(self, Command command) except *:
        """
        Execute the given command.

        If the internal queue is already full then will log a warning and block
        until queue size reduces.

        Parameters
        ----------
        command : Command
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
            self._loop.create_task(self._queue.put(command))

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

# -- INTERNAL -------------------------------------------------------------------------------------

    cpdef void _on_start(self) except *:
        if not self._loop.is_running():
            self._log.warning("Started when loop is not running.")

        self.is_running = True  # Queue will continue to process
        self._run_queue_task = self._loop.create_task(self._run())

        self._log.debug(f"Scheduled {self._run_queue_task}")

    cpdef void _on_stop(self) except *:
        if self.is_running:
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
                if message.category == MessageCategory.EVENT:
                    self._handle_event(message)
                elif message.category == MessageCategory.COMMAND:
                    self._execute_command(message)
                else:
                    self._log.error(f"Cannot handle message: unrecognized {message}.")
        except asyncio.CancelledError:
            if self.qsize() > 0:
                self._log.warning(f"Running canceled "
                                  f"with {self.qsize()} message(s) on queue.")
            else:
                self._log.debug(f"Message queue processing stopped (qsize={self.qsize()}).")
