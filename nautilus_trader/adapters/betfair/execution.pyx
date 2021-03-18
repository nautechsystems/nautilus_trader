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

import betfairlightweight

from cpython.datetime cimport datetime

from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logging cimport LogColor
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.providers cimport InstrumentProvider
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport from_unix_time_ms
from nautilus_trader.core.datetime cimport to_unix_time_ms
from nautilus_trader.execution.reports cimport ExecutionStateReport
from nautilus_trader.live.execution_client cimport LiveExecutionClient
from nautilus_trader.live.execution_engine cimport LiveExecutionEngine
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.c_enums.order_side cimport OrderSideParser
from nautilus_trader.model.c_enums.order_state cimport OrderState
from nautilus_trader.model.c_enums.order_type cimport OrderTypeParser
from nautilus_trader.model.commands cimport AmendOrder
from nautilus_trader.model.commands cimport CancelOrder
from nautilus_trader.model.commands cimport SubmitBracketOrder
from nautilus_trader.model.commands cimport SubmitOrder
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.events cimport AccountState
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport ExecutionId
from nautilus_trader.model.identifiers cimport OrderId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.order.base cimport Order
from nautilus_trader.model.order.base cimport PassiveOrder

from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider

cdef int _SECONDS_IN_HOUR = 60 * 60


cdef class BetfairExecutionClient(LiveExecutionClient):
    """
    Provides an execution client for Betfair.
    """

    def __init__(
        self,
        client not None: betfairlightweight.APIClient,
        AccountId account_id not None,
        LiveExecutionEngine engine not None,
        LiveClock clock not None,
        Logger logger not None,
    ):
        """
        Initialize a new instance of the `BetfairExecutionClient` class.

        Parameters
        ----------
        client : betfairlightweight.APIClient
            The Betfair client.
        account_id : AccountId
            The account identifier for the client.
        engine : LiveDataEngine
            The data engine for the client.
        clock : LiveClock
            The clock for the client.
        logger : Logger
            The logger for the client.

        """
        cdef InstrumentProvider instrument_provider = BetfairInstrumentProvider(
            client=client,
            load_all=False,
        )

        super().__init__(
            "BETFAIR",
            account_id,
            engine,
            instrument_provider,
            clock,
            logger,
            config={
                "name": f"BetfairExecClient",
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
        self._cached_orders = {}  # type: {OrderId: Order}
        self._cached_filled = {}  # type: {OrderId: Decimal}

    cdef void _on_reset(self) except *:
        self._cached_orders.clear()
        self._cached_filled.clear()

    cpdef void connect(self) except *:
        """
        Connect the client.
        """
        self._log.info("Connecting...")

        if self._client.check_required_credentials():
            self._log.info("API credentials validated.", LogColor.GREEN)
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
        except betfairlightweight.BetfairError as ex:
            self._log_betfair_error(ex, self._connect.__name__)
            return

        # Start streams
        self._watch_balances_task = self._loop.create_task(self._watch_balances())
        self._watch_orders_task = self._loop.create_task(self._watch_orders())
        self._watch_exec_reports_task = self._loop.create_task(self._watch_exec_reports())

        self.is_connected = True
        self._log.info("Connected.")

    async def state_report(self, list active_orders):
        """
        Return an execution state report based on the given list of active
        orders.

        Parameters
        ----------
        active_orders : list[Order]
            The orders which currently have an 'active' status.

        Returns
        -------
        ExecutionStateReport

        """
        Condition.not_none(active_orders, "active_orders")

        cdef dict order_states = {}
        cdef dict order_filled = {}
        cdef dict position_states = {}

        if not active_orders:
            # Nothing to resolve
            return ExecutionStateReport(
                venue=self.venue,
                account_id=self.account_id,
                order_states=order_states,
                order_filled=order_filled,
                position_states=position_states,
            )

        cdef int count = len(active_orders)
        self._log.info(
            f"Resolving state: {count} active order{'s' if count > 1 else ''}...",
            LogColor.BLUE,
        )

        cdef Instrument instrument
        cdef Order order
        cdef str status
        cdef dict response
        cdef list trades
        cdef list order_trades
        for order in active_orders:
            if order.id.is_null():
                self._log.error(f"Cannot resolve state for {repr(order.cl_ord_id)}, "
                                f"OrderId was 'NULL'.")
                continue  # Cannot resolve order
            instrument = self._instrument_provider.find_c(order.symbol)
            if instrument is None:
                self._log.error(f"Cannot resolve state for {repr(order.cl_ord_id)}, "
                                f"instrument for {order.instrument_id} not found.")
                continue  # Cannot resolve order

            try:
                response = await self._client.fetch_order(
                    id=order.id.value,
                    symbol=order.symbol.value,
                )
                trades = await self._client.fetch_my_trades(
                    symbol=order.symbol.value,
                    since=to_unix_time_ms(order.timestamp),
                )
                order_trades = [trade for trade in trades if trade["order"] == order.id.value]

            except betfairlightweight.BetfairError as ex:
                self._log_ccxt_error(ex, self._update_balances.__name__)
                continue
            if response is None:
                self._log.error(f"No order found for {order.id.value}.")
                continue
            # self._log.info(str(response), LogColor.BLUE)  # TODO: Development

            cum_qty = order.filled_qty.as_decimal()
            for trade in order_trades:
                execution_id = ExecutionId(str(response["id"]))
                if execution_id in order.execution_ids_c():
                    continue  # Trade already applied
                self._generate_order_filled(
                    cl_ord_id=order.cl_ord_id,
                    order_id=order.id,
                    execution_id=ExecutionId(str(response["id"])),
                    instrument_id=order.instrument_id,
                    order_side=order.side,
                    fill_qty=Decimal(f"{trade['amount']:.{instrument.size_precision}}"),
                    cum_qty=cum_qty,
                    leaves_qty=order.quantity - cum_qty,
                    avg_px=Decimal(trade["price"]),
                    commission_amount=trade["fee"]["cost"],
                    commission_currency=trade["fee"]["currency"],
                    liquidity_side=LiquiditySide.TAKER if trade["takerOrMaker"] == "taker" else LiquiditySide.MAKER,
                    timestamp=from_unix_time_ms(trade["timestamp"]),
                )

            status = response["status"]
            if status == "open":
                if cum_qty > 0:
                    order_states[order.id] = OrderState.PARTIALLY_FILLED
                    order_filled[order.id] = cum_qty
            elif status == "closed":
                order_states[order.id] = OrderState.FILLED
                order_filled[order.id] = cum_qty
            elif status == "canceled":
                order_states[order.id] = OrderState.CANCELLED
                timestamp = from_unix_time_ms(<long>response["timestamp"])
                self._generate_order_cancelled(order.cl_ord_id, order.id, timestamp)
            elif status == "expired":
                order_states[order.id] = OrderState.EXPIRED
                self._generate_order_expired(order.cl_ord_id, order.id, timestamp)

        return ExecutionStateReport(
            venue=self.venue,
            account_id=self.account_id,
            order_states=order_states,
            order_filled=order_filled,
            position_states=position_states,
        )

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
            # TODO: CCXT Pro issues for exchange.close()
            # stop_tasks.append(self._watch_balances_task)
        if self._watch_orders_task:
            self._watch_orders_task.cancel()
            # TODO: CCXT Pro issues for exchange.close()
            # stop_tasks.append(self._watch_orders_task)
        if self._watch_exec_reports_task:
            self._watch_exec_reports_task.cancel()
            # TODO: CCXT Pro issues for exchange.close()
            # stop_tasks.append(self._watch_exec_reports_task)

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

    cpdef void amend_order(self, AmendOrder command) except *:
        """
        Amend the order with parameters contained in the command.

        Parameters
        ----------
        command : AmendOrder
            The command to execute.

        """
        Condition.not_none(command, "command")

        # TODO: Implement
        self._log.error("Cannot amend orders in this version.")

    cpdef void cancel_order(self, CancelOrder command) except *:
        """
        Cancel the order with the `ClientOrderId` contained in the given command.

        Parameters
        ----------
        command : CancelOrder
            The command to execute.

        """
        Condition.not_none(command, "command")

        self._loop.create_task(self._cancel_order(command.cl_ord_id))

    # -- INTERNAL --------------------------------------------------------------------------------------

    cdef inline void _log_betfair_error(self, ex, str method_name) except *:
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
        except betfairlightweight.BetfairError as ex:
            self._log_betfair_error(ex, self._update_balances.__name__)
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
                except betfairlightweight.BetfairError as ex:
                    self._log_betfair_error(ex, self._watch_balances.__name__)
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
                except betfairlightweight.BetfairError as ex:
                    self._log_betfair_error(ex, self._watch_orders.__name__)
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
                except betfairlightweight.BetfairError as ex:
                    self._log_betfair_error(ex, self._watch_balances.__name__)
                    continue
        except asyncio.CancelledError as ex:
            self._log.debug(f"Cancelled `_watch_my_trades` for {self.account_id}.")
        except Exception as ex:
            self._log.exception(ex)

    # -- COMMANDS --------------------------------------------------------------------------------------

    async def _submit_order(self, Order order):
        self._log.debug(f"Submitted {order}.")

        # Generate event here to ensure it is processed before OrderAccepted
        self._generate_order_submitted(
            cl_ord_id=order.cl_ord_id,
            timestamp=self._clock.utc_now_c(),
        )

        try:
            # Submit order and await response
            await self._client.create_order(
                symbol=order.symbol.value,
                type=OrderTypeParser.to_str(order.type).lower(),
                side=OrderSideParser.to_str(order.side).lower(),
                amount=str(order.quantity),
                price=str(order.price) if isinstance(order, PassiveOrder) else None,
            )
        except betfairlightweight.BetfairError as ex:
            self._generate_order_rejected(
                cl_ord_id=order.cl_ord_id,
                reason=str(ex),
                timestamp=self._clock.utc_now_c(),
            )

    async def _cancel_order(self, ClientOrderId cl_ord_id):
        cdef Order order = self._engine.cache.order(cl_ord_id)
        if order is None:
            self._log.error(f"Cannot cancel order, {repr(cl_ord_id)} not found.")
            return  # Cannot cancel

        if not order.is_working_c():
            self._log.error(f"Cannot cancel order, state=OrderState.{order.state_string_c()}.")
            return  # Cannot cancel

        try:
            await self._client.cancel_order(
                id=order.id.value,
                symbol=order.symbol.value,
            )
        except betfairlightweight.BetfairError as ex:
            self._log_betfair_error(ex, self._cancel_order.__name__)
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
        cdef AccountState account_state = AccountState(
            self.account_id,
            balances,
            balances_free,
            balances_locked,
            {},
            self._uuid_factory.generate(),
            self._clock.utc_now_c(),
        )

        self._handle_event(account_state)

    cdef inline void _on_order_status(self, dict event) except *:
        cdef OrderId order_id = OrderId(event["id"])
        cdef ClientOrderId cl_ord_id = ClientOrderId(event["clientOrderId"])

        if order_id not in self._cached_orders:
            order = self._engine.cache.order(cl_ord_id)
            if order is None:
                # If state resolution has done its job this should never happen
                self._log.error(f"Cannot fill un-cached order with {repr(order_id)}.")
                return
            self._cache_order(order_id, order)

        cdef datetime timestamp = from_unix_time_ms(event["timestamp"])
        cdef str status = event["status"]
        # status == "rejected" should be captured in `submit_order`
        if status == "open" and event["filled"] == 0:
            self._generate_order_accepted(cl_ord_id, order_id, timestamp)
        elif status == "canceled":
            self._generate_order_cancelled(cl_ord_id, order_id, timestamp)
            self._decache_order(order_id)
        elif status == "expired":
            self._generate_order_expired(cl_ord_id, order_id, timestamp)
            self._decache_order(order_id)

    cdef inline void _on_exec_report(self, dict event) except *:
        cdef OrderId order_id = OrderId(event["order"])
        cdef Order order = self._cached_orders.get(order_id)

        if order is None:
            cl_ord_id = self._engine.cache.cl_ord_id(order_id)
            if cl_ord_id is None:
                self._log.error(f"Cannot fill un-cached order with {repr(order_id)}.")
                return
            order = self._engine.cache.order(cl_ord_id)
            if order is None:
                # If `reconcile_state` has done its job this should never happen
                self._log.error(f"Cannot fill un-cached order with {repr(order_id)}.")
                return
            self._cache_order(order_id, order)

        prev_cum_qty: Decimal = self._cached_filled.get(order_id)
        fill_qty: Decimal = Decimal(f"{event['amount']:.{order.quantity.precision_c()}f}")
        cum_qty: Decimal = prev_cum_qty + fill_qty
        self._cached_filled[order.cl_ord_id] = cum_qty
        leaves_qty: Decimal = order.quantity - cum_qty
        if leaves_qty == 0:
            self._decache_order(order_id)

        self._generate_order_filled(
            cl_ord_id=order.cl_ord_id,
            order_id=order_id,
            execution_id=ExecutionId(event["id"]),
            instrument_id=order.instrument_id,
            order_side=order.side,
            fill_qty=fill_qty,
            cum_qty=cum_qty,
            leaves_qty=order.quantity - cum_qty,
            avg_px=event["price"],
            commission_amount=event.get("fee", {}).get("cost", 0),
            commission_currency=event.get("fee", {}).get("currency"),
            liquidity_side=LiquiditySide.TAKER if event["takerOrMaker"] == "taker" else LiquiditySide.MAKER,
            timestamp=from_unix_time_ms(event["timestamp"]),
        )

    cdef inline void _cache_order(self, OrderId order_id, Order order) except *:
        self._cached_orders[order_id] = order
        self._cached_filled[order_id] = order.filled_qty
        self._log.debug(f"Cached {repr(order_id)} {order}.")

    cdef inline void _decache_order(self, OrderId order_id) except *:
        self._cached_orders.pop(order_id, None)
        self._cached_filled.pop(order_id, None)
        self._log.debug(f"De-cached {repr(order_id)}.")
