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

from __future__ import annotations

import asyncio
from asyncio import Queue

from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.config import LiveDataEngineConfig
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.data import Data
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.data.messages import DataCommand
from nautilus_trader.data.messages import DataRequest
from nautilus_trader.data.messages import DataResponse
from nautilus_trader.msgbus.bus import MessageBus


class LiveDataEngine(DataEngine):
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
    clock : LiveClock
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
        loop: asyncio.AbstractEventLoop,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        logger: Logger,
        config: LiveDataEngineConfig | None = None,
    ) -> None:
        if config is None:
            config = LiveDataEngineConfig()
        PyCondition.type(config, LiveDataEngineConfig, "config")
        super().__init__(
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            config=config,
        )

        self._loop: asyncio.AbstractEventLoop = loop
        self._cmd_queue: asyncio.Queue = Queue(maxsize=config.qsize)
        self._req_queue: asyncio.Queue = Queue(maxsize=config.qsize)
        self._res_queue: asyncio.Queue = Queue(maxsize=config.qsize)
        self._data_queue: asyncio.Queue = Queue(maxsize=config.qsize)

        # Async tasks
        self._cmd_queue_task: asyncio.Task | None = None
        self._req_queue_task: asyncio.Task | None = None
        self._res_queue_task: asyncio.Task | None = None
        self._data_queue_task: asyncio.Task | None = None
        self._kill: bool = False

    def connect(self) -> None:
        """
        Connect the engine by calling connect on all registered clients.
        """
        self._log.info("Connecting all clients...")
        for client in self._clients.values():
            client.connect()

    def disconnect(self) -> None:
        """
        Disconnect the engine by calling disconnect on all registered clients.
        """
        self._log.info("Disconnecting all clients...")
        for client in self._clients.values():
            client.disconnect()

    def get_cmd_queue_task(self) -> asyncio.Task | None:
        """
        Return the internal command queue task for the engine.

        Returns
        -------
        asyncio.Task or ``None``

        """
        return self._cmd_queue_task

    def get_req_queue_task(self) -> asyncio.Task | None:
        """
        Return the internal request queue task for the engine.

        Returns
        -------
        asyncio.Task or ``None``

        """
        return self._req_queue_task

    def get_res_queue_task(self) -> asyncio.Task | None:
        """
        Return the internal response queue task for the engine.

        Returns
        -------
        asyncio.Task or ``None``

        """
        return self._res_queue_task

    def get_data_queue_task(self) -> asyncio.Task | None:
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
        self._kill = True
        self.stop()
        if self._cmd_queue_task:
            self._log.debug(f"Canceling {self._cmd_queue_task.get_name()}...")
            self._cmd_queue_task.cancel()
            self._cmd_queue_task = None
        if self._req_queue_task:
            self._log.debug(f"Canceling {self._req_queue_task.get_name()}...")
            self._req_queue_task.cancel()
            self._req_queue_task = None
        if self._res_queue_task:
            self._log.debug(f"Canceling {self._res_queue_task.get_name()}...")
            self._res_queue_task.cancel()
            self._res_queue_task = None
        if self._data_queue_task:
            self._log.debug(f"Canceling {self._data_queue_task.get_name()}...")
            self._data_queue_task.cancel()
            self._data_queue_task = None

    def execute(self, command: DataCommand) -> None:
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
        This method is not thread-safe and should only be called from the same thread the event
        loop is running on. Calling it from a different thread may lead to unexpected behavior.

        """
        PyCondition.not_none(command, "command")
        # Do not allow None through (None is a sentinel value which stops the queue)

        try:
            self._cmd_queue.put_nowait(command)
        except asyncio.QueueFull:
            self._log.warning(
                f"Blocking on `_cmd_queue.put` as queue full at "
                f"{self._cmd_queue.qsize()} items.",
            )
            # Schedule the `put` operation to be executed once there is space in the queue
            self._loop.create_task(self._cmd_queue.put(command))

    def request(self, request: DataRequest) -> None:
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
        This method is not thread-safe and should only be called from the same thread the event
        loop is running on. Calling it from a different thread may lead to unexpected behavior.

        """
        PyCondition.not_none(request, "request")
        # Do not allow None through (None is a sentinel value which stops the queue)

        try:
            self._req_queue.put_nowait(request)
        except asyncio.QueueFull:
            self._log.warning(
                f"Blocking on `_req_queue.put` as queue full at "
                f"{self._req_queue.qsize()} items.",
            )
            # Schedule the `put` operation to be executed once there is space in the queue
            self._loop.create_task(self._req_queue.put(request))

    def response(self, response: DataResponse) -> None:
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
        This method is not thread-safe and should only be called from the same thread the event
        loop is running on. Calling it from a different thread may lead to unexpected behavior.

        """
        PyCondition.not_none(response, "response")

        try:
            self._res_queue.put_nowait(response)
        except asyncio.QueueFull:
            self._log.warning(
                f"Blocking on `_res_queue.put` as queue full at "
                f"{self._res_queue.qsize():_} items.",
            )
            # Schedule the `put` operation to be executed once there is space in the queue
            self._loop.create_task(self._res_queue.put(response))

    def process(self, data: Data) -> None:
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
        This method is not thread-safe and should only be called from the same thread the event
        loop is running on. Calling it from a different thread may lead to unexpected behavior.

        """
        PyCondition.not_none(data, "data")
        # Do not allow None through (None is a sentinel value which stops the queue)

        try:
            self._data_queue.put_nowait(data)
        except asyncio.QueueFull:
            self._log.warning(
                f"Blocking on `_data_queue.put` as queue full at "
                f"{self._data_queue.qsize():_} items.",
            )
            # Schedule the `put` operation to be executed once there is space in the queue
            self._loop.create_task(self._data_queue.put(data))

    # -- INTERNAL -------------------------------------------------------------------------------------

    def _enqueue_sentinels(self) -> None:
        self._cmd_queue.put_nowait(self._sentinel)
        self._req_queue.put_nowait(self._sentinel)
        self._res_queue.put_nowait(self._sentinel)
        self._data_queue.put_nowait(self._sentinel)
        self._log.debug("Sentinel messages placed on queues.")

    def _on_start(self) -> None:
        if not self._loop.is_running():
            self._log.warning("Started when loop is not running.")

        self._cmd_queue_task = self._loop.create_task(self._run_cmd_queue(), name="cmd_queue")
        self._req_queue_task = self._loop.create_task(self._run_res_queue(), name="res_queue")
        self._res_queue_task = self._loop.create_task(self._run_req_queue(), name="req_queue")
        self._data_queue_task = self._loop.create_task(self._run_data_queue(), name="data_queue")

        self._log.debug(f"Scheduled {self._cmd_queue_task}")
        self._log.debug(f"Scheduled {self._req_queue_task}")
        self._log.debug(f"Scheduled {self._res_queue_task}")
        self._log.debug(f"Scheduled {self._data_queue_task}")

    def _on_stop(self) -> None:
        if self._kill:
            return  # Avoids queuing redundant sentinel messages

        # This will stop the queues processing as soon as they see the sentinel message
        self._enqueue_sentinels()

    async def _run_cmd_queue(self) -> None:
        self._log.debug(
            f"DataCommand message queue processing starting (qsize={self.cmd_qsize()})...",
        )
        try:
            while True:
                command: DataCommand | None = await self._cmd_queue.get()
                if command is self._sentinel:
                    break
                self._execute_command(command)
        except asyncio.CancelledError:
            self._log.warning("DataCommand message queue canceled.")
        except RuntimeError as ex:
            self._log.error(f"RuntimeError: {ex}.")
        finally:
            stopped_msg = "DataCommand message queue stopped"
            if not self._cmd_queue.empty():
                self._log.warning(f"{stopped_msg} with {self.cmd_qsize()} message(s) on queue.")
            else:
                self._log.debug(stopped_msg + ".")

    async def _run_req_queue(self) -> None:
        self._log.debug(
            f"DataRequest message queue processing starting (qsize={self.req_qsize()})...",
        )
        try:
            while True:
                request: DataRequest | None = await self._req_queue.get()
                if request is self._sentinel:
                    break
                self._handle_request(request)
        except asyncio.CancelledError:
            self._log.warning("DataRequest message queue canceled.")
        except RuntimeError as ex:
            self._log.error(f"RuntimeError: {ex}.")
        finally:
            stopped_msg = "DataRequest message queue stopped"
            if not self._req_queue.empty():
                self._log.warning(f"{stopped_msg} with {self.req_qsize()} message(s) on queue.")
            else:
                self._log.debug(stopped_msg + ".")

    async def _run_res_queue(self) -> None:
        self._log.debug(
            f"DataResponse message queue processing starting (qsize={self.res_qsize()})...",
        )
        try:
            while True:
                response: DataRequest | None = await self._res_queue.get()
                if response is self._sentinel:
                    break
                self._handle_response(response)
        except asyncio.CancelledError:
            self._log.warning("DataResponse message queue canceled.")
        except RuntimeError as ex:
            self._log.error(f"RuntimeError: {ex}.")
        finally:
            stopped_msg = "DataResponse message queue stopped"
            if not self._res_queue.empty():
                self._log.warning(f"{stopped_msg} with {self.res_qsize()} message(s) on queue.")
            else:
                self._log.debug(stopped_msg + ".")

    async def _run_data_queue(self) -> None:
        self._log.debug(f"Data queue processing starting (qsize={self.data_qsize()})...")
        try:
            while True:
                data: Data | None = await self._data_queue.get()
                if data is self._sentinel:
                    break
                self._handle_data(data)
        except asyncio.CancelledError:
            self._log.warning("Data message queue canceled.")
        except RuntimeError as ex:
            self._log.error(f"RuntimeError: {ex}.")
        finally:
            stopped_msg = "Data message queue stopped"
            if not self._data_queue.empty():
                self._log.warning(f"{stopped_msg} with {self.data_qsize()} message(s) on queue.")
            else:
                self._log.debug(stopped_msg + ".")
