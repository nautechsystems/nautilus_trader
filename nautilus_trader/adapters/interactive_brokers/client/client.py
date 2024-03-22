# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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
import os
from collections.abc import Callable
from collections.abc import Coroutine
from inspect import iscoroutinefunction
from typing import Any

from ibapi import comm
from ibapi.client import EClient
from ibapi.commission_report import CommissionReport
from ibapi.common import MAX_MSG_LEN
from ibapi.common import NO_VALID_ID
from ibapi.common import BarData
from ibapi.errors import BAD_LENGTH
from ibapi.execution import Execution
from ibapi.utils import current_fn_name

# fmt: off
from nautilus_trader.adapters.interactive_brokers.client.account import InteractiveBrokersClientAccountMixin
from nautilus_trader.adapters.interactive_brokers.client.common import AccountOrderRef
from nautilus_trader.adapters.interactive_brokers.client.common import Request
from nautilus_trader.adapters.interactive_brokers.client.common import Requests
from nautilus_trader.adapters.interactive_brokers.client.common import Subscriptions
from nautilus_trader.adapters.interactive_brokers.client.connection import InteractiveBrokersClientConnectionMixin
from nautilus_trader.adapters.interactive_brokers.client.contract import InteractiveBrokersClientContractMixin
from nautilus_trader.adapters.interactive_brokers.client.error import InteractiveBrokersClientErrorMixin
from nautilus_trader.adapters.interactive_brokers.client.market_data import InteractiveBrokersClientMarketDataMixin
from nautilus_trader.adapters.interactive_brokers.client.order import InteractiveBrokersClientOrderMixin
from nautilus_trader.adapters.interactive_brokers.client.wrapper import InteractiveBrokersEWrapper
from nautilus_trader.adapters.interactive_brokers.common import IB_VENUE
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import Component
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.model.identifiers import ClientId


# fmt: on


class InteractiveBrokersClient(
    Component,
    InteractiveBrokersClientConnectionMixin,
    InteractiveBrokersClientAccountMixin,
    InteractiveBrokersClientMarketDataMixin,
    InteractiveBrokersClientOrderMixin,
    InteractiveBrokersClientContractMixin,
    InteractiveBrokersClientErrorMixin,
):
    """
    A client component that interfaces with the Interactive Brokers TWS or Gateway.

    This class integrates various mixins to provide functionality for connection
    management, account management, market data, and order processing with
    Interactive Brokers. It inherits from both `Component` and `EWrapper` to provide
    event-driven responses and custom component behavior.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        host: str = "127.0.0.1",
        port: int = 7497,
        client_id: int = 1,
    ) -> None:
        super().__init__(
            clock=clock,
            component_id=ClientId(f"{IB_VENUE.value}-{client_id:03d}"),
            component_name=f"{type(self).__name__}-{client_id:03d}",
            msgbus=msgbus,
        )

        # Config
        self._loop = loop
        self._cache = cache
        self._host = host
        self._port = port
        self._client_id = client_id

        # TWS API
        self._eclient: EClient = EClient(
            wrapper=InteractiveBrokersEWrapper(
                nautilus_logger=self._log,
                client=self,
            ),
        )

        # EClient Overrides
        self._eclient.sendMsg = self.sendMsg
        self._eclient.logRequest = self.logRequest

        # Tasks
        self._connection_watchdog_task: asyncio.Task | None = None
        self._tws_incoming_msg_reader_task: asyncio.Task | None = None
        self._internal_msg_queue_processor_task: asyncio.Task | None = None
        self._internal_msg_queue: asyncio.Queue = asyncio.Queue()

        # Event flags
        self._is_client_ready: asyncio.Event = asyncio.Event()
        self._is_ib_connected: asyncio.Event = asyncio.Event()

        # Hot caches
        self.registered_nautilus_clients: set = set()
        self._event_subscriptions: dict[str, Callable] = {}

        # Subscriptions
        self._requests = Requests()
        self._subscriptions = Subscriptions()

        # AccountMixin
        self._account_ids: set[str] = set()

        # ConnectionMixin
        self._reconnect_attempts: int = 0
        self._max_reconnect_attempts: int = int(os.getenv("IB_MAX_RECONNECT_ATTEMPTS", 0))
        self._indefinite_reconnect: bool = False if self._max_reconnect_attempts else True
        self._reconnect_delay: int = 5  # seconds

        # MarketDataMixin
        self._bar_type_to_last_bar: dict[str, BarData | None] = {}

        # OrderMixin
        self._exec_id_details: dict[
            str,
            dict[str, Execution | (CommissionReport | str)],
        ] = {}
        self._order_id_to_order_ref: dict[int, AccountOrderRef] = {}
        self._next_valid_order_id: int = -1

        # Start client
        self._request_id_seq: int = 10000

    def _start(self) -> None:
        """
        Start the client.

        This method is called when the client is first initialized and when the client
        is reset. It sets up the client and starts the connection watchdog, incoming
        message reader, and internal message queue processing tasks.

        """
        if not self._loop.is_running():
            self._log.warning("Started when loop is not running.")

        self._log.info(f"Starting InteractiveBrokersClient ({self._client_id})...")
        self._loop.run_until_complete(self._startup())
        self._is_client_ready.set()

    async def _startup(self):
        try:
            self._log.info(f"Starting InteractiveBrokersClient ({self._client_id})...")
            await self._connect()
            self._start_tws_incoming_msg_reader()
            self._start_internal_msg_queue_processor()
            self._eclient.startApi()
            # TWS/Gateway will send a managedAccounts message upon successful connection,
            # which will set the `_is_ib_connected` event. This typically takes a few
            # seconds, so we wait for it here.
            await asyncio.wait_for(self._is_ib_connected.wait(), 15)
            self._start_connection_watchdog()
            self._is_client_ready.set()
        except asyncio.TimeoutError:
            self._log.error("Client failed to initialize. Connection timeout.")
            self._stop()
        except Exception as e:
            self._log.exception("Unhandled exception in client startup", e)
            self._stop()

    def _start_tws_incoming_msg_reader(self) -> None:
        """
        Start the incoming message reader task.
        """
        if self._tws_incoming_msg_reader_task:
            self._tws_incoming_msg_reader_task.cancel()
        self._tws_incoming_msg_reader_task = self._create_task(
            self._run_tws_incoming_msg_reader(),
        )

    def _start_internal_msg_queue_processor(self) -> None:
        """
        Start the internal message queue processing task.
        """
        if self._internal_msg_queue_processor_task:
            self._internal_msg_queue_processor_task.cancel()
        self._internal_msg_queue_processor_task = self._create_task(
            self._run_internal_msg_queue_processor(),
        )

    def _start_connection_watchdog(self) -> None:
        """
        Start the connection watchdog task.
        """
        if self._connection_watchdog_task:
            self._connection_watchdog_task.cancel()
        self._connection_watchdog_task = self._create_task(
            self._run_connection_watchdog(),
        )

    def _stop(self) -> None:
        """
        Stop the client and cancel running tasks.
        """
        self._log.info(f"Stopping InteractiveBrokersClient ({self._client_id})...")
        # Cancel tasks
        tasks = [
            self._connection_watchdog_task,
            self._tws_incoming_msg_reader_task,
            self._internal_msg_queue_processor_task,
        ]
        for task in tasks:
            if task and not task.cancelled():
                task.cancel()

        self._eclient.disconnect()
        self._is_client_ready.clear()
        self._account_ids = set()
        for client in self.registered_nautilus_clients:
            self._log.warning(f"Client {client} disconnected.")
        self.registered_nautilus_clients = set()

    def _reset(self) -> None:
        """
        Restart the client.
        """
        self._log.info(f"Resetting InteractiveBrokersClient ({self._client_id})...")
        self._stop()
        self._start()

    def _resume(self) -> None:
        """
        Resume the client and resubscribe to all subscriptions.
        """
        self._log.info(f"Resuming InteractiveBrokersClient ({self._client_id})...")
        self._is_client_ready.set()

    def _degrade(self) -> None:
        """
        Degrade the client when connectivity is lost.
        """
        self._log.info(f"Degrading InteractiveBrokersClient ({self._client_id})...")
        self._is_client_ready.clear()
        self._account_ids = set()

    async def _resubscribe_all(self) -> None:
        """
        Cancel and restart all subscriptions.
        """
        self._log.debug("Resubscribing all subscriptions...")
        for subscription in self._subscriptions.get_all():
            try:
                subscription.cancel()
                if iscoroutinefunction(subscription.handle):
                    await subscription.handle()
                else:
                    await asyncio.to_thread(subscription.handle)
            except Exception as e:
                self._log.exception(f"Failed to resubscribe to {subscription}", e)

    async def wait_until_ready(self, timeout: int = 300) -> None:
        """
        Check if the client is running and ready within a given timeout.

        Parameters
        ----------
        timeout : int, default 300
            Time in seconds to wait for the client to be ready.

        """
        try:
            if not self._is_client_ready.is_set():
                await asyncio.wait_for(self._is_client_ready.wait(), timeout)
        except asyncio.TimeoutError as e:
            self._log.error(f"Client is not ready. {e}")

    async def _run_connection_watchdog(self) -> None:
        """
        Run a watchdog to monitor and manage the health of the socket connection.

        Continuously checks the connection status, manages client state based on
        connection health, and handles subscription management in case of network
        failure or forced IB connection reset.

        """
        try:
            while True:
                await asyncio.sleep(1)
                if not self._is_ib_connected.is_set() or not self._eclient.isConnected():
                    self._log.error(
                        "Connection watchdog detects connection lost.",
                    )
                    await self._handle_disconnection()
        except asyncio.CancelledError:
            self._log.debug("Client connection watchdog task was canceled.")

    async def _handle_disconnection(self) -> None:
        """
        Handle the disconnection of the client from TWS/Gateway.
        """
        if self.is_running:
            self._degrade()
        if not self._is_ib_connected.is_set():
            self._log.debug("`_is_ib_connected` unset by `_handle_disconnection`.", LogColor.BLUE)
            self._is_ib_connected.clear()
        await asyncio.sleep(5)
        await self._handle_reconnect()

        self._resume()

    def _create_task(
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
        task = self._loop.create_task(
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

    def subscribe_event(self, name: str, handler: Callable) -> None:
        """
        Subscribe a handler function to a named event.

        Parameters
        ----------
        name : str
            The name of the event to subscribe to.
        handler : Callable
            The handler function to be called when the event occurs.

        """
        self._event_subscriptions[name] = handler

    def unsubscribe_event(self, name: str) -> None:
        """
        Unsubscribe a handler from a named event.

        Parameters
        ----------
        name : str
            The name of the event to unsubscribe from.

        """
        self._event_subscriptions.pop(name)

    async def _await_request(self, request: Request, timeout: int) -> Any | None:
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
        Any | ``None``
            The result of the request, or None if the request timed out.

        """
        try:
            return await asyncio.wait_for(request.future, timeout)
        except asyncio.TimeoutError as e:
            self._log.warning(f"Request timed out for {request}. Ending request.")
            self._end_request(request.req_id, success=False, exception=e)
            return None
        except ConnectionError as e:
            self._log.error(f"Connection error during {request}. Ending request.")
            self._end_request(request.req_id, success=False, exception=e)
            return None

    def _end_request(
        self,
        req_id: int,
        success: bool = True,
        exception: type | BaseException | None = None,
    ) -> None:
        """
        End a request with a specified result or exception.

        Parameters
        ----------
        req_id : int
            The request ID to conclude.
        success : bool, optional
            Whether the request was successful. Defaults to True.
        exception : type | BaseException | None, optional
            An exception to set on request failure. Defaults to None.

        """
        if not (request := self._requests.get(req_id=req_id)):
            return

        if not request.future.done():
            if success:
                request.future.set_result(request.result)
            else:
                request.cancel()
                if exception:
                    request.future.set_exception(exception)
        self._requests.remove(req_id=req_id)

    async def _run_tws_incoming_msg_reader(self) -> None:
        """
        Continuously read messages from TWS/Gateway and then put them in the internal
        message queue for processing.
        """
        self._log.debug("Client TWS incoming message reader started.")
        buf = b""
        try:
            while self._eclient.conn and self._eclient.conn.isConnected():
                data = await asyncio.to_thread(self._eclient.conn.recvMsg)
                buf += data
                while buf:
                    _, msg, buf = comm.read_msg(buf)
                    self._log.debug(f"Msg buffer received: {buf!s}")
                    if msg:
                        # Place msg in the internal queue for processing
                        self._internal_msg_queue.put_nowait(msg)
                    else:
                        self._log.debug("More incoming packets are needed.")
                        break
        except asyncio.CancelledError:
            self._log.debug("Client TWS incoming message reader was cancelled.")
        except Exception as e:
            self._log.exception("Unhandled exception in Client TWS incoming message reader", e)
        finally:
            if self._is_ib_connected.is_set() and not self.is_disposed:
                self._log.debug(
                    "`_is_ib_connected` unset by `_run_tws_incoming_msg_reader`.",
                    LogColor.BLUE,
                )
                self._is_ib_connected.clear()
            self._log.debug("Client TWS incoming message reader stopped.")

    async def _run_internal_msg_queue_processor(self) -> None:
        """
        Continuously process messages from the internal incoming message queue.
        """
        self._log.debug(
            "Client internal message queue processor started.",
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
            (
                self._log.warning(log_msg)
                if not self._internal_msg_queue.empty()
                else self._log.debug(
                    log_msg,
                )
            )
        finally:
            self._log.debug("Internal message queue processor stopped.")

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
        self._log.debug(f"Msg received: {msg}")
        self._log.debug(f"Msg received fields: {fields}")

        # The decoder identifies the message type based on its payload (e.g., open
        # order, process real-time ticks, etc.) and then calls the corresponding
        # method from the EWrapper. Many of those methods are overridden in the client
        # manager and handler classes to support custom processing required for Nautilus.
        self._eclient.decoder.interpret(fields)
        return True

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

    # -- EClient overrides ------------------------------------------------------------------------

    def sendMsg(self, msg):
        """
        Override the logging for ibapi EClient.sendMsg.
        """
        full_msg = comm.make_msg(msg)
        self._log.debug(f"TWS API request sent: function={current_fn_name(1)} msg={full_msg}")
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
        self._log.debug(f"TWS API prepared request: function={fnName} data={prms}")
