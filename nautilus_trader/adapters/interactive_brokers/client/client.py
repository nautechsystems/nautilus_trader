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
from decimal import Decimal
from inspect import iscoroutinefunction
from typing import Any

from ibapi import comm
from ibapi.client import EClient
from ibapi.commission_report import CommissionReport
from ibapi.common import MAX_MSG_LEN
from ibapi.common import NO_VALID_ID
from ibapi.common import BarData
from ibapi.common import SetOfFloat
from ibapi.common import SetOfString
from ibapi.common import TickAttribBidAsk
from ibapi.common import TickAttribLast
from ibapi.contract import ContractDetails
from ibapi.errors import BAD_LENGTH
from ibapi.execution import Execution
from ibapi.order import Order as IBOrder
from ibapi.order_state import OrderState as IBOrderState
from ibapi.utils import current_fn_name
from ibapi.wrapper import EWrapper

# fmt: off
from nautilus_trader.adapters.interactive_brokers.client.account import InteractiveBrokersAccountManager
from nautilus_trader.adapters.interactive_brokers.client.common import Request
from nautilus_trader.adapters.interactive_brokers.client.common import Requests
from nautilus_trader.adapters.interactive_brokers.client.common import Subscriptions
from nautilus_trader.adapters.interactive_brokers.client.connection import InteractiveBrokersConnectionManager
from nautilus_trader.adapters.interactive_brokers.client.contract import InteractiveBrokersContractManager
from nautilus_trader.adapters.interactive_brokers.client.error import InteractiveBrokersErrorHandler
from nautilus_trader.adapters.interactive_brokers.client.market_data import InteractiveBrokersMarketDataManager
from nautilus_trader.adapters.interactive_brokers.client.order import InteractiveBrokersOrderManager
from nautilus_trader.adapters.interactive_brokers.common import IB_VENUE
from nautilus_trader.adapters.interactive_brokers.common import IBContract
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
        self._connection_manager = InteractiveBrokersConnectionManager(self)
        self.account_manager = InteractiveBrokersAccountManager(self)
        self.market_data_manager = InteractiveBrokersMarketDataManager(self)
        self.order_manager = InteractiveBrokersOrderManager(self)
        self.contract_manager = InteractiveBrokersContractManager(self)
        self._error_handler = InteractiveBrokersErrorHandler(self)

        # Tasks
        self._watch_dog_task: asyncio.Task | None = None
        self._tws_incoming_msg_reader_task: asyncio.Task | None = None
        self._internal_msg_queue_task: asyncio.Task | None = None

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

    def setup_client(self) -> None:
        """
        Set up the client after a successful connection. Changes the client state to
        CONNECTED, starts the incoming message reader and queue tasks, and initiates the
        start API call to the EClient.

        Returns
        -------
        None

        """
        self._eclient.setConnState(EClient.CONNECTED)
        if self._client._tws_incoming_msg_reader_task:
            self._client._tws_incoming_msg_reader_task.cancel()
        self._client._tws_incoming_msg_reader_task = self._client.create_task(
            self._client._run_tws_incoming_msg_reader(),
        )
        self._client._internal_msg_queue_task = self._client.create_task(
            self._client._run_internal_msg_queue(),
        )
        self._eclient.startApi()

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
        if self._tws_incoming_msg_reader_task:
            self._log.debug("Stopping the TWS incoming message reader...")
            self._tws_incoming_msg_reader_task.cancel()
        if self._internal_msg_queue_task:
            self._log.debug("Stopping the internal message queue...")
            self._internal_msg_queue_task.cancel()

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
        self._watch_dog_task = self.create_task(self._run_watch_dog())

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
        if self._client.is_degraded:
            for subscription in self._client.subscriptions.get_all():
                try:
                    subscription.cancel()
                    if iscoroutinefunction(subscription.handle):
                        await subscription.handle()
                    else:
                        await self._client.loop.run_in_executor(None, subscription.handle)
                except Exception as e:
                    self._log.exception("Failed subscription", e)
            self._client._resume()

    async def _start_client_if_initialized_but_not_running(self) -> None:
        """
        Start the client if it has been initialized but is not yet running.

        Returns
        -------
        None

        """
        if self._client.is_initialized and not self._client.is_running:
            self._client.start()

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
        timeout : int
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
            self.end_request(request.req_id, success=False, exception=e)
            return None

    def end_request(
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
        if not (request := self.requests.get(req_id=req_id)):
            return

        if not request.future.done():
            if success:
                request.future.set_result(request.result)
            else:
                request.cancel()
                if exception:
                    request.future.set_exception(exception)
        self.requests.remove(req_id=req_id)

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
                data = await self.loop.run_in_executor(None, self._eclient.conn.recvMsg)
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

    def next_req_id(self) -> int:
        """
        Generate the next sequential request ID.

        Returns
        -------
        int

        """
        new_id = self._request_id_seq
        self._request_id_seq += 1
        return new_id

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
                    if self._client.is_ib_ready.is_set():
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
        if self._client.is_degraded:
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
        elif self._client.is_running:
            # Connectivity between TWS/Gateway and IB server is broken
            self._client._degrade()

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
        if self._client.is_running:
            self._client._degrade()
        self._client.is_ib_ready.clear()
        await asyncio.sleep(5)  # Avoid too fast attempts
        await self._connection_manager.establish_socket_connection()
        try:
            await asyncio.wait_for(self._client.is_ib_ready.wait(), 15)
            self._log.info(
                f"Connected to {self.host}:{self.port} w/ id:{self._client.client_id}",
            )
        except asyncio.TimeoutError:
            self._log.error(
                f"Unable to connect to {self.host}:{self.port} w/ id:{self._client.client_id}",
            )
        except Exception as e:
            self._log.exception("Failed connection", e)

    # -- EClient overrides ------------------------------------------------------------------------

    # -- InteractiveBrokersClient -----------------------------------------------------------------
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

    # -- InteractiveBrokersConnectionManager -----------------------------------------------------
    def connectionClosed(self) -> None:
        self._connection_manager.connectionClosed()

    # -- InteractiveBrokersErrorHandler -----------------------------------------------------------
    def error(
        self,
        req_id: int,
        error_code: int,
        error_string: str,
        advanced_order_reject_json: str = "",
    ) -> None:
        self._error_handler.error(req_id, error_code, error_string, advanced_order_reject_json)

    # -- InteractiveBrokersAccountManager ---------------------------------------------------------
    def accountSummary(
        self,
        req_id: int,
        account_id: str,
        tag: str,
        value: str,
        currency: str,
    ) -> None:
        self.account_manager.accountSummary(
            req_id,
            account_id,
            tag,
            value,
            currency,
        )

    def managedAccounts(self, accounts_list: str) -> None:
        self.account_manager.managedAccounts(accounts_list)

    def positionEnd(self) -> None:
        self.account_manager.positionEnd()

    # -- InteractiveBrokersContractManager --------------------------------------------------------
    def contractDetails(
        self,
        req_id: int,
        contract_details: ContractDetails,
    ) -> None:
        self.contract_manager.contractDetails(req_id, contract_details)

    def contractDetailsEnd(self, req_id: int) -> None:
        self.contract_manager.contractDetailsEnd(req_id)

    def securityDefinitionOptionParameter(
        self,
        req_id: int,
        exchange: str,
        underlying_con_id: int,
        trading_class: str,
        multiplier: str,
        expirations: SetOfString,
        strikes: SetOfFloat,
    ) -> None:
        self.contract_manager.securityDefinitionOptionParameter(
            req_id,
            exchange,
            underlying_con_id,
            trading_class,
            multiplier,
            expirations,
            strikes,
        )

    def securityDefinitionOptionParameterEnd(self, req_id: int) -> None:
        self.contract_manager.securityDefinitionOptionParameterEnd(req_id)

    def symbolSamples(self, req_id: int, contract_descriptions: list) -> None:
        self.contract_manager.symbolSamples(req_id, contract_descriptions)

    # -- InteractiveBrokersMarketDataManager ------------------------------------------------------
    def marketDataType(self, req_id: int, market_data_type: int) -> None:
        self.market_data_manager.marketDataType(req_id, market_data_type)

    def tickByTickBidAsk(
        self,
        req_id: int,
        time: int,
        bid_price: float,
        ask_price: float,
        bid_size: Decimal,
        ask_size: Decimal,
        tick_attrib_bid_ask: TickAttribBidAsk,
    ) -> None:
        self.market_data_manager.tickByTickBidAsk(
            req_id,
            time,
            bid_price,
            ask_price,
            bid_size,
            ask_size,
            tick_attrib_bid_ask,
        )

    def tickByTickAllLast(
        self,
        req_id: int,
        tick_type: int,
        time: int,
        price: float,
        size: Decimal,
        tick_attrib_last: TickAttribLast,
        exchange: str,
        special_conditions: str,
    ) -> None:
        self.market_data_manager.tickByTickAllLast(
            req_id,
            tick_type,
            time,
            price,
            size,
            tick_attrib_last,
            exchange,
            special_conditions,
        )

    def realtimeBar(
        self,
        req_id: int,
        time: int,
        open_: float,
        high: float,
        low: float,
        close: float,
        volume: Decimal,
        wap: Decimal,
        count: int,
    ) -> None:
        self.market_data_manager.realtimeBar(
            req_id,
            time,
            open_,
            high,
            low,
            close,
            volume,
            wap,
            count,
        )

    def historicalData(self, req_id: int, bar: BarData) -> None:
        self.market_data_manager.historicalData(req_id, bar)

    def historicalDataEnd(self, req_id: int, start: str, end: str) -> None:
        self.market_data_manager.historicalDataEnd(req_id, start, end)

    def historicalDataUpdate(self, req_id: int, bar: BarData) -> None:
        self.market_data_manager.historicalDataUpdate(req_id, bar)

    def historicalTicksBidAsk(
        self,
        req_id: int,
        ticks: list,
        done: bool,
    ) -> None:
        self.market_data_manager.historicalTicksBidAsk(req_id, ticks, done)

    def historicalTicksLast(self, req_id: int, ticks: list, done: bool) -> None:
        self.market_data_manager.historicalTicksLast(req_id, ticks, done)

    def historicalTicks(self, req_id: int, ticks: list, done: bool) -> None:
        self.market_data_manager.historicalTicks(req_id, ticks, done)

    # -- InteractiveBrokersOrderManager -----------------------------------------------------------
    def nextValidId(self, order_id: int) -> None:
        self.order_manager.nextValidId(order_id)

    def openOrder(
        self,
        order_id: int,
        contract: IBContract,
        order: IBOrder,
        order_state: IBOrderState,
    ) -> None:
        self.order_manager.openOrder(order_id, contract, order, order_state)

    def openOrderEnd(self) -> None:
        self.order_manager.openOrderEnd()

    def orderStatus(
        self,
        order_id: int,
        status: str,
        filled: Decimal,
        remaining: Decimal,
        avg_fill_price: float,
        perm_id: int,
        parent_id: int,
        last_fill_price: float,
        client_id: int,
        why_held: str,
        mkt_cap_price: float,
    ) -> None:
        self.order_manager.orderStatus(
            order_id,
            status,
            filled,
            remaining,
            avg_fill_price,
            perm_id,
            parent_id,
            last_fill_price,
            client_id,
            why_held,
            mkt_cap_price,
        )

    def execDetails(self, req_id: int, contract: IBContract, execution: Execution) -> None:
        self.order_manager.execDetails(req_id, contract, execution)

    def commissionReport(self, commission_report: CommissionReport) -> None:
        self.order_manager.commissionReport(commission_report)
