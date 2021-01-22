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

import ccxt
from ccxt.base.errors import BaseError as CCXTError

from nautilus_trader.adapters.ccxt.exchanges.binance cimport BinanceOrderRequestBuilder
from nautilus_trader.adapters.ccxt.exchanges.binance cimport BinanceOrderFillParser
from nautilus_trader.adapters.ccxt.exchanges.bitmex cimport BitmexOrderRequestBuilder
from nautilus_trader.adapters.ccxt.exchanges.bitmex cimport BitmexOrderFillParser
from nautilus_trader.adapters.ccxt.providers import CCXTInstrumentProvider
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LogColor
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport from_posix_ms
from nautilus_trader.model.c_enums.order_side cimport OrderSideParser
from nautilus_trader.model.c_enums.order_state cimport OrderState
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.commands cimport CancelOrder
from nautilus_trader.model.commands cimport AmendOrder
from nautilus_trader.model.commands cimport SubmitBracketOrder
from nautilus_trader.model.commands cimport SubmitOrder
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.events cimport AccountState
from nautilus_trader.model.events cimport OrderAccepted
from nautilus_trader.model.events cimport OrderCancelled
from nautilus_trader.model.events cimport OrderExpired
from nautilus_trader.model.events cimport OrderFilled
from nautilus_trader.model.events cimport OrderInvalid
from nautilus_trader.model.events cimport OrderRejected
from nautilus_trader.model.events cimport OrderSubmitted
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport ExecutionId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport OrderId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.order cimport Order
from nautilus_trader.model.order cimport PassiveOrder
from nautilus_trader.live.execution cimport LiveExecutionClient
from nautilus_trader.live.execution cimport LiveExecutionEngine

cdef int _SECONDS_IN_HOUR = 60 * 60
cdef tuple _INTEGRATED_VENUES = ("BINANCE", "BITMEX")


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
        venue_code = client.name.upper()
        # Ensure only integrated exchanges for execution
        if venue_code not in _INTEGRATED_VENUES:
            raise RuntimeError(f"{venue_code} has not been integrated in this version.")

        super().__init__(
            Venue(venue_code),
            account_id,
            engine,
            clock,
            logger,
            config={
                "name": f"CCXTExecClient-{client.name.upper()}",
            }
        )

        self._client = client
        self._instrument_provider = CCXTInstrumentProvider(
            client=client,
            load_all=False,
        )

        self.is_connected = False
        self.is_resolved = False

        self._account_last_free = {}
        self._account_last_used = {}
        self._account_last_total = {}

        # Hot cache
        self._active_orders = {}  # type: dict[ClientOrderId, Order]

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
        except CCXTError as ex:
            self._log_ccxt_error(ex, self._connect.__name__)
            return

        # Start streams
        self._watch_balances_task = self._loop.create_task(self._watch_balances())
        self._watch_orders_task = self._loop.create_task(self._watch_orders())
        self._watch_exec_reports_task = self._loop.create_task(self._watch_exec_reports())

        self.is_connected = True
        self._log.info("Connected.")

    cpdef void resolve_state(self, list active_orders) except *:
        """
        Return a state replay stream based on the given list of accepted orders
        and open positions.

        Parameters
        ----------
        active_orders : list[Order]
            The orders which currently have an active status.

        Returns
        -------
        list[Event]

        """
        Condition.not_none(active_orders, "active_orders")

        self._loop.create_task(self._resolve_state(active_orders))

    async def _resolve_state(self, list active_orders) -> None:
        """
        Resolve the execution state by comparing the given active orders from
        the execution cache with the order state from the exchange.

        Parameters
        ----------
        active_orders : list[Order]
            The orders which are active.

        """
        if not active_orders:
            self.is_resolved = True
            self._log.info("State resolved.", LogColor.GREEN)
            return  # Nothing to resolve

        cdef int count = len(active_orders)
        self._log.info(f"Resolving states for {count} "
                       f"active order{'s' if count > 1 else ''}...")

        cdef dict target_states = {}  # type: dict[ClientOrderId, OrderState]

        # TODO: Fetch open orders and compare to active_orders

        cdef Order order
        cdef str status
        for order in active_orders:
            if not order.is_active_c():
                self._log.warning(f"Order was not active, "
                                  f"was OrderState.{order.state_string_c()}.")
                continue
            if order.id.is_null():
                self._log.error(f"OrderId was not assigned for {repr(order.cl_ord_id)}, "
                                f"state is lost.")
                continue
            try:
                response = await self._client.fetch_order(order.id.value, order.symbol.code)
            except CCXTError as ex:
                self._log_ccxt_error(ex, self._update_balances.__name__)
                continue
            if response is None:
                self._log.error(f"No order found for {order.id.value}.")
                continue
            self._log.info(str(response), LogColor.BLUE)
            # TODO: Refactor below
            status = response["status"]
            if status == "canceled":
                timestamp = from_posix_ms(<long>response["timestamp"])
                self._generate_order_cancelled(order.cl_ord_id, order.id, timestamp)
                target_states[order.cl_ord_id] = OrderState.CANCELLED
            elif status == "closed":
                filled_event = {
                    "exec_id": str(response["timestamp"]),  # TODO: Transaction time for now
                    "symbol": response["symbol"],
                    "fill_qty": Decimal(response["filled"]) - order.filled_qty,
                    "cum_qty": response["filled"],
                    "avg_px": response["average"],
                    "liquidity_side": LiquiditySide.TAKER,  # TODO: Implement
                    "commission": None,
                    "commission_currency": None,
                    "timestamp": response["timestamp"],
                }
                self._generate_order_filled(order.cl_ord_id, order.id, filled_event)
                target_states[order.cl_ord_id] = OrderState.FILLED
            elif status == "expired":
                self._generate_order_expired(order.cl_ord_id, order.id, timestamp)
                target_states[order.cl_ord_id] = OrderState.EXPIRED

        while True:
            await asyncio.sleep(0.1)
            for order in active_orders:
                order_state = target_states.get(order.cl_ord_id)
                if order_state is None:
                    continue
                if not order.state_c() != target_states[order.cl_ord_id]:
                    continue
            break

        self._log.info("State resolved.", LogColor.GREEN)
        self.is_resolved = True

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
        self.is_resolved = False
        self._log.info("Disconnected.")

    cpdef void reset(self) except *:
        """
        Reset the client.
        """
        if self.is_connected:
            self._log.error("Cannot reset a connected execution client.")
            return

        self._log.info("Resetting...")

        self._instrument_provider = CCXTInstrumentProvider(
            client=self._client,
            load_all=False,
        )

        self._account_last_free.clear()
        self._account_last_used.clear()
        self._account_last_total.clear()
        self._active_orders.clear()

        self._log.info("Reset.")

    cpdef void dispose(self) except *:
        """
        Dispose the client.
        """
        if self.is_connected:
            self._log.error("Cannot dispose a connected execution client.")
            return

        self._log.info("Disposing...")

        # Nothing to dispose yet
        self._log.info("Disposed.")

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
        cdef dict event
        cdef dict event0
        try:
            while True:
                try:
                    # events type is ArrayCacheBySymbolById
                    events = await self._client.watch_orders()
                    event0 = events[0]
                    event = event0["info"]
                    event["symbol"] = event0["symbol"]  # Replace for symbol with '/'
                    event["timestamp"] = event0["timestamp"]
                    # TODO: Development
                    # self._log.info("Raw: " + str(event), LogColor.BLUE)
                    self._on_order_status(event)
                except CCXTError as ex:
                    self._log_ccxt_error(ex, self._watch_orders.__name__)
                    continue
        except asyncio.CancelledError as ex:
            self._log.debug(f"Cancelled `_watch_orders`.")
        except Exception as ex:
            self._log.exception(ex)

    async def _watch_exec_reports(self):
        cdef dict event0
        cdef dict event
        try:
            while True:
                try:
                    # events type is ArrayCacheBySymbolById
                    events = await self._client.watch_my_trades()
                    event0 = events[0]
                    event = event0["info"]
                    event["symbol"] = event0["symbol"]  # Replace with `/` symbol
                    event["timestamp"] = event0["timestamp"]
                    # TODO: Development
                    # self._log.info("Raw: " + str(event), LogColor.GREEN)
                    self._on_exec_report(event)
                except CCXTError as ex:
                    self._log_ccxt_error(ex, self._watch_balances.__name__)
                    continue
        except asyncio.CancelledError as ex:
            self._log.debug(f"Cancelled `_watch_my_trades` for {self.account_id}.")
        except Exception as ex:
            self._log.exception(ex)

# -- COMMANDS --------------------------------------------------------------------------------------

    async def _submit_order(self, Order order):
        # Common arguments
        cdef str symbol = order.symbol.code
        cdef str order_type  # Assign for specific API
        cdef str order_side  # Assign for specific API
        cdef str quantity = str(order.quantity)
        cdef str price = str(order.price) if isinstance(order, PassiveOrder) else None
        cdef dict params     # Assign for specific API
        try:
            # Exchange specific arguments
            # Eventually refactor the below into separate classes
            if self.venue.value == "BINANCE":
                params = BinanceOrderRequestBuilder.build(order)
                order_type = params["type"]
                order_side = OrderSideParser.to_str(order.side)
            elif self.venue.value == "BITMEX":
                params = BitmexOrderRequestBuilder.build(order)
                order_type = params["type"]
                order_side = OrderSideParser.to_str(order.side).capitalize()
        except ValueError as ex:
            self._generate_order_invalid(order.cl_ord_id, str(ex))
            return

        self._log.debug(f"Submitted {order}.")
        # Generate event here to ensure it is processed before OrderAccepted
        self._generate_order_submitted(
            order.cl_ord_id,
            self._clock.utc_now_c(),
        )

        self._active_orders[order.cl_ord_id] = order

        try:
            # Submit order and await response
            await self._client.create_order(
                symbol=symbol,
                type=order_type,
                side=order_side,
                amount=quantity,
                price=price,
                params=params,
            )
        except CCXTError as ex:
            self._generate_order_rejected(order.cl_ord_id, str(ex), self._clock.utc_now_c())
            return

    async def _cancel_order(self, ClientOrderId cl_ord_id):
        cdef Order order = self._engine.cache.order(cl_ord_id)
        if order is None:
            self._log.error(f"Cannot cancel order, {repr(cl_ord_id)} not found.")
            return  # Cannot cancel

        if not order.is_working_c():
            self._log.error(f"Cannot cancel order, state=OrderState.{order.state_string_c()}.")
            return  # Cannot cancel

        try:
            await self._client.cancel_order(order.id.value, order.symbol.code)
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
        if self.venue.value == "BINANCE":
            self._on_binance_order_status(event)
        elif self.venue.value == "BITMEX":
            self._on_bitmex_order_status(event)

    cdef inline void _on_exec_report(self, dict event) except *:
        if self.venue.value == "BINANCE":
            self._on_binance_exec_report(event)
        elif self.venue.value == "BITMEX":
            self._on_bitmex_exec_report(event)

    cdef inline void _on_binance_order_status(self, dict event) except *:
        cdef OrderId order_id = OrderId(str(event["i"]))
        cdef datetime timestamp = from_posix_ms(event["E"])  # Event time (generic for now)
        cdef str exec_type = event["x"]
        if exec_type == "NEW":
            cl_ord_id = ClientOrderId(event["c"])  # ClientOrderId
            self._generate_order_accepted(cl_ord_id, order_id, timestamp)
        elif exec_type == "CANCELED":
            cl_ord_id = ClientOrderId(event["C"])  # Original ClientOrderId
            self._generate_order_cancelled(cl_ord_id, order_id, timestamp)
            self._active_orders.pop(cl_ord_id)
        elif exec_type == "EXPIRED":
            cl_ord_id = ClientOrderId(event["c"])  # ClientOrderId
            self._generate_order_expired(cl_ord_id, order_id, timestamp)
            self._active_orders.pop(cl_ord_id)

    cdef inline void _on_binance_exec_report(self, dict event) except *:
        cdef OrderId order_id = OrderId(str(event["i"]))
        cdef datetime timestamp = from_posix_ms(event["E"])  # Event time (generic for now)
        cdef str exec_type = event["x"]
        if exec_type == "TRADE":
            cl_ord_id = ClientOrderId(event["c"])  # ClientOrderId
            fill_info = BinanceOrderFillParser.parse(event)
            self._generate_order_filled(cl_ord_id, order_id, fill_info)

    cdef inline void _on_bitmex_order_status(self, dict event) except *:
        cdef str cl_ord_id_str = event["clOrdID"]
        if cl_ord_id_str == '':  # Sent from website or otherwise not supplied
            cl_ord_id_str = "NULL"
        cdef ClientOrderId cl_ord_id = ClientOrderId(cl_ord_id_str)
        cdef OrderId order_id = OrderId(event["orderID"])
        cdef datetime timestamp = from_posix_ms(event["timestamp"])  # Event time (generic for now)
        cdef str ord_status = event["ordStatus"]
        if ord_status == "New":
            self._generate_order_accepted(cl_ord_id, order_id, timestamp)
        elif ord_status == "Canceled":
            self._generate_order_cancelled(cl_ord_id, order_id, timestamp)
            self._active_orders.pop(cl_ord_id, None)
        elif ord_status == "Rejected":
            self._generate_order_rejected(cl_ord_id, order_id, timestamp)
            self._active_orders.pop(cl_ord_id, None)

    cdef inline void _on_bitmex_exec_report(self, dict event) except *:
        cdef str cl_ord_id_str = event["clOrdID"]
        if cl_ord_id_str == '':  # Sent from website or otherwise not supplied
            cl_ord_id_str = "NULL"
        cdef ClientOrderId cl_ord_id = ClientOrderId(cl_ord_id_str)
        cdef OrderId order_id = OrderId(event["orderID"])
        cdef datetime timestamp = from_posix_ms(event["timestamp"])  # Event time (generic for now)
        if event["execType"] == "Trade":
            fill_info = BitmexOrderFillParser.parse(event)
            self._generate_order_filled(cl_ord_id, order_id, fill_info)
            self._active_orders.pop(cl_ord_id, None)

    cdef inline void _generate_order_invalid(
        self,
        ClientOrderId cl_ord_id,
        str reason,
    ) except *:
        # Generate event
        cdef OrderInvalid invalid = OrderInvalid(
            cl_ord_id,
            reason,
            self._uuid_factory.generate(),
            self._clock.utc_now_c(),
        )
        self._handle_event(invalid)

    cdef inline void _generate_order_submitted(
        self, ClientOrderId cl_ord_id,
        datetime timestamp,
    ) except *:
        # Generate event
        cdef OrderSubmitted submitted = OrderSubmitted(
            self.account_id,
            cl_ord_id,
            timestamp,
            self._uuid_factory.generate(),
            timestamp,
        )
        self._handle_event(submitted)

    cdef inline void _generate_order_rejected(
        self,
        ClientOrderId cl_ord_id,
        str reason,
        datetime timestamp,
    ) except *:
        # Generate event
        cdef OrderRejected rejected = OrderRejected(
            self.account_id,
            cl_ord_id,
            timestamp,
            reason,
            self._uuid_factory.generate(),
            self._clock.utc_now_c(),
        )
        self._handle_event(rejected)

    cdef inline void _generate_order_accepted(
        self,
        ClientOrderId cl_ord_id,
        OrderId order_id,
        datetime timestamp,
    ) except *:
        # Generate event
        cdef OrderAccepted accepted = OrderAccepted(
            self.account_id,
            cl_ord_id,
            order_id,
            timestamp,
            self._uuid_factory.generate(),
            self._clock.utc_now_c(),
        )
        self._handle_event(accepted)

    cdef inline void _generate_order_filled(
        self,
        ClientOrderId cl_ord_id,
        OrderId order_id,
        dict event,
    ) except *:
        cdef Instrument instrument = self._instrument_provider.get_c(event["symbol"])
        if instrument is None:
            self._log.error(f"Cannot fill order with {repr(order_id)}, "
                            f"instrument for {event['symbol']} not found.")
            return  # Cannot fill order

        # Fetch order from hot cache
        cdef Order order = self._active_orders.get(cl_ord_id)
        if order is None:
            # Fetch order from execution engines cache
            order = self._engine.cache.order(cl_ord_id)
            if order is None:
                self._log.error(f"Cannot fill order for {repr(cl_ord_id)}, "
                                f"{repr(order_id)} not found in cache.")
                return  # Cannot fill order

        # Determine commission
        cdef str currency_str = event["commission_currency"]
        cdef Money commission = None
        cdef Currency currency = None
        if currency_str is None:
            commission = Money(0, instrument.quote_currency)
        else:
            currency = self._instrument_provider.currency(currency_str)
            if currency is None:
                self._log.error(f"Cannot determine commission for {repr(order_id)}, "
                                f"currency for {currency_str} not found.")
                commission = Money(0, instrument.quote_currency)
            else:
                commission = Money(event["commission"], currency)

        # Determine position identifier
        cdef PositionId position_id = self._engine.cache.position_id(order.cl_ord_id)
        if position_id is None:
            position_id = PositionId.null_c()

        # Determine quantities (we have to do it per exchange for now)
        cdef Quantity fill_qty = Quantity(event["fill_qty"], instrument.size_precision)
        cdef Quantity cum_qty = Quantity(event["cum_qty"], instrument.size_precision)
        cdef Quantity leaves_qty = Quantity(order.quantity - cum_qty, instrument.size_precision)
        if leaves_qty == 0:
            self._active_orders.pop(cl_ord_id, None)

        # POSIX timestamp in milliseconds
        cdef long timestamp = <long>event["timestamp"]

        # Generate event
        cdef OrderFilled filled = OrderFilled(
            self.account_id,
            order.cl_ord_id,
            order_id,
            ExecutionId(event["exec_id"]),
            position_id,
            order.strategy_id,
            order.symbol,
            order.side,
            fill_qty,
            cum_qty,
            leaves_qty,
            Price(event["avg_px"], instrument.price_precision),
            instrument.quote_currency,
            instrument.is_inverse,
            commission,
            event["liquidity_side"],
            from_posix_ms(timestamp),
            self._uuid_factory.generate(),
            self._clock.utc_now_c(),
        )

        self._handle_event(filled)

    cdef inline void _generate_order_cancelled(
        self,
        ClientOrderId cl_ord_id,
        OrderId order_id,
        datetime timestamp,
    ) except *:
        # Generate event
        cdef OrderCancelled cancelled = OrderCancelled(
            self.account_id,
            cl_ord_id,
            order_id,
            timestamp,
            self._uuid_factory.generate(),
            self._clock.utc_now_c(),
        )

        self._handle_event(cancelled)

    cdef inline void _generate_order_expired(
        self,
        ClientOrderId cl_ord_id,
        OrderId order_id,
        datetime timestamp,
    ) except *:
        # Generate event
        cdef OrderExpired expired = OrderExpired(
            self.account_id,
            cl_ord_id,
            order_id,
            timestamp,
            self._uuid_factory.generate(),
            self._clock.utc_now_c(),
        )

        self._handle_event(expired)
