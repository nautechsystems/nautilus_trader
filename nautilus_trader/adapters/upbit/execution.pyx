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

from cpython.datetime cimport datetime
from cpython.datetime cimport timedelta
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
from nautilus_trader.model.events cimport AccountState
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport ExecutionId
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport VenueOrderId
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.order.base cimport Order
from nautilus_trader.model.order.base cimport PassiveOrder


cdef int _SECONDS_IN_HOUR = 60 * 60


cdef class UpbitExecutionClient(LiveExecutionClient):
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

        # Order quantity cache (to handle CCXT not tracking ClOrdID and cumulative qty in trade events)
        self._cached_orders = {}  # type: {VenueOrderId: Order}
        self._cached_filled = {}  # type: {VenueOrderId: Decimal}

    cdef void _on_reset(self) except *:
        self._cached_orders.clear()
        self._cached_filled.clear()

    cpdef void connect(self) except *:
        """
        Connect the client.
        """
        self._log.info("Connecting...")

        # Re-cache orders
        cdef list orders_all = self._engine.cache.orders()
        cdef Order order
        for order in orders_all:
            if order.is_completed_c():
                continue
            if order.instrument_id.venue.client_id == self.id:
                self._cached_orders[order.venue_order_id] = order
                self._cached_filled[order.venue_order_id] = order.filled_qty.as_decimal()

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
            )
            self._log.info(str(response), LogColor.BLUE)  # TODO: Development
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

    async def generate_exec_reports(self, VenueOrderId venue_order_id, Symbol symbol, datetime since=None):
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
        cdef list response = []
        try:
            response.extend(await self._client.fetch_open_orders(
                symbol=symbol.value
            ))
            self._log.info(f"112 {response}")
        except CCXTError as ex:
            self._log_ccxt_error(ex, self.generate_trades.__name__)
            return reports

        try:
            response.extend(await self._client.fetch_closed_orders(
                symbol=symbol.value,
            ))
            self._log.info(f"113 {response}")
        except CCXTError as ex:
            self._log_ccxt_error(ex, self.generate_trades.__name__)
            return reports

        if len(response) == 0:
            return reports  # TODO: Is this necessary??

        cdef list fills = [fill for fill in response if fill["id"] == venue_order_id.value]
        self._log.info(str(fills), color=LogColor.GREEN)  # TODO: Development

        if not fills:
            return reports

        cdef ClientOrderId client_order_id = self._engine.cache.client_order_id(venue_order_id)
        if client_order_id is None:
            self._log.error(f"Cannot generate trades list: "
                            f"no ClientOrderId found for {repr(venue_order_id)}.")
            return reports

        cdef dict fill
        cdef ExecutionReport report
        for fill in fills:
            report = ExecutionReport(
                execution_id=ExecutionId(str(fill["id"])),
                client_order_id=client_order_id,
                venue_order_id=venue_order_id,
                last_qty=Decimal(fill["amount"]),
                last_px=Decimal(fill["price"]),
                commission_amount=Decimal(fill["fee"]["cost"]),
                commission_currency=fill["fee"]["currency"],
                liquidity_side=LiquiditySide.TAKER if fill["type"] == "market" else LiquiditySide.MAKER,
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
            stop_tasks.append(self._update_instruments_task)

        # Cancel streaming tasks
        if self._watch_balances_task:
            self._watch_balances_task.cancel()
            stop_tasks.append(self._watch_balances_task)

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
        cdef dict params = {}
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

    async def _watch_balances(self, interval_seconds=3.0):
        cdef dict params = {}
        cdef dict event
        try:
            while True:
                try:
                    event = await self._client.fetch_balance(params)
                    self._on_account_state(event)
                except CCXTError as ex:
                    self._log_ccxt_error(ex, self._watch_balances.__name__)
                    continue
                finally:
                    await asyncio.sleep(interval_seconds)
        except asyncio.CancelledError as ex:
            self._log.debug(f"Cancelled `_watch_balances` for {self.account_id}.")
        except Exception as ex:
            self._log.exception(ex)

# -- COMMANDS --------------------------------------------------------------------------------------

    async def _submit_order(self, Order order):
        if order.time_in_force == TimeInForce.GTD:
            raise ValueError("TimeInForce.GTD not supported in this version.")

        if order.time_in_force == TimeInForce.DAY:
            raise ValueError("Upbit does not support TimeInForce.DAY.")

        cdef dict params = {"identifier": f"{order.client_order_id.value}"}
        cdef str order_type = ""
        if order.type == OrderType.MARKET:
            order_type = "market"
        elif order.type == OrderType.LIMIT:
            if order.is_post_only:
                raise ValueError("Upbit does not support post only orders.")
            if order.is_hidden:
                raise ValueError("Upbit does not support hidden orders.")
            order_type = "limit"
        else:
            raise ValueError(f"Invalid OrderType, "
                             f"was {OrderTypeParser.to_str(order.type)}")

        # Generate event here to ensure it is processed before OrderAccepted
        self._generate_order_submitted(
            client_order_id=order.client_order_id,
            timestamp_ns=self._clock.timestamp_ns()
        )

        try:
            # Submit order and await response
            # ccxt client will quantize price, and quantity.
            response = await self._client.create_order(
                symbol=order.instrument_id.symbol.value,
                type=order_type,
                side=OrderSideParser.to_str(order.side).lower(),
                amount=str(order.quantity),
                price=str(order.price) if isinstance(order, PassiveOrder) else None,
                params=params,
            )
            self._on_order_status(response, client_order_id=order.client_order_id)
            self._loop.create_task(self._check_order(order))
        except CCXTError as ex:
            self._generate_order_rejected(
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
            response = await self._client.cancel_order(
                id=order.venue_order_id.value,
                symbol=order.instrument_id.symbol.value,
            )
            self._on_order_status(response)
            self._loop.create_task(self._check_order(order))
        except CCXTError as ex:
            self._log_ccxt_error(ex, self._cancel_order.__name__)
            return

    async def _check_order(self, Order order, float timeout_seconds=30.0, float interval_seconds=1.0):
        cdef datetime timeout = self._clock.utc_now() + timedelta(seconds=timeout_seconds)
        try:
            while self._clock.utc_now() < timeout:
                try:
                    if order.state_c() in [OrderState.FILLED,
                                           OrderState.CANCELLED,
                                           OrderState.EXPIRED]:
                        self._log.info(f"Order Done: {order}", color=LogColor.GREEN)
                        return
                    response = await self._client.fetch_order(
                        id=order.venue_order_id.value
                    )
                    self._on_order_status(response, client_order_id=order.client_order_id)
                except CCXTError as ex:
                    self._log_ccxt_error(ex, self._check_order.__name__)
                    continue
                finally:
                    await asyncio.sleep(interval_seconds)
            self._log.warning(f"Open Order: {order}")
        except asyncio.CancelledError as ex:
            self._log.debug(f"Cancelled `_check_order`.")
        except Exception as ex:
            self._log.exception(ex)

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
        cdef AccountState account_state = AccountState(
            self.account_id,
            balances,
            balances_free,
            balances_locked,
            {},
            self._uuid_factory.generate(),
            self._clock.timestamp_ns(),
        )

        self._handle_event(account_state)

    cdef inline void _on_order_status(self, dict event, ClientOrderId client_order_id=None) except *:
        cdef:
            Order order
            cdef VenueOrderId venue_order_id

        venue_order_id = VenueOrderId(event["id"])
        if client_order_id is None:
            client_order_id = ClientOrderId(event['identifier'])

        if venue_order_id not in self._cached_orders:
            order = self._engine.cache.order(client_order_id)
            if order is None:
                # If state resolution has done its job this should never happen
                self._log.error(f"Cannot fill un-cached order with {repr(venue_order_id)}.")
                return
            self._cache_order(venue_order_id, order)
        else:
            order = self._cached_orders.get(venue_order_id)

        cdef int64_t timestamp_ns = <int64_t>(event["timestamp"])
        cdef str status = event["status"]
        # status == "rejected" should be captured in `submit_order`
        if status == "open" and event["filled"] == 0:
            if order.state_c() is not OrderState.ACCEPTED:
                self._generate_order_accepted(client_order_id, venue_order_id, timestamp_ns)
        elif status == "open" and event["filled"] > 0:
            self._on_exec_report(event)
        elif status == "canceled":
            self._generate_order_cancelled(client_order_id, venue_order_id, timestamp_ns)
            self._decache_order(venue_order_id)
        elif status == "closed":
            # Upbit skip open status when marketable order filled.
            if order.state_c() == OrderState.SUBMITTED:
                self._generate_order_accepted(client_order_id, venue_order_id, timestamp_ns)
            self._on_exec_report(event)

    cdef inline void _on_exec_report(self, dict event):
        cdef VenueOrderId venue_order_id = VenueOrderId(event["id"])
        cdef Order order = self._cached_orders.get(venue_order_id)
        if order is None:
            client_order_id = self._engine.cache.client_order_id(venue_order_id)
            if client_order_id is None:
                self._log.error(f"Cannot fill un-cached order with {repr(venue_order_id)}.")
                return
            order = self._engine.cache.order(client_order_id)
            if order is None:
                # If `reconcile_state` has done its job this should never happen
                self._log.error(f"Cannot fill un-cached order with {repr(venue_order_id)}.")
                return
            self._cache_order(venue_order_id, order)

        prev_cum_qty: Decimal = self._cached_filled.get(venue_order_id, Decimal())
        cum_qty = Decimal(f"{event['filled']:.{order.quantity.precision_c()}f}")
        last_qty: Decimal = cum_qty - prev_cum_qty
        self._cached_filled[order.venue_order_id] = cum_qty
        leaves_qty: Decimal = order.quantity - cum_qty
        exchange_given_leaves_qty = Decimal(f"{event['remaining']}")
        if leaves_qty == 0:
            self._decache_order(venue_order_id)
        if leaves_qty != exchange_given_leaves_qty:
            self._log.warning(f"Can't match with leaves qty({leaves_qty}), "
                              f"exchange given leaves qty({exchange_given_leaves_qty}.")

        self._generate_order_filled(
            client_order_id=order.client_order_id,
            venue_order_id=venue_order_id,
            execution_id=ExecutionId(event["id"]),
            instrument_id=order.instrument_id,
            order_side=order.side,
            last_qty=last_qty,
            last_px=event["price"],
            cum_qty=cum_qty,
            leaves_qty=order.quantity - cum_qty,
            commission_amount=event.get("fee", {}).get("cost", 0),
            commission_currency=event.get("fee", {}).get("currency"),
            liquidity_side=LiquiditySide.TAKER if event["type"] == "market" else LiquiditySide.MAKER,
            timestamp_ns=(millis_to_nanos(millis=event["timestamp"])),
        )

    cdef inline void _cache_order(self, VenueOrderId venue_order_id, Order order) except *:
        self._cached_orders[venue_order_id] = order
        self._cached_filled[venue_order_id] = order.filled_qty
        self._log.debug(f"Cached {repr(venue_order_id)} {order}.")

    cdef inline void _decache_order(self, VenueOrderId venue_order_id) except *:
        self._cached_orders.pop(venue_order_id, None)
        self._cached_filled.pop(venue_order_id, None)
        self._log.debug(f"De-cached {repr(venue_order_id)}.")


