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

from nautilus_trader.config import LiveDataEngineConfig

from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.queue cimport Queue
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.data cimport Data
from nautilus_trader.core.message cimport Command
from nautilus_trader.data.engine cimport DataEngine
from nautilus_trader.data.messages cimport DataCommand
from nautilus_trader.data.messages cimport DataRequest
from nautilus_trader.data.messages cimport DataResponse
from nautilus_trader.msgbus.bus cimport MessageBus


cdef class LiveDataEngine(DataEngine):
    """
    Provides a high-performance asynchronous live data engine.

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
    config : LiveDataEngineConfig, optional
        The configuration for the instance.

    Raises
    ------
    TypeError
        If `config` is not of type `LiveDataEngineConfig`.
    """
    _sentinel = None

    def __init__(
        self,
        loop not None: asyncio.AbstractEventLoop,
        MessageBus msgbus not None,
        Cache cache not None,
        LiveClock clock not None,
        Logger logger not None,
        config: Optional[LiveDataEngineConfig] = None,
    ):
        if config is None:
            config = LiveDataEngineConfig()
        Condition.type(config, LiveDataEngineConfig, "config")
        super().__init__(
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            config=config,
        )

        self._loop = loop
        self._cmd_queue = Queue(maxsize=config.qsize)
        self._req_queue = Queue(maxsize=config.qsize)
        self._res_queue = Queue(maxsize=config.qsize)
        self._data_queue = Queue(maxsize=config.qsize)

        # Async tasks
        self._cmd_queue_task = None
        self._req_queue_task = None
        self._res_queue_task = None
        self._data_queue_task = None
        self.is_running = False

    def connect(self):
        """
        Connect the engine by calling connect on all registered clients.
        """
        self._log.info("Connecting all clients...")
        for client in self._clients.values():
            client.connect()

    def disconnect(self):
        """
        Disconnect the engine by calling disconnect on all registered clients.
        """
        self._log.info("Disconnecting all clients...")
        for client in self._clients.values():
            client.disconnect()

    def get_cmd_queue_task(self) -> Optional[asyncio.Task]:
        """
        Return the internal command queue task for the engine.

        Returns
        -------
        asyncio.Task or ``None``

        """
        return self._cmd_queue_task

    def get_req_queue_task(self) -> Optional[asyncio.Task]:
        """
        Return the internal request queue task for the engine.

        Returns
        -------
        asyncio.Task or ``None``

        """
        return self._req_queue_task

    def get_res_queue_task(self) -> Optional[asyncio.Task]:
        """
        Return the internal response queue task for the engine.

        Returns
        -------
        asyncio.Task or ``None``

        """
        return self._res_queue_task

    def get_data_queue_task(self) -> Optional[asyncio.Task]:
        """
        Return the internal data queue task for the engine.

        Returns
        -------
        asyncio.Task or ``None``

        """
        return self._data_queue_task

    def cmd_qsize(self) -> int:
        """
        Return the number of `DataCommand` objects buffered on the internal queue.

        Returns
        -------
        int

        """
        return self._cmd_queue.qsize()

    def req_qsize(self) -> int:
        """
        Return the number of `DataRequest` objects buffered on the internal queue.

        Returns
        -------
        int

        """
        return self._req_queue.qsize()

    def res_qsize(self) -> int:
        """
        Return the number of `DataResponse` objects buffered on the internal queue.

        Returns
        -------
        int

        """
        return self._res_queue.qsize()

    def data_qsize(self) -> int:
        """
        Return the number of `Data` objects buffered on the internal queue.

        Returns
        -------
        int

        """
        return self._data_queue.qsize()

    def kill(self) -> None:
        """
        Kill the engine by abruptly canceling the queue tasks and calling stop.
        """
        self._log.warning("Killing engine...")
        if self._cmd_queue_task:
            self._log.debug(f"Canceling {self._cmd_queue_task.get_name()}...")
            self._cmd_queue_task.cancel()
            self._cmd_queue_task.done()
        if self._req_queue_task:
            self._log.debug(f"Canceling {self._req_queue_task.get_name()}...")
            self._req_queue_task.cancel()
            self._req_queue_task.done()
        if self._res_queue_task:
            self._log.debug(f"Canceling {self._res_queue_task.get_name()}...")
            self._res_queue_task.cancel()
            self._res_queue_task.done()
        if self._data_queue_task:
            self._log.debug(f"Canceling {self._data_queue_task.get_name()}...")
            self._data_queue_task.cancel()
            self._data_queue_task.done()
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
            self._cmd_queue.put_nowait(command)
        except asyncio.QueueFull:
            self._log.warning(
                f"Blocking on `_cmd_queue.put` as queue full at "
                f"{self._cmd_queue.qsize()} items.",
            )
            self._loop.create_task(self._cmd_queue.put(command))  # Blocking until qsize reduces

    cpdef void request(self, DataRequest request) except *:
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
            self._req_queue.put_nowait(request)
        except asyncio.QueueFull:
            self._log.warning(
                f"Blocking on `_req_queue.put` as queue full at "
                f"{self._req_queue.qsize()} items.",
            )
            self._loop.create_task(self._req_queue.put(request))  # Blocking until qsize reduces

    cpdef void response(self, DataResponse response) except *:
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

        try:
            self._res_queue.put_nowait(response)
        except asyncio.QueueFull:
            self._log.warning(
                f"Blocking on `_res_queue.put` as queue full at "
                f"{self._res_queue.qsize()} items.",
            )
            self._loop.create_task(self._res_queue.put(response))  # Blocking until qsize reduces

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
            self._log.warning(
                f"Blocking on `_data_queue.put` as queue full at "
                f"{self._data_queue.qsize()} items.",
            )
            self._loop.create_task(self._data_queue.put(data))  # Blocking until qsize reduces

# -- INTERNAL -------------------------------------------------------------------------------------

    def _enqueue_sentinels(self) -> None:
        self._cmd_queue.put_nowait(self._sentinel)
        self._req_queue.put_nowait(self._sentinel)
        self._res_queue.put_nowait(self._sentinel)
        self._data_queue.put_nowait(self._sentinel)
        self._log.debug(f"Sentinel messages placed on queues.")

    cpdef void _on_start(self) except *:
        if not self._loop.is_running():
            self._log.warning("Started when loop is not running.")

        self.is_running = True  # Queues will continue to process
        self._cmd_queue_task = self._loop.create_task(self._run_cmd_queue(), name="cmd_queue")
        self._res_queue_task = self._loop.create_task(self._run_req_queue(), name="req_queue")
        self._req_queue_task = self._loop.create_task(self._run_res_queue(), name="res_queue")
        self._data_queue_task = self._loop.create_task(self._run_data_queue(), name="data_queue")

        self._log.debug(f"Scheduled {self._cmd_queue_task}")
        self._log.debug(f"Scheduled {self._res_queue_task}")
        self._log.debug(f"Scheduled {self._req_queue_task}")
        self._log.debug(f"Scheduled {self._data_queue_task}")

    cpdef void _on_stop(self) except *:
        if self.is_running:
            self.is_running = False
            self._enqueue_sentinels()

    async def _run_cmd_queue(self):
        self._log.debug(
            f"DataCommand message queue processing starting (qsize={self.cmd_qsize()})...",
        )
        cdef Command command
        try:
            while self.is_running:
                command = await self._cmd_queue.get()
                if command is None:  # Sentinel message (fast C-level check)
                    continue         # Returns to the top to check `self.is_running`
                self._execute_command(command)
        except asyncio.CancelledError:
            if not self._cmd_queue.empty():
                self._log.warning(
                    f"DataCommand message queue processing stopped "
                    f"with {self.cmd_qsize()} message(s) on queue.",
                )
            else:
                self._log.debug("DataCommand message queue processing stopped.")

    async def _run_req_queue(self):
        self._log.debug(
            f"DataRequest message queue processing starting (qsize={self.req_qsize()})...",
        )
        cdef DataRequest request
        try:
            while self.is_running:
                request = await self._req_queue.get()
                if request is None:  # Sentinel message (fast C-level check)
                    continue         # Returns to the top to check `self.is_running`
                self._handle_request(request)
        except asyncio.CancelledError:
            if not self._req_queue.empty():
                self._log.warning(
                    f"DataRequest message queue processing stopped "
                    f"with {self.req_qsize()} message(s) on queue.",
                )
            else:
                self._log.debug("DataRequest message queue processing stopped.")

    async def _run_res_queue(self):
        self._log.debug(
            f"DataResponse message queue processing starting (qsize={self.req_qsize()})...",
        )
        cdef DataResponse response
        try:
            while self.is_running:
                response = await self._res_queue.get()
                if response is None:  # Sentinel message (fast C-level check)
                    continue          # Returns to the top to check `self.is_running`
                self._handle_response(response)
        except asyncio.CancelledError:
            if not self._res_queue.empty():
                self._log.warning(
                    f"DataResponse message queue processing stopped "
                    f"with {self.res_qsize()} message(s) on queue.",
                )
            else:
                self._log.debug("DataResponse message queue processing stopped.")

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
                self._log.warning(
                    f"Data queue processing stopped "
                    f"with {self.data_qsize()} item(s) on queue.",
                )
            else:
                self._log.debug("Data queue processing stopped.")
