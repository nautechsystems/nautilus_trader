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
from ibapi.wrapper import EWrapper

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
from nautilus_trader.adapters.interactive_brokers.common import IB_VENUE
from nautilus_trader.adapters.interactive_brokers.common import IBContract
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import instrument_id_to_ib_contract
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.component import Component
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.common.logging import Logger
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId


# fmt: on


class InteractiveBrokersClient(
    Component,
    InteractiveBrokersClientConnectionMixin,
    InteractiveBrokersClientAccountMixin,
    InteractiveBrokersClientMarketDataMixin,
    InteractiveBrokersClientOrderMixin,
    InteractiveBrokersClientContractMixin,
    InteractiveBrokersClientErrorMixin,
    EWrapper,
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
        self._loop = loop
        self._cache = cache
        self._host = host
        self._port = port
        self._client_id = client_id

        self._eclient: EClient = EClient(wrapper=self)
        self._internal_msg_queue: asyncio.Queue = asyncio.Queue()

        # Tasks
        self._watch_dog_task: asyncio.Task | None = None
        self._tws_incoming_msg_reader_task: asyncio.Task | None = None
        self._internal_msg_queue_task: asyncio.Task | None = None

        # Event flags
        self._is_ready: asyncio.Event = asyncio.Event()  # Client is fully functional
        self._is_ib_ready: asyncio.Event = asyncio.Event()  # Connectivity between IB and TWS

        # Hot caches
        self.registered_nautilus_clients: set = set()
        self._event_subscriptions: dict[str, Callable] = {}
        self._bar_type_to_last_bar: dict[str, BarData | None] = {}

        # Temporary cache
        self._exec_id_details: dict[
            str,
            dict[str, Execution | (CommissionReport | str)],
        ] = {}

        # Reset
        self._reset()
        self._request_id_seq: int = 10000

        # Subscriptions
        self._requests = Requests()
        self._subscriptions = Subscriptions()

        # Overrides for EClient
        self._eclient.sendMsg = self.sendMsg
        self._eclient.logRequest = self.logRequest

        self._account_ids: set[str] = set()
        self._connection_attempt_counter: int = 0
        self._contract_for_probe: IBContract = instrument_id_to_ib_contract(
            InstrumentId.from_str("EUR/CHF.IDEALPRO"),
        )
        self._order_id_to_order_ref: dict[int, AccountOrderRef] = {}
        self._next_valid_order_id: int = -1

    def _setup_client(self) -> None:
        """
        Set up the client after a successful connection. Changes the client state to
        CONNECTED, starts the incoming message reader and queue tasks, and initiates the
        start API call to the EClient.

        Returns
        -------
        None

        """
        self._eclient.setConnState(EClient.CONNECTED)
        if self._tws_incoming_msg_reader_task:
            self._tws_incoming_msg_reader_task.cancel()
        self._tws_incoming_msg_reader_task = self._create_task(
            self._run_tws_incoming_msg_reader(),
        )
        self._internal_msg_queue_task = self._create_task(
            self._run_internal_msg_queue(),
        )
        self._eclient.startApi()

    def _start(self) -> None:
        """
        Start the client.

        Returns
        -------
        None

        """
        self._is_ready.set()

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
        tasks = [
            self._watch_dog_task,
            self._tws_incoming_msg_reader_task,
            self._internal_msg_queue_task,
        ]
        for task in tasks:
            if task:
                task.cancel()

        self._eclient.disconnect()
        self._is_ready.clear()
        self._account_ids = set()

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
        self._watch_dog_task = self._create_task(self._run_watch_dog())

    def _resume(self) -> None:
        """
        Resume the client and reset the connection attempt counter.

        Returns
        -------
        None

        """
        self._is_ready.set()
        self._connection_attempt_counter = 0

    def _degrade(self) -> None:
        """
        Degrade the client when connectivity is lost.

        Returns
        -------
        None

        """
        self._is_ready.clear()
        self._account_ids = set()

    async def _resume_client_if_degraded(self) -> None:
        """
        Handle actions when Interactive Brokers is ready and the connection is fully
        functional. If the client state is degraded, it attempts to cancel and restart
        all subscriptions.

        Returns
        -------
        None

        Raises
        ------
        Exception
            If an error occurs during the handling of subscriptions or starting the client.

        """
        if self.is_degraded:
            for subscription in self._subscriptions.get_all():
                try:
                    subscription.cancel()
                    if iscoroutinefunction(subscription.handle):
                        await subscription.handle()
                    else:
                        await self._loop.run_in_executor(None, subscription.handle)
                except Exception as e:
                    self._log.exception("Failed subscription", e)
            self._resume()

    async def _start_client_if_initialized_but_not_running(self) -> None:
        """
        Start the client if it has been initialized but is not yet running.

        Returns
        -------
        None

        """
        if self.is_initialized and not self.is_running:
            self._start()

    async def is_running_async(self, timeout: int = 300) -> None:
        """
        Check if the client is running and ready within a given timeout.

        Parameters
        ----------
        timeout : int
            Time in seconds to wait for the client to be ready. Defaults to 300 seconds.

        Returns
        -------
        None

        """
        try:
            if not self._is_ready.is_set():
                await asyncio.wait_for(self._is_ready.wait(), timeout)
        except asyncio.TimeoutError as e:
            self._log.error(f"Client is not ready. {e}")

    async def _run_watch_dog(self):
        """
        Run a watchdog to monitor and manage the health of the socket connection.
        Continuously checks the connection status, manages client state based on
        connection health, and handles subscription management in case of network
        failure or IB nightly reset.

        Returns
        -------
        None

        Raises
        ------
        asyncio.CancelledError
            If the watchdog task gets cancelled.

        """
        try:
            while True:
                await asyncio.sleep(1)
                if self._eclient.isConnected():
                    if self._is_ib_ready.is_set():
                        await self._resume_client_if_degraded()
                        await self._start_client_if_initialized_but_not_running()
                    else:
                        await self._handle_ib_is_not_ready()
                else:
                    await self._monitor_and_reconnect()
        except asyncio.CancelledError:
            self._log.debug("`watch_dog` task was canceled.")

    async def _handle_ib_is_not_ready(self) -> None:
        """
        Manage actions when Interactive Brokers is not ready or the connection is
        degraded. Performs a connectivity probe to TWS using a historical data request
        if the client is degraded. If the client is running, it handles the situation
        where connectivity between TWS/Gateway and IB server is broken.

        Returns
        -------
        None

        Raises
        ------
        Exception
            If an error occurs during the probe or handling degraded state.

        """
        if self.is_degraded:
            # Probe connectivity. Sometime restored event will not be received from TWS without this
            self._eclient.reqHistoricalData(
                reqId=1,
                contract=self._contract_for_probe,
                endDateTime="",
                durationStr="30 S",
                barSizeSetting="5 secs",
                whatToShow="MIDPOINT",
                useRTH=False,
                formatDate=2,
                keepUpToDate=False,
                chartOptions=[],
            )
            await asyncio.sleep(15)
            self._eclient.cancelHistoricalData(1)
        elif self.is_running:
            # Connectivity between TWS/Gateway and IB server is broken
            self._degrade()

    async def _monitor_and_reconnect(self) -> None:
        """
        Manage socket connectivity, including reconnection attempts and error handling.
        Degrades the client if it's currently running and tries to re-establish the
        socket connection. Waits for the Interactive Brokers readiness signal, logging
        success or failure accordingly.

        Raises
        ------
        asyncio.TimeoutError
            If the connection attempt times out.
        Exception
            For general failures in re-establishing the connection.

        """
        if self.is_running:
            self._degrade()
        self._is_ib_ready.clear()
        await asyncio.sleep(5)  # Avoid too fast attempts
        await self._establish_socket_connection()
        try:
            await asyncio.wait_for(self._is_ib_ready.wait(), 15)
            self._log.info(
                f"Connected to {self._host}:{self._port} w/ id:{self._client_id}",
            )
        except asyncio.TimeoutError:
            self._log.error(
                f"Unable to connect to {self._host}:{self._port} w/ id:{self._client_id}",
            )
        except Exception as e:
            self._log.exception("Failed connection", e)

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
        self._event_subscriptions[name] = handler

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
        Any | None
            The result of the request, or None if the request timed out.

        """
        try:
            return await asyncio.wait_for(request.future, timeout)
        except asyncio.TimeoutError as e:
            self._log.info(f"Request timed out for {request}")
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

        Returns
        -------
        None

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

        Returns
        -------
        None

        """
        self._log.debug("Client TWS incoming message reader starting...")
        buf = b""
        try:
            while self._eclient.conn and self._eclient.conn.isConnected():
                data = await self._loop.run_in_executor(None, self._eclient.conn.recvMsg)
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
            self._log.debug("Client TWS incoming message reader was canceled.")
        except Exception as e:
            self._log.exception("Unhandled exception in EReader worker", e)
        finally:
            self._log.debug("Client TWS incoming message reader stopped.")

    async def _run_internal_msg_queue(self) -> None:
        """
        Continuously process messages from the internal incoming message queue.

        Returns
        -------
        None

        """
        self._log.debug(
            "Client internal message queue starting...",
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

    # -- EWrapper overrides -----------------------------------------------------------------------

    def logAnswer(self, fnName, fnParams):
        """
        Override the logging for EWrapper.logAnswer.
        """
        if "self" in fnParams:
            prms = dict(fnParams)
            del prms["self"]
        else:
            prms = fnParams
        self._log.debug(f"Msg handled: function={fnName} data={prms}")
