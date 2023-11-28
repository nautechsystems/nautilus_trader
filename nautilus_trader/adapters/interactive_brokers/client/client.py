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
import functools
from collections.abc import Callable
from collections.abc import Coroutine
from typing import Any

from ibapi import comm
from ibapi.client import EClient
from ibapi.common import MAX_MSG_LEN
from ibapi.common import NO_VALID_ID
from ibapi.errors import BAD_LENGTH
from ibapi.utils import current_fn_name
from ibapi.wrapper import EWrapper

# fmt: off
from nautilus_trader.adapters.interactive_brokers.client.account import InteractiveBrokersAccountManager
from nautilus_trader.adapters.interactive_brokers.client.common import Request
from nautilus_trader.adapters.interactive_brokers.client.common import Requests
from nautilus_trader.adapters.interactive_brokers.client.common import Subscriptions
from nautilus_trader.adapters.interactive_brokers.client.connection import InteractiveBrokersConnectionManager
from nautilus_trader.adapters.interactive_brokers.client.error import InteractiveBrokersErrorHandler
from nautilus_trader.adapters.interactive_brokers.client.market_data import InteractiveBrokersMarketDataManager
from nautilus_trader.adapters.interactive_brokers.client.order import InteractiveBrokersOrderManager
from nautilus_trader.adapters.interactive_brokers.common import IB_VENUE
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.component import Component
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.common.logging import Logger
from nautilus_trader.model.identifiers import ClientId


# fmt: on


class InteractiveBrokersClient(Component, EWrapper):
    """
    A client component that interfaces with the Interactive Brokers TWS or Gateway.

    This class integrates various managers and handlers to provide functionality for
    connection management, account management, market data, and order processing with
    Interactive Brokers. It inherits from both `Component` and `EWrapper` to provide
    event-driven responses and custom component behavior.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        logger: Logger,
        host: str = "127.0.0.1",
        port: int = 7497,
        client_id: int = 1,
    ):
        super().__init__(
            clock=clock,
            logger=logger,
            component_id=ClientId(f"{IB_VENUE.value}-{client_id:03d}"),
            component_name=f"{type(self).__name__}-{client_id:03d}",
            msgbus=msgbus,
            config={"name": f"{type(self).__name__}-{client_id:03d}", "client_id": client_id},
        )
        # Config
        self.loop = loop
        self.cache = cache
        self.clock = self._clock
        self.log = self._log
        self.msgbus = self._msgbus
        self.host = host
        self.port = port
        self.client_id = client_id

        self._eclient: EClient = EClient(wrapper=self)
        self._internal_msg_queue: asyncio.Queue = asyncio.Queue()
        self._connection_attempt_counter = 0

        # Managers and handlers to handle different aspects of the client
        self.connection_manager = InteractiveBrokersConnectionManager(self)
        self.account_manager = InteractiveBrokersAccountManager(self)
        self.market_data_manager = InteractiveBrokersMarketDataManager(self)
        self.order_manager = InteractiveBrokersOrderManager(self)
        self._error_handler = InteractiveBrokersErrorHandler(self)

        # Tasks
        self._watch_dog_task: asyncio.Task | None = None
        self.tws_incoming_msg_reader_task: asyncio.Task | None = None
        self.internal_msg_queue_task: asyncio.Task | None = None

        # Event flags
        self.is_ready: asyncio.Event = asyncio.Event()  # Client is fully functional
        self.is_ib_ready: asyncio.Event = asyncio.Event()  # Connectivity between IB and TWS

        # Hot caches
        self.registered_nautilus_clients: set = set()
        self.event_subscriptions: dict[str, Callable] = {}

        # Reset
        self._reset()
        self._request_id_seq = 10000

        # Subscriptions
        self.requests = Requests()
        self.subscriptions = Subscriptions()

        # Overrides for EClient
        self._eclient.sendMsg = self.sendMsg
        self._eclient.logRequest = self.logRequest

    def create_task(
        self,
        coro: Coroutine,
        log_msg: str | None = None,
        actions: Callable | None = None,
        success: str | None = None,
    ) -> asyncio.Task:
        """
        Create an asyncio task with error handling and optional callback actions.

        Parameters
        ----------
        coro : Coroutine
            The coroutine to run.
        log_msg : str, optional
            The log message for the task.
        actions : Callable, optional
            The actions callback to run when the coroutine is done.
        success : str, optional
            The log message to write on actions success.

        Returns
        -------
        asyncio.Task

        """
        log_msg = log_msg or coro.__name__
        self._log.debug(f"Creating task {log_msg}.")
        task = self.loop.create_task(
            coro,
            name=coro.__name__,
        )
        task.add_done_callback(
            functools.partial(
                self._on_task_completed,
                actions,
                success,
            ),
        )
        return task

    def subscribe_event(self, name: str, handler: Callable) -> None:
        """
        Subscribe a handler function to a named event.

        Parameters
        ----------
        name : str
            The name of the event to subscribe to.
        handler : Callable
            The handler function to be called when the event occurs.

        Returns
        -------
        None

        """
        self.event_subscriptions[name] = handler

    def unsubscribe_event(self, name: str) -> None:
        """
        Unsubscribe a handler from a named event.

        Parameters
        ----------
        name : str
            The name of the event to unsubscribe from.

        Returns
        -------
        None

        """
        self.event_subscriptions.pop(name)

    async def is_running_async(self, timeout: int = 300) -> None:
        """
        Check if the client is running and ready within a given timeout.

        Parameters
        ----------
        timeout : int, optional
            Time in seconds to wait for the client to be ready. Defaults to 300 seconds.

        Returns
        -------
        None

        """
        try:
            if not self.is_ready.is_set():
                await asyncio.wait_for(self.is_ready.wait(), timeout)
        except asyncio.TimeoutError as e:
            self._log.error(f"Client is not ready. {e}")

    async def await_request(self, request: Request, timeout: int) -> Any | None:
        """
        Await the completion of a request within a specified timeout.

        Parameters
        ----------
        request : Request
            The request object to await.
        timeout : int
            The maximum time to wait for the request to complete, in seconds.

        Returns
        -------
        Any | None
            The result of the request, or None if the request timed out.

        """
        try:
            return await asyncio.wait_for(request.future, timeout)
        except asyncio.TimeoutError as e:
            self._log.info(f"Request timed out for {request}")
            self._end_request(request.req_id, success=False, exception=e)
            return None

    def end_request(
        self,
        req_id: int,
        success: bool = True,
        exception: asyncio.TimeoutError | None = None,
    ) -> None:
        """
        End a request with a specified result or exception.

        Parameters
        ----------
        req_id : int
            The request ID to conclude.
        success : bool, optional
            Whether the request was successful. Defaults to True.
        exception : asyncio.TimeoutError | None, optional
            An exception to set on request failure. Defaults to None.

        Returns
        -------
        None

        """
        if not (request := self.requests.get(req_id=req_id)):
            return

        if not request.future.done():
            if success:
                request.future.set_result(request.result)
            else:
                request.cancel()
                request.future.set_exception(exception)
        self.requests.remove(req_id=req_id)

    async def run_tws_incoming_msg_reader(self) -> None:
        """
        Continuously read messages from TWS/Gateway and then put them in the internal
        message queue for processing.

        Returns
        -------
        None

        """
        self._log.debug("TWS incoming message reader starting...")
        buf = b""
        try:
            while self._eclient.conn and self._eclient.conn.isConnected():
                data = await self.loop.run_in_executor(None, self._eclient.conn.recvMsg)
                buf += data
                while buf:
                    size, msg, buf = comm.read_msg(buf)
                    self._log.debug(f"TWS incoming message reader received msg={buf!s}")
                    if msg:
                        # Place msg in the internal queue for processing
                        self._internal_msg_queue.put_nowait(msg)
                    else:
                        self._log.debug("More incoming packets are needed.")
                        break
        except asyncio.CancelledError:
            self._log.debug("TWS incoming message reader was canceled.")
        except Exception as e:
            self._log.exception("Unhandled exception in EReader worker", e)
        finally:
            self._log.debug("TWS incoming message reader stopped.")

    async def run_internal_msg_queue(self) -> None:
        """
        Continuously process messages from the internal incoming message queue.

        Returns
        -------
        None

        """
        self._log.debug(
            "Internal message queue starting...",
        )
        try:
            while (
                self._eclient.conn
                and self._eclient.conn.isConnected()
                or not self._internal_msg_queue.empty()
            ):
                msg = await self._internal_msg_queue.get()
                if not self._process_message(msg):
                    break
                self._internal_msg_queue.task_done()
        except asyncio.CancelledError:
            log_msg = f"Internal message queue processing stopped. (qsize={self._internal_msg_queue.qsize()})."
            self._log.warning(log_msg) if not self._internal_msg_queue.empty() else self._log.debug(
                log_msg,
            )
        finally:
            self._eclient.disconnect()

    def _on_task_completed(
        self,
        actions: Callable | None,
        success: str | None,
        task: asyncio.Task,
    ) -> None:
        """
        Handle the completion of a task.

        Parameters
        ----------
        actions : Callable, optional
            Callback actions to execute upon task completion.
        success : str, optional
            Success log message to display on successful completion of actions.
        task : asyncio.Task
            The asyncio Task that has been completed.

        Returns
        -------
        None

        """
        if task.exception():
            self._log.error(
                f"Error on `{task.get_name()}`: " f"{task.exception()!r}",
            )
        else:
            if actions:
                try:
                    actions()
                except Exception as e:
                    self._log.error(
                        f"Failed triggering action {actions.__name__} on `{task.get_name()}`: "
                        f"{e!r}",
                    )
            if success:
                self._log.info(success, LogColor.GREEN)

    def _next_req_id(self) -> int:
        """
        Generate the next sequential request ID.

        Returns
        -------
        int

        """
        new_id = self._request_id_seq
        self._request_id_seq += 1
        return new_id

    def _start(self) -> None:
        """
        Start the client.

        Returns
        -------
        None

        """
        self.is_ready.set()

    def _stop(self) -> None:
        """
        Stop the client and cancel running tasks.

        Returns
        -------
        None

        """
        if self.registered_nautilus_clients != set():
            self._log.warning(
                f"Any registered Clients from {self.registered_nautilus_clients} will disconnect.",
            )

        # Cancel tasks
        if self._watch_dog_task:
            self._log.debug("Stopping the watch dog...")
            self._watch_dog_task.cancel()
        if self.tws_incoming_msg_reader_task:
            self._log.debug("Stopping the TWS incoming message reader...")
            self.tws_incoming_msg_reader_task.cancel()
        if self.internal_msg_queue_task:
            self._log.debug("Stopping the internal message queue...")
            self.internal_msg_queue_task.cancel()

        self._eclient.disconnect()
        self.is_ready.clear()
        self.account_manager.account_ids = set()

    def _reset(self) -> None:
        """
        Reset the client state and restart connection watchdog.

        Returns
        -------
        None

        """
        self._stop()
        self._eclient.reset()

        # Start the Watchdog
        self._watch_dog_task = self.create_task(self.connection_manager.run_watch_dog())

    def _resume(self) -> None:
        """
        Resume the client and reset the connection attempt counter.

        Returns
        -------
        None

        """
        self.is_ready.set()
        self._connection_attempt_counter = 0

    def _degrade(self) -> None:
        """
        Degrade the client when connectivity is lost.

        Returns
        -------
        None

        """
        self.is_ready.clear()
        self.account_manager.account_ids = set()

    def _process_message(self, msg: str) -> bool:
        """
        Process a single message from TWS/Gateway.

        Parameters
        ----------
        msg : str
            The message to be processed.

        Returns
        -------
        bool

        """
        if len(msg) > MAX_MSG_LEN:
            self._eclient.wrapper.error(
                NO_VALID_ID,
                BAD_LENGTH.code(),
                f"{BAD_LENGTH.msg()}:{len(msg)}:{msg}",
            )
            return False
        fields: tuple[bytes] = comm.read_fields(msg)
        self._log.debug(f"Incoming message fields: {fields}")

        # The decoder identifies the message type based on its payload (e.g., open
        # order, process real-time ticks, etc.) and then calls the corresponding
        # method from the EWrapper. Many of those methods are overridden in the client
        # manager and handler classes to support custom processing required for Nautilus.
        self._eclient.decoder.interpret(fields)
        return True

    # -- EWrapper overrides -----------------------------------------------------------------------
    def sendMsg(self, msg):
        """
        Override the logging for ibapi EClient.sendMsg.
        """
        full_msg = comm.make_msg(msg)
        self._log.debug(f"TWS API Sending: function={current_fn_name(1)} msg={full_msg}")
        self._eclient.conn.sendMsg(full_msg)

    def logRequest(self, fnName, fnParams):
        """
        Override the logging for ibapi EClient.logRequest.
        """
        if "self" in fnParams:
            prms = dict(fnParams)
            del prms["self"]
        else:
            prms = fnParams
        self._log.debug(f"TWS API Request: function={fnName} data={prms}")

    def logAnswer(self, fnName, fnParams):
        """
        Override the logging for EWrapper.logAnswer.
        """
        if "self" in fnParams:
            prms = dict(fnParams)
            del prms["self"]
        else:
            prms = fnParams
        self._log.debug(f"TWS API Response: function={fnName} data={prms}")
