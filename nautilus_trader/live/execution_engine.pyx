# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.live.config import LiveExecEngineConfig

from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.queue cimport Queue
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.message cimport Message
from nautilus_trader.core.message cimport MessageCategory
from nautilus_trader.execution.engine cimport ExecutionEngine
from nautilus_trader.live.execution_client cimport LiveExecutionClient
from nautilus_trader.model.commands.trading cimport TradingCommand
from nautilus_trader.model.events.order cimport OrderEvent
from nautilus_trader.msgbus.bus cimport MessageBus


cdef class LiveExecutionEngine(ExecutionEngine):
    """
    Provides a high-performance asynchronous live execution engine.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the engine.
    msgbus : MessageBus
        The message bus for the engine.
    cache : Cache
        The cache for the engine.
    clock : Clock
        The clock for the engine.
    logger : Logger
        The logger for the engine.
    config : LiveExecEngineConfig, optional
        The configuration for the instance.

    Raises
    ------
    TypeError
        If `config` is not of type `LiveExecEngineConfig`.
    """
    _sentinel = None

    def __init__(
        self,
        loop not None: asyncio.AbstractEventLoop,
        MessageBus msgbus not None,
        Cache cache not None,
        LiveClock clock not None,
        Logger logger not None,
        config: Optional[LiveExecEngineConfig]=None,
    ):
        if config is None:
            config = LiveExecEngineConfig()
        Condition.type(config, LiveExecEngineConfig, "config")
        super().__init__(
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

    def connect(self):
        """
        Connect the engine by calling connect on all registered clients.
        """
        for client in self._clients.values():
            client.connect()

    def disconnect(self):
        """
        Disconnect the engine by calling disconnect on all registered clients.
        """
        for client in self._clients.values():
            client.disconnect()

    def get_run_queue_task(self) -> asyncio.Task:
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

    async def reconcile_state(self, double timeout_secs) -> bool:
        """
        Reconcile the execution engines state with all execution clients.

        Parameters
        ----------
        timeout_secs : double
            The seconds to allow for reconciliation before timing out.

        Returns
        -------
        bool
            True if states reconcile within timeout, else False.

        Raises
        ------
        ValueError
            If `timeout_secs` is not positive (> 0).

        """
        Condition.positive(timeout_secs, "timeout_secs")

        # Request execution mass status report from each client
        mass_status_coros = [
            c.generate_mass_status() for c in self._clients.values()
        ]
        client_id_mass_status = await asyncio.gather(*mass_status_coros)

        cdef list results = []

        # Reconcile each mass status with the execution engine
        cdef LiveExecutionClient client
        for mass_status in client_id_mass_status:
            result = self._reconcile_mass_status(mass_status)
            client = self._clients[mass_status.client_id]
            client.reconciliation_active = False
            results.append(result)

        return all(results)

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
            self._log.warning(
                f"Blocking on `_queue.put` as queue full "
                f"at {self._queue.qsize()} items.",
            )
            self._loop.create_task(self._queue.put(command))  # Blocking until qsize reduces

    cpdef void process(self, OrderEvent event) except *:
        """
        Process the given event.

        If the internal queue is already full then will log a warning and block
        until queue size reduces.

        Parameters
        ----------
        event : OrderEvent
            The event to process.

        Warnings
        --------
        This method should only be called from the same thread the event loop is
        running on.

        """
        Condition.not_none(event, "event")

        try:
            self._queue.put_nowait(event)
        except asyncio.QueueFull:
            self._log.warning(
                f"Blocking on `_queue.put` as queue full "
                f"at {self._queue.qsize()} items.",
            )
            self._loop.create_task(self._queue.put(event))  # Blocking until qsize reduces

    cpdef void _on_start(self) except *:
        if not self._loop.is_running():
            self._log.warning("Started when loop is not running.")

        self.is_running = True  # Queue will continue to process
        self._run_queue_task = self._loop.create_task(self._run())

        self._log.debug(f"Scheduled {self._run_queue_task}")

    cpdef void _on_stop(self) except *:
        if self.is_running:
            self.is_running = False
            self._enqueue_sentinel()

    async def _run(self):
        self._log.debug(
            f"Message queue processing starting (qsize={self.qsize()})...",
        )
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
            if not self._queue.empty():
                self._log.warning(
                    f"Running canceled with {self.qsize()} message(s) on queue.",
                )
            else:
                self._log.debug(
                    f"Message queue processing stopped (qsize={self.qsize()}).",
                )

    cdef void _enqueue_sentinel(self) except *:
        self._queue.put_nowait(self._sentinel)
        self._log.debug(f"Sentinel message placed on message queue.")
