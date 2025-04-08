# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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
from asyncio import Queue
from typing import Final

from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.config import LiveDataEngineConfig
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.data import Data
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.data.messages import DataCommand
from nautilus_trader.data.messages import DataResponse
from nautilus_trader.data.messages import RequestData
from nautilus_trader.live.enqueue import ThrottledEnqueuer


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
    config : LiveDataEngineConfig, optional
        The configuration for the instance.

    Raises
    ------
    TypeError
        If `config` is not of type `LiveDataEngineConfig`.

    """

    _sentinel: Final[None] = None

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        config: LiveDataEngineConfig | None = None,
    ) -> None:
        if config is None:
            config = LiveDataEngineConfig()
        PyCondition.type(config, LiveDataEngineConfig, "config")
        super().__init__(
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            config=config,
        )

        self._loop: asyncio.AbstractEventLoop = loop
        self._cmd_queue: asyncio.Queue = Queue(maxsize=config.qsize)
        self._req_queue: asyncio.Queue = Queue(maxsize=config.qsize)
        self._res_queue: asyncio.Queue = Queue(maxsize=config.qsize)
        self._data_queue: asyncio.Queue = Queue(maxsize=config.qsize)

        self._cmd_enqueuer: ThrottledEnqueuer[DataCommand] = ThrottledEnqueuer(
            qname="cmd_queue",
            queue=self._cmd_queue,
            loop=self._loop,
            clock=self._clock,
            logger=self._log,
        )
        self._req_enqueuer: ThrottledEnqueuer[RequestData] = ThrottledEnqueuer(
            qname="req_queue",
            queue=self._req_queue,
            loop=self._loop,
            clock=self._clock,
            logger=self._log,
        )
        self._res_enqueuer: ThrottledEnqueuer[DataResponse] = ThrottledEnqueuer(
            qname="res_queue",
            queue=self._res_queue,
            loop=self._loop,
            clock=self._clock,
            logger=self._log,
        )
        self._data_enqueuer: ThrottledEnqueuer[Data] = ThrottledEnqueuer(
            qname="data_queue",
            queue=self._data_queue,
            loop=self._loop,
            clock=self._clock,
            logger=self._log,
        )

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
        if self._clients:
            self._log.info("Connecting all clients...")
        else:
            self._log.warning("No clients to connect")
            return

        for client in self._clients.values():
            client.connect()

    def disconnect(self) -> None:
        """
        Disconnect the engine by calling disconnect on all registered clients.
        """
        if self._clients:
            self._log.info("Disconnecting all clients...")
        else:
            self._log.warning("No clients to disconnect")
            return

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
        Return the number of `RequestData` objects buffered on the internal queue.

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
        self._log.warning("Killing engine")
        self._kill = True
        self.stop()
        if self._cmd_queue_task:
            self._log.debug(f"Canceling task '{self._cmd_queue_task.get_name()}'")
            self._cmd_queue_task.cancel()
            self._cmd_queue_task = None
        if self._req_queue_task:
            self._log.debug(f"Canceling task '{self._req_queue_task.get_name()}'")
            self._req_queue_task.cancel()
            self._req_queue_task = None
        if self._res_queue_task:
            self._log.debug(f"Canceling task '{self._res_queue_task.get_name()}'")
            self._res_queue_task.cancel()
            self._res_queue_task = None
        if self._data_queue_task:
            self._log.debug(f"Canceling task '{self._data_queue_task.get_name()}'")
            self._data_queue_task.cancel()
            self._data_queue_task = None

    def execute(self, command: DataCommand) -> None:
        """
        Execute the given data command.

        If the internal queue is at or near capacity, it logs a warning (throttled)
        and schedules an asynchronous `put()` operation. This ensures all messages are
        eventually enqueued and processed without blocking the caller when the queue is full.

        Parameters
        ----------
        command : DataCommand
            The command to execute.

        """
        self._cmd_enqueuer.enqueue(command)

    def request(self, request: RequestData) -> None:
        """
        Handle the given request.

        If the internal queue is at or near capacity, it logs a warning (throttled)
        and schedules an asynchronous `put()` operation. This ensures all messages are
        eventually enqueued and processed without blocking the caller when the queue is full.

        Parameters
        ----------
        request : RequestData
            The request to handle.

        """
        self._req_enqueuer.enqueue(request)

    def response(self, response: DataResponse) -> None:
        """
        Handle the given response.

        If the internal queue is at or near capacity, it logs a warning (throttled)
        and schedules an asynchronous `put()` operation. This ensures all messages are
        eventually enqueued and processed without blocking the caller when the queue is full.

        Parameters
        ----------
        response : DataResponse
            The response to handle.

        """
        self._res_enqueuer.enqueue(response)

    def process(self, data: Data) -> None:
        """
        Process the given data message.

        If the internal queue is at or near capacity, it logs a warning (throttled)
        and schedules an asynchronous `put()` operation. This ensures all messages are
        eventually enqueued and processed without blocking the caller when the queue is full.

        Parameters
        ----------
        data : Data
            The data to process.

        Warnings
        --------
        This method is not thread-safe and should only be called from the same thread the event
        loop is running on. Calling it from a different thread may lead to unexpected behavior.

        """
        self._data_enqueuer.enqueue(data)

    # -- INTERNAL -------------------------------------------------------------------------------------

    def _enqueue_sentinels(self) -> None:
        self._loop.call_soon_threadsafe(self._cmd_queue.put_nowait, self._sentinel)
        self._loop.call_soon_threadsafe(self._req_queue.put_nowait, self._sentinel)
        self._loop.call_soon_threadsafe(self._res_queue.put_nowait, self._sentinel)
        self._loop.call_soon_threadsafe(self._data_queue.put_nowait, self._sentinel)
        self._log.debug("Sentinel messages placed on queues")

    def _on_start(self) -> None:
        if not self._loop.is_running():
            self._log.warning("Started when loop is not running")

        self._cmd_queue_task = self._loop.create_task(self._run_cmd_queue(), name="cmd_queue")
        self._req_queue_task = self._loop.create_task(self._run_res_queue(), name="res_queue")
        self._res_queue_task = self._loop.create_task(self._run_req_queue(), name="req_queue")
        self._data_queue_task = self._loop.create_task(self._run_data_queue(), name="data_queue")

        self._log.debug(f"Scheduled task '{self._cmd_queue_task.get_name()}'")
        self._log.debug(f"Scheduled task '{self._req_queue_task.get_name()}'")
        self._log.debug(f"Scheduled task '{self._res_queue_task.get_name()}'")
        self._log.debug(f"Scheduled task '{self._data_queue_task.get_name()}'")

    def _on_stop(self) -> None:
        if self._kill:
            return  # Avoids queuing redundant sentinel messages

        # This will stop the queues processing as soon as they see the sentinel message
        self._enqueue_sentinels()

    async def _run_cmd_queue(self) -> None:
        self._log.debug(
            f"DataCommand message queue processing starting (qsize={self.cmd_qsize()})",
        )
        try:
            while True:
                command: DataCommand | None = await self._cmd_queue.get()
                if command is self._sentinel:
                    break
                self._execute_command(command)
        except asyncio.CancelledError:
            self._log.warning("DataCommand message queue canceled")
        except Exception as e:
            self._log.exception(f"{e!r}", e)
        finally:
            stopped_msg = "DataCommand message queue stopped"
            if not self._cmd_queue.empty():
                self._log.warning(f"{stopped_msg} with {self.cmd_qsize()} message(s) on queue")
            else:
                self._log.debug(stopped_msg)

    async def _run_req_queue(self) -> None:
        self._log.debug(
            f"RequestData message queue processing starting (qsize={self.req_qsize()})",
        )
        try:
            while True:
                request: RequestData | None = await self._req_queue.get()
                if request is self._sentinel:
                    break
                self._handle_request(request)
        except asyncio.CancelledError:
            self._log.warning("RequestData message queue canceled")
        except Exception as e:
            self._log.exception(f"{e!r}", e)
        finally:
            stopped_msg = "RequestData message queue stopped"
            if not self._req_queue.empty():
                self._log.warning(f"{stopped_msg} with {self.req_qsize()} message(s) on queue")
            else:
                self._log.debug(stopped_msg)

    async def _run_res_queue(self) -> None:
        self._log.debug(
            f"DataResponse message queue processing starting (qsize={self.res_qsize()})",
        )
        try:
            while True:
                response: DataResponse | None = await self._res_queue.get()
                if response is self._sentinel:
                    break
                self._handle_response(response)
        except asyncio.CancelledError:
            self._log.warning("DataResponse message queue canceled")
        except Exception as e:
            self._log.exception(f"{e!r}", e)
        finally:
            stopped_msg = "DataResponse message queue stopped"
            if not self._res_queue.empty():
                self._log.warning(f"{stopped_msg} with {self.res_qsize()} message(s) on queue")
            else:
                self._log.debug(stopped_msg)

    async def _run_data_queue(self) -> None:
        self._log.debug(f"Data queue processing starting (qsize={self.data_qsize()})")
        try:
            while True:
                data: Data | None = await self._data_queue.get()
                if data is self._sentinel:
                    break
                self._handle_data(data)
        except asyncio.CancelledError:
            self._log.warning("Data message queue canceled")
        except Exception as e:
            self._log.exception(f"{e!r}", e)
        finally:
            stopped_msg = "Data message queue stopped"
            if not self._data_queue.empty():
                self._log.warning(f"{stopped_msg} with {self.data_qsize()} message(s) on queue")
            else:
                self._log.debug(stopped_msg)
