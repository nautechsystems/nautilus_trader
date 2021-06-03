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

from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.queue cimport Queue
from nautilus_trader.core.constants cimport *  # str constants only
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.message cimport Message
from nautilus_trader.core.message cimport MessageType
from nautilus_trader.data.engine cimport DataEngine
from nautilus_trader.data.messages cimport DataCommand
from nautilus_trader.data.messages cimport DataRequest
from nautilus_trader.data.messages cimport DataResponse
from nautilus_trader.model.data cimport Data
from nautilus_trader.trading.portfolio cimport Portfolio


cdef class LiveDataEngine(DataEngine):
    """
    Provides a high-performance asynchronous live data engine.
    """
    _sentinel = None

    def __init__(
        self,
        loop not None: asyncio.AbstractEventLoop,
        Portfolio portfolio not None,
        Cache cache not None,
        LiveClock clock not None,
        Logger logger not None,
        dict config=None,
    ):
        """
        Initialize a new instance of the ``LiveDataEngine`` class.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the engine.
        portfolio : int
            The portfolio to register.
        cache : Cache
            The cache for the engine.
        clock : Clock
            The clock for the engine.
        logger : Logger
            The logger for the engine.
        config : dict[str, object], optional
            The configuration options.

        """
        if config is None:
            config = {}
        if "qsize" not in config:
            config["qsize"] = 10000
        super().__init__(
            portfolio,
            cache,
            clock,
            logger,
            config,
        )

        self._loop = loop
        self._data_queue = Queue(maxsize=config.get("qsize"))
        self._message_queue = Queue(maxsize=config.get("qsize"))

        self._run_queues_task = None
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
        return self._run_queues_task

    cpdef int data_qsize(self) except *:
        """
        Return the number of objects buffered on the internal data queue.

        Returns
        -------
        int

        """
        return self._data_queue.qsize()

    cpdef int message_qsize(self) except *:
        """
        Return the number of objects buffered on the internal message queue.

        Returns
        -------
        int

        """
        return self._message_queue.qsize()

    cpdef void kill(self) except *:
        """
        Kill the engine by abruptly cancelling the queue tasks and calling stop.
        """
        self._log.warning("Killing engine...")
        if self._run_queues_task:
            self._log.debug("Cancelling run_queues_task...")
            self._run_queues_task.cancel()
        if self.is_running:
            self.is_running = False  # Avoids sentinel messages for queues
            self.stop()

    cpdef void execute(self, DataCommand command) except *:
        """
        Execute the given data command.

        If the internal queue is already full then will log a warning and block
        until queue size reduces.

        Parameters
        ----------
        command : DataCommand
            The command to execute.

        Warnings
        --------
        This method should only be called from the same thread the event loop is
        running on.

        """
        Condition.not_none(command, "command")
        # Do not allow None through (None is a sentinel value which stops the queue)

        try:
            self._message_queue.put_nowait(command)
        except asyncio.QueueFull:
            self._log.warning(f"Blocking on `_message_queue.put` as message_queue full at "
                              f"{self._message_queue.qsize()} items.")
            self._loop.create_task(self._message_queue.put(command))  # Blocking until qsize reduces

    cpdef void process(self, Data data) except *:
        """
        Process the given data.

        If the internal queue is already full then will log a warning and block
        until queue size reduces.

        Parameters
        ----------
        data : Data
            The data to process.

        Warnings
        --------
        This method should only be called from the same thread the event loop is
        running on.

        """
        Condition.not_none(data, "data")
        # Do not allow None through (None is a sentinel value which stops the queue)

        try:
            self._data_queue.put_nowait(data)
        except asyncio.QueueFull:
            self._log.warning(f"Blocking on `_data_queue.put` as data_queue full at "
                              f"{self._data_queue.qsize()} items.")
            self._loop.create_task(self._data_queue.put(data))  # Blocking until qsize reduces

    cpdef void send(self, DataRequest request) except *:
        """
        Handle the given request.

        If the internal queue is already full then will log a warning and block
        until queue size reduces.

        Parameters
        ----------
        request : DataRequest
            The request to handle.

        Warnings
        --------
        This method should only be called from the same thread the event loop is
        running on.

        """
        Condition.not_none(request, "request")
        # Do not allow None through (None is a sentinel value which stops the queue)

        try:
            self._message_queue.put_nowait(request)
        except asyncio.QueueFull:
            self._log.warning(f"Blocking on `_message_queue.put` as message_queue full at "
                              f"{self._message_queue.qsize()} items.")
            self._loop.create_task(self._message_queue.put(request))  # Blocking until qsize reduces

    cpdef void receive(self, DataResponse response) except *:
        """
        Handle the given response.

        If the internal queue is already full then will log a warning and block
        until queue size reduces.

        Parameters
        ----------
        response : DataResponse
            The response to handle.

        Warnings
        --------
        This method should only be called from the same thread the event loop is
        running on.

        """
        Condition.not_none(response, "response")
        # Do not allow None through (None is a sentinel value which stops the queue)

        try:
            self._message_queue.put_nowait(response)
        except asyncio.QueueFull:
            self._log.warning(f"Blocking on `_message_queue.put` as message_queue full at "
                              f"{self._message_queue.qsize()} items.")
            self._loop.create_task(self._message_queue.put(response))  # Blocking until qsize reduces

    cpdef void _on_start(self) except *:
        if not self._loop.is_running():
            self._log.warning("Started when loop is not running.")

        self.is_running = True  # Queues will continue to process

        # Run queues
        self._run_queues_task = asyncio.gather(
            self._loop.create_task(self._run_data_queue()),
            self._loop.create_task(self._run_message_queue()),
        )

        self._log.debug(f"Scheduled {self._run_queues_task}")

    cpdef void _on_stop(self) except *:
        if self.is_running:
            self.is_running = False
            self._enqueue_sentinels()

    async def _run_data_queue(self):
        self._log.debug(f"Data queue processing starting (qsize={self.data_qsize()})...")
        cdef Data data
        try:
            while self.is_running:
                data = await self._data_queue.get()
                if data is None:  # Sentinel message (fast C-level check)
                    continue      # Returns to the top to check `self.is_running`
                self._handle_data(data)
        except asyncio.CancelledError:
            if not self._data_queue.empty():
                self._log.warning(f"Running canceled "
                                  f"with {self.data_qsize()} data item(s) on queue.")
            else:
                self._log.debug(f"Data queue processing stopped (qsize={self.data_qsize()}).")

    async def _run_message_queue(self):
        self._log.debug(f"Message queue processing starting (qsize={self.message_qsize()})...")
        cdef Message message
        try:
            while self.is_running:
                message = await self._message_queue.get()
                if message is None:  # Sentinel message (fast C-level check)
                    continue         # Returns to the top to check `self.is_running`
                if message.type == MessageType.COMMAND:
                    self._execute_command(message)
                elif message.type == MessageType.REQUEST:
                    self._handle_request(message)
                elif message.type == MessageType.RESPONSE:
                    self._handle_response(message)
                else:
                    self._log.error(f"Cannot handle message: unrecognized {message}.")
        except asyncio.CancelledError:
            if not self._message_queue.empty():
                self._log.warning(f"Running canceled "
                                  f"with {self.message_qsize()} message(s) on queue.")
            else:
                self._log.debug(f"Message queue processing stopped (qsize={self.message_qsize()}).")

    cdef void _enqueue_sentinels(self):
        self._data_queue.put_nowait(self._sentinel)
        self._message_queue.put_nowait(self._sentinel)
        self._log.debug(f"Sentinel message placed on data queue.")
        self._log.debug(f"Sentinel message placed on message queue.")
