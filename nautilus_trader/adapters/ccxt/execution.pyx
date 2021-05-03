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
from decimal import Decimal

from libc.stdint cimport int64_t

import ccxt
from ccxt.base.errors import BaseError as CCXTError

from cpython.datetime cimport datetime

from nautilus_trader.adapters.ccxt.providers cimport CCXTInstrumentProvider
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logging cimport LogColor
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.providers cimport InstrumentProvider
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport dt_to_unix_millis
from nautilus_trader.core.datetime cimport millis_to_nanos
from nautilus_trader.execution.messages cimport ExecutionReport
from nautilus_trader.execution.messages cimport OrderStatusReport
from nautilus_trader.live.execution_client cimport LiveExecutionClient
from nautilus_trader.live.execution_engine cimport LiveExecutionEngine
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.order_side cimport OrderSideParser
from nautilus_trader.model.c_enums.order_state cimport OrderState
from nautilus_trader.model.c_enums.order_type cimport OrderType
from nautilus_trader.model.c_enums.order_type cimport OrderTypeParser
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForce
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForceParser
from nautilus_trader.model.commands cimport CancelOrder
from nautilus_trader.model.commands cimport SubmitBracketOrder
from nautilus_trader.model.commands cimport SubmitOrder
from nautilus_trader.model.commands cimport UpdateOrder
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport ExecutionId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport VenueOrderId
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.orders.base cimport PassiveOrder


cdef int _SECONDS_IN_HOUR = 60 * 60


cdef class CCXTExecutionClient(LiveExecutionClient):
    """
    Provides an execution client for the unified CCXT Pro API.
    """

    def __init__(
        self,
        client not None: ccxt.Exchange,
        AccountId account_id not None,
        LiveExecutionEngine engine not None,
        LiveClock clock not None,
        Logger logger not None,
    ):
        """
        Initialize a new instance of the `CCXTExecutionClient` class.

        Parameters
        ----------
        client : ccxt.Exchange
            The unified CCXT client.
        account_id : AccountId
            The account identifier for the client.
        engine : LiveDataEngine
            The data engine for the client.
        clock : LiveClock
            The clock for the client.
        logger : Logger
            The logger for the client.

        """
        cdef InstrumentProvider instrument_provider = CCXTInstrumentProvider(
            client=client,
            load_all=False,
        )

        super().__init__(
            ClientId(client.name.upper()),
            account_id,
            engine,
            instrument_provider,
            clock,
            logger,
            config={
                "name": f"CCXTExecClient-{client.name.upper()}",
            }
        )

        self._client = client
        self.is_connected = False

        # Scheduled tasks
        self._update_instruments_task = None

        # Streaming tasks
        self._watch_balances_task = None
        self._watch_orders_task = None
        self._watch_exec_reports_task = None

    cpdef void connect(self) except *:
        """
        Connect the client.
        """
        self._log.info("Connecting...")

        if self._client.check_required_credentials():
            self._log.info("API credentials validated.", color=LogColor.GREEN)
        else:
            self._log.error("API credentials missing or invalid.")
            self._log.error(f"Required: {self._client.required_credentials()}.")
            return

        # Schedule instruments update
        delay = _SECONDS_IN_HOUR
        update = self._run_after_delay(delay, self._update_instruments(delay))
        self._update_instruments_task = self._loop.create_task(update)

        self._loop.create_task(self._connect())

    async def _connect(self):
        try:
            await self._load_instruments()
            await self._update_balances()
        except CCXTError as ex:
            self._log_ccxt_error(ex, self._connect.__name__)
            return

        # Start streams
        self._watch_balances_task = self._loop.create_task(self._watch_balances())
        self._watch_orders_task = self._loop.create_task(self._watch_orders())
        self._watch_exec_reports_task = self._loop.create_task(self._watch_exec_reports())

        self.is_connected = True
        self._log.info("Connected.")

    async def generate_order_status_report(self, Order order):
        """
        Generate an order status report for the given order.

        If an error occurs then logs and returns None.

        Parameters
        ----------
        order : Order
            The order for the report.

        Returns
        -------
        OrderStatusReport or None

        """
        self._log.info(f"Generating OrderStatusReport for {repr(order.venue_order_id)}...")

        if order.venue_order_id.is_null():
            self._log.error(f"Cannot reconcile state for {repr(order.client_order_id)}, "
                            f"VenueOrderId was 'NULL'.")
            return None  # Cannot generate state report

        cdef Instrument instrument = self._instrument_provider.find(order.instrument_id)
        if instrument is None:
            self._log.error(f"Cannot reconcile state for {repr(order.client_order_id)}, "
                            f"instrument for {order.instrument_id} not found.")
            return None  # Cannot generate state report

        try:
            response = await self._client.fetch_order(
                id=order.venue_order_id.value,
                symbol=order.instrument_id.symbol.value,
            )
            # self._log.info(str(response), LogColor.BLUE)  # TODO: Development
        except CCXTError as ex:
            self._log_ccxt_error(ex, self._update_balances.__name__)
            return None

        if response is None:
            self._log.error(f"No order found for {order.venue_order_id.value}.")
            return None

        filled_qty = Decimal(f"{response['filled']:.{instrument.price_precision}f}")

        # Determine state
        status = response["status"]
        if status == "open" and filled_qty > 0:
            state = OrderState.PARTIALLY_FILLED
        elif status == "closed":
            state = OrderState.FILLED
        elif status == "canceled":
            state = OrderState.CANCELLED
        elif status == "expired":
            state = OrderState.EXPIRED
        else:
            state = OrderState.ACCEPTED

        return OrderStatusReport(
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            order_state=state,
            filled_qty=Quantity(filled_qty),
            timestamp_ns=millis_to_nanos(millis=response["timestamp"]),
        )

    async def generate_exec_reports(
        self,
        VenueOrderId venue_order_id,
        Symbol symbol,
        datetime since=None,
    ):
        """
        Generate a list of execution reports.

        The returned list may be empty if no trades match the given parameters.

        Parameters
        ----------
        venue_order_id : VenueOrderId
            The venue order identifier for the trades.
        symbol : Symbol
            The symbol for the trades.
        since : datetime, optional
            The timestamp to filter trades on.

        Returns
        -------
        list[ExecutionReport]

        """
        Condition.not_none(venue_order_id, "venue_order_id")
        Condition.not_none(symbol, "symbol")

        self._log.info(f"Generating list[ExecutionReport] for {repr(venue_order_id)}...")

        cdef list reports = []  # Output
        cdef list response
        try:
            response = await self._client.fetch_my_trades(
                symbol=symbol.value,
                since=dt_to_unix_millis(since),
            )
        except CCXTError as ex:
            self._log_ccxt_error(ex, self.generate_trades.__name__)
            return reports

        if response is None:
            return reports

        cdef list fills = [fill for fill in response if fill["order"] == venue_order_id.value]
        self._log.info(str(fills), color=LogColor.GREEN)  # TODO: Development

        if not fills:
            return reports

        cdef ClientOrderId client_order_id = self._engine.cache.client_order_id(venue_order_id)
        if client_order_id is None:
            self._log.error(f"Cannot generate trades list: "
                            f"no ClientOrderId found for {repr(venue_order_id)}.")
            return reports

        cdef InstrumentId instrument_id = InstrumentId(symbol, self.venue)
        cdef Instrument instrument = self._instrument_provider.find(instrument_id)
        if instrument is None:
            self._log.error(f"Cannot reconcile state for {repr(client_order_id)}, "
                            f"instrument for {instrument_id} not found.")
            return  # Cannot generate state report

        cdef dict fill
        cdef ExecutionReport report
        for fill in fills:
            report = ExecutionReport(
                client_order_id=client_order_id,
                venue_order_id=venue_order_id,
                execution_id=ExecutionId(str(fill["id"])),
                last_qty=Quantity(fill["amount"], instrument.size_precision),
                last_px=Price(fill["price"], instrument.price_precision),
                commission=self._parse_commission(fill),
                liquidity_side=LiquiditySide.TAKER if fill["takerOrMaker"] == "taker" else LiquiditySide.MAKER,
                execution_ns=millis_to_nanos(millis=fill["timestamp"]),
                timestamp_ns=self._clock.timestamp_ns(),
            )
            reports.append(report)

        return reports

    cpdef void disconnect(self) except *:
        """
        Disconnect the client.
        """
        self._loop.create_task(self._disconnect())

    async def _disconnect(self):
        self._log.info("Disconnecting...")

        stop_tasks = []
        # Cancel scheduled tasks
        if self._update_instruments_task:
            self._update_instruments_task.cancel()
            # TODO: This task is not finishing
            # stop_tasks.append(self._update_instruments_task)

        # Cancel streaming tasks
        if self._watch_balances_task:
            self._watch_balances_task.cancel()
            stop_tasks.append(self._watch_balances_task)
        if self._watch_orders_task:
            self._watch_orders_task.cancel()
            stop_tasks.append(self._watch_orders_task)
        if self._watch_exec_reports_task:
            self._watch_exec_reports_task.cancel()
            stop_tasks.append(self._watch_exec_reports_task)

        # Wait for all tasks to complete
        if stop_tasks:
            await asyncio.gather(*stop_tasks)

        # Ensure ccxt closed
        self._log.info("Closing WebSocket(s)...")
        await self._client.close()

        self.is_connected = False
        self._log.info("Disconnected.")

# -- COMMAND HANDLERS ------------------------------------------------------------------------------

    cpdef void submit_order(self, SubmitOrder command) except *:
        """
        Submit the order contained in the given command for execution.

        Parameters
        ----------
        command : SubmitOrder
            The command to execute.

        """
        Condition.not_none(command, "command")

        self._loop.create_task(self._submit_order(command.order))

    cpdef void submit_bracket_order(self, SubmitBracketOrder command) except *:
        """
        Submit the bracket order contained in the given command for execution.

        Parameters
        ----------
        command : SubmitBracketOrder
            The command to execute.

        """
        Condition.not_none(command, "command")

        self._log.error("Cannot submit bracket orders in this version.")

    cpdef void update_order(self, UpdateOrder command) except *:
        """
        Update the order with parameters contained in the command.

        Parameters
        ----------
        command : UpdateOrder
            The command to execute.

        """
        Condition.not_none(command, "command")

        # TODO: Implement
        self._log.error("Cannot update orders in this version.")

    cpdef void cancel_order(self, CancelOrder command) except *:
        """
        Cancel the order with the `ClientOrderId` contained in the given command.

        Parameters
        ----------
        command : CancelOrder
            The command to execute.

        """
        Condition.not_none(command, "command")

        self._loop.create_task(self._cancel_order(command.client_order_id))

# -- INTERNAL --------------------------------------------------------------------------------------

    cdef inline void _log_ccxt_error(self, ex, str method_name) except *:
        self._log.warning(f"{type(ex).__name__}: {ex} in {method_name}")

    async def _run_after_delay(self, double delay, coro):
        await asyncio.sleep(delay)
        return await coro

# -- REQUESTS --------------------------------------------------------------------------------------

    async def _load_instruments(self):
        await self._instrument_provider.load_all_async()
        self._log.info(f"Updated {self._instrument_provider.count} instruments.")

    async def _update_instruments(self, delay):
        await self._load_instruments()

        # Reschedule instruments update
        update = self._run_after_delay(delay, self._update_instruments(delay))
        self._update_instruments_task = self._loop.create_task(update)

    async def _update_balances(self):
        cdef dict params = {'type': 'spot'}  # TODO: Hard coded to spot account for now
        cdef dict response
        try:
            response = await self._client.fetch_balance(params)
        except TypeError:
            # Temporary workaround for testing
            response = self._client.fetch_balance
        except CCXTError as ex:
            self._log_ccxt_error(ex, self._update_balances.__name__)
            return

        self._on_account_state(response)

# -- STREAMS ---------------------------------------------------------------------------------------

    async def _watch_balances(self):
        cdef dict params = {'type': 'spot'}  # TODO: Hard coded to spot account for now
        cdef dict event
        try:
            while True:
                try:
                    event = await self._client.watch_balance(params)
                    self._on_account_state(event)
                except CCXTError as ex:
                    self._log_ccxt_error(ex, self._watch_balances.__name__)
                    continue
        except asyncio.CancelledError as ex:
            self._log.debug(f"Cancelled `_watch_balances` for {self.account_id}.")
        except Exception as ex:
            self._log.exception(ex)

    async def _watch_orders(self):
        try:
            while True:
                try:
                    # events type is ArrayCacheBySymbolById
                    events = await self._client.watch_orders()
                    self._on_order_status(events[0])  # Only caching 1 event
                except CCXTError as ex:
                    self._log_ccxt_error(ex, self._watch_orders.__name__)
                    continue
        except asyncio.CancelledError as ex:
            self._log.debug(f"Cancelled `_watch_orders`.")
        except Exception as ex:
            self._log.exception(ex)

    async def _watch_exec_reports(self):
        try:
            while True:
                try:
                    # events type is ArrayCacheBySymbolById
                    events = await self._client.watch_my_trades()
                    self._on_exec_report(events[0])  # Only caching 1 event
                except CCXTError as ex:
                    self._log_ccxt_error(ex, self._watch_balances.__name__)
                    continue
        except asyncio.CancelledError as ex:
            self._log.debug(f"Cancelled `_watch_my_trades` for {self.account_id}.")
        except Exception as ex:
            self._log.exception(ex)

# -- COMMANDS --------------------------------------------------------------------------------------

    async def _submit_order(self, Order order):
        self._log.debug(f"Submitted {order}.")

        # Generate event here to ensure it is processed before OrderAccepted
        self.generate_order_submitted(
            client_order_id=order.client_order_id,
            timestamp_ns=self._clock.timestamp_ns(),
        )

        try:
            # Submit order and await response
            await self._client.create_order(
                symbol=order.instrument_id.symbol.value,
                type=OrderTypeParser.to_str(order.type).lower(),
                side=OrderSideParser.to_str(order.side).lower(),
                amount=str(order.quantity),
                price=str(order.price) if isinstance(order, PassiveOrder) else None,
                params={'clientOrderId': order.client_order_id.value},
            )
        except CCXTError as ex:
            self.generate_order_rejected(
                client_order_id=order.client_order_id,
                reason=str(ex),
                timestamp_ns=self._clock.timestamp_ns(),
            )

    async def _cancel_order(self, ClientOrderId client_order_id):
        cdef Order order = self._engine.cache.order(client_order_id)
        if order is None:
            self._log.error(f"Cannot cancel order, {repr(client_order_id)} not found.")
            return  # Cannot cancel

        if not order.is_working_c():
            self._log.error(f"Cannot cancel order, state=OrderState.{order.state_string_c()}.")
            return  # Cannot cancel

        try:
            await self._client.cancel_order(
                id=order.venue_order_id.value,
                symbol=order.instrument_id.symbol.value,
            )
        except CCXTError as ex:
            self._log_ccxt_error(ex, self._cancel_order.__name__)
            return

# -- EVENTS ----------------------------------------------------------------------------------------

    cdef inline void _on_account_state(self, dict event) except *:
        cdef list balances = []
        cdef list balances_free = []
        cdef list balances_locked = []

        cdef dict event_free = event["free"]
        cdef dict event_used = event["used"]
        cdef dict event_total = event["total"]

        if event_free == self._account_last_free \
                and event_used == self._account_last_used \
                and event_total == self._account_last_used:
            return  # No updates

        self._account_last_free = event_free
        self._account_last_used = event_used
        self._account_last_total = event_total

        cdef str code
        cdef Currency currency

        # Update total balances
        for code, amount in event_total.items():
            if amount:
                currency = self._instrument_provider.currency(code)
                if currency is None:
                    self._log.error(f"Cannot update total balance for {code} "
                                    f"(no currency loaded).")
                balances.append(Money(amount, currency))

        # Update free balances
        for code, amount in event_free.items():
            if amount:
                currency = self._instrument_provider.currency(code)
                if currency is None:
                    self._log.error(f"Cannot update total balance for {code} "
                                    f"(no currency loaded).")
                balances_free.append(Money(amount, currency))

        # Update locked balances
        for code, amount in event_used.items():
            if amount:
                currency = self._instrument_provider.currency(code)
                if currency is None:
                    self._log.error(f"Cannot update total balance for {code} "
                                    f"(no currency loaded).")
                balances_locked.append(Money(amount, currency))

        # Generate event
        self.generate_account_state(
            balances,
            balances_free,
            balances_locked,
        )

    cdef inline void _on_order_status(self, dict event) except *:
        cdef ClientOrderId client_order_id = ClientOrderId(event["clientOrderId"])
        cdef VenueOrderId venue_order_id = VenueOrderId(event["id"])

        cdef int64_t timestamp_ns = <int64_t>(event["timestamp"])
        cdef str status = event["status"]
        # status == "rejected" should be captured in `submit_order`
        if status == "open" and event["filled"] == 0:
            self.generate_order_accepted(client_order_id, venue_order_id, timestamp_ns)
        elif status == "canceled":
            self.generate_order_cancelled(client_order_id, venue_order_id, timestamp_ns)
        elif status == "expired":
            self.generate_order_expired(client_order_id, venue_order_id, timestamp_ns)

    cdef inline void _on_exec_report(self, dict event) except *:
        cdef VenueOrderId venue_order_id = VenueOrderId(event["order"])

        cdef ClientOrderId client_order_id = self._engine.cache.client_order_id(venue_order_id)
        if client_order_id is None:
            self._log.error(f"Cannot fill un-cached order with {repr(venue_order_id)}.")
            return

        cdef Order order = self._engine.cache.order(client_order_id)
        if order is None:
            # If `reconcile_state` has done its job this should never happen
            self._log.error(f"Cannot fill un-cached order with {repr(client_order_id)}.")
            return

        cdef Instrument instrument = self._instrument_provider.find(order.instrument_id)
        if instrument is None:
            self._log.error(f"Cannot reconcile state for {repr(order.client_order_id)}, "
                            f"instrument for {order.instrument_id} not found.")
            return  # Cannot generate state report

        self.generate_order_filled(
            client_order_id=order.client_order_id,
            venue_order_id=venue_order_id,
            execution_id=ExecutionId(event["id"]),
            position_id=None,  # Assigned in engine,
            instrument_id=order.instrument_id,
            order_side=order.side,
            last_qty=Quantity(event["amount"], instrument.size_precision),
            last_px=Price(event["price"], instrument.price_precision),
            quote_currency=instrument.quote_currency,
            is_inverse=instrument.is_inverse,
            commission=self._parse_commission(event),
            liquidity_side=LiquiditySide.TAKER if event["takerOrMaker"] == "taker" else LiquiditySide.MAKER,
            timestamp_ns=(millis_to_nanos(millis=event["timestamp"])),
        )

    cdef inline Money _parse_commission(self, dict event):
        cdef dict commission = event.get("fee", {})
        cdef str commission_currency = commission.get("currency")
        if commission_currency is None:
            return None

        cdef Currency currency = self._instrument_provider.currency(commission_currency)
        if currency is None:
            self._log.error(
                f"Cannot determine commission: currency {commission_currency} not found.",
            )
            return None

        return Money(commission.get("cost", 0), currency)


cdef class BinanceCCXTExecutionClient(CCXTExecutionClient):
    """
    Provides a CCXT pro execution client for the Binance exchange.
    """

    def __init__(
            self,
            client not None: ccxt.Exchange,
            AccountId account_id not None,
            LiveExecutionEngine engine not None,
            LiveClock clock not None,
            Logger logger not None,
    ):
        """
        Initialize a new instance of the `BinanceCCXTExecutionClient` class.

        Parameters
        ----------
        client : ccxt.Exchange
            The unified CCXT client.
        account_id : AccountId
            The account identifier for the client.
        engine : LiveDataEngine
            The data engine for the client.
        clock : LiveClock
            The clock for the client.
        logger : Logger
            The logger for the client.

        """
        Condition.true(client.name.upper() == "BINANCE", "client.name != BINANCE")

        super().__init__(
            client,
            account_id,
            engine,
            clock,
            logger,
        )

# -- COMMANDS ----------------------------------------------------------------------------------

    async def _submit_order(self, Order order):
        # Common arguments

        if order.time_in_force == TimeInForce.GTD:
            raise ValueError("TimeInForce.GTD not supported in this version.")

        if order.time_in_force == TimeInForce.DAY:
            raise ValueError("Binance does not support TimeInForce.DAY.")

        cdef dict params = {
            "newClientOrderId": order.client_order_id.value,
            "recvWindow": 10000  # TODO: Server time sync issue?
        }

        cdef str order_type = ""
        if order.type == OrderType.MARKET:
            order_type = "MARKET"
        elif order.type == OrderType.LIMIT and order.is_post_only:
            # Cannot be hidden as post only is True
            order_type = "LIMIT_MAKER"
        elif order.type == OrderType.LIMIT:
            if order.is_hidden:
                raise ValueError("Binance does not support hidden orders.")
            order_type = "LIMIT"
            params["timeInForce"] = TimeInForceParser.to_str(order.time_in_force)
        elif order.type == OrderType.STOP_MARKET:
            if order.side == OrderSide.BUY:
                order_type = "STOP_LOSS"
            elif order.side == OrderSide.SELL:
                order_type = "TAKE_PROFIT"
            params["stopPrice"] = str(order.price)
        else:
            raise ValueError(f"Invalid OrderType, "
                             f"was {OrderTypeParser.to_str(order.type)}")

        self._log.debug(f"Submitted {order}.")
        # Generate event here to ensure it is processed before OrderAccepted
        self.generate_order_submitted(
            client_order_id=order.client_order_id,
            timestamp_ns=self._clock.timestamp_ns(),
        )

        try:
            # Submit order and await response
            await self._client.create_order(
                symbol=order.instrument_id.symbol.value,
                type=order_type,
                side=OrderSideParser.to_str(order.side),
                amount=str(order.quantity),
                price=str(order.price) if isinstance(order, PassiveOrder) else None,
                params=params,
            )
        except CCXTError as ex:
            self.generate_order_rejected(
                client_order_id=order.client_order_id,
                reason=str(ex),
                timestamp_ns=self._clock.timestamp_ns(),
            )


cdef class BitmexCCXTExecutionClient(CCXTExecutionClient):
    """
    Provides a CCXT Pro execution client for the Bitmex exchange.
    """

    def __init__(
            self,
            client not None: ccxt.Exchange,
            AccountId account_id not None,
            LiveExecutionEngine engine not None,
            LiveClock clock not None,
            Logger logger not None,
    ):
        """
        Initialize a new instance of the `BitmexCCXTExecutionClient` class.

        Parameters
        ----------
        client : ccxt.Exchange
            The unified CCXT client.
        account_id : AccountId
            The account identifier for the client.
        engine : LiveDataEngine
            The data engine for the client.
        clock : LiveClock
            The clock for the client.
        logger : Logger
            The logger for the client.

        """
        Condition.true(client.name.upper() == "BITMEX", "client.name != BITMEX")

        super().__init__(
            client,
            account_id,
            engine,
            clock,
            logger,
        )

# -- COMMANDS ----------------------------------------------------------------------------------

    async def _submit_order(self, Order order):
        if order.time_in_force == TimeInForce.GTD:
            raise ValueError("GTD not supported in this version.")

        cdef dict params = {
            "clOrdID": order.client_order_id.value,
        }

        cdef str order_type = ""
        cdef list exec_instructions = []
        if order.type == OrderType.MARKET:
            order_type = "Market"
        elif order.type == OrderType.LIMIT:
            order_type = "Limit"
            if order.is_hidden:
                params["displayQty"] = 0
            # Execution instructions
            if order.is_post_only:
                exec_instructions.append("ParticipateDoNotInitiate")
            if order.is_reduce_only:
                exec_instructions.append("ReduceOnly")
            if exec_instructions:
                params["execInst"] = ','.join(exec_instructions)
        elif order.type == OrderType.STOP_MARKET:
            order_type = "StopMarket"
            params["stopPx"] = str(order.price)
            if order.is_reduce_only:
                params["execInst"] = "ReduceOnly"

        if order.time_in_force == TimeInForce.DAY:
            params["timeInForce"] = "Day"
        elif order.time_in_force == TimeInForce.GTC:
            params["timeInForce"] = "GoodTillCancel"
        elif order.time_in_force == TimeInForce.IOC:
            params["timeInForce"] = "ImmediateOrCancel"
        elif order.time_in_force == TimeInForce.FOK:
            params["timeInForce"] = "FillOrKill"

        self._log.debug(f"Submitted {order}.")
        # Generate event here to ensure it is processed before OrderAccepted
        self.generate_order_submitted(
            client_order_id=order.client_order_id,
            timestamp_ns=self._clock.timestamp_ns(),
        )

        try:
            # Submit order and await response
            await self._client.create_order(
                symbol=order.instrument_id.symbol.value,
                type=order_type,
                side=OrderSideParser.to_str(order.side).capitalize(),
                amount=str(order.quantity),
                price=str(order.price) if isinstance(order, PassiveOrder) else None,
                params=params,
            )
        except CCXTError as ex:
            self.generate_order_rejected(
                client_order_id=order.client_order_id,
                reason=str(ex),
                timestamp_ns=self._clock.timestamp_ns(),
            )
