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
from cpython.datetime cimport datetime

import ccxt
from ccxt.base.errors import BaseError as CCXTError

from nautilus_trader.adapters.ccxt.providers import CCXTInstrumentProvider
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport from_posix_ms
from nautilus_trader.model.c_enums.order_side cimport OrderSideParser
from nautilus_trader.model.c_enums.order_type cimport OrderType
from nautilus_trader.model.c_enums.order_type cimport OrderTypeParser
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.commands cimport CancelOrder
from nautilus_trader.model.commands cimport AmendOrder
from nautilus_trader.model.commands cimport SubmitBracketOrder
from nautilus_trader.model.commands cimport SubmitOrder
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.events cimport AccountState
from nautilus_trader.model.events cimport OrderAccepted
from nautilus_trader.model.events cimport OrderCancelled
from nautilus_trader.model.events cimport OrderDenied
from nautilus_trader.model.events cimport OrderFilled
from nautilus_trader.model.events cimport OrderRejected
from nautilus_trader.model.events cimport OrderSubmitted
from nautilus_trader.model.events cimport OrderWorking
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
        super().__init__(
            Venue(client.name.upper()),
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

        self._order_id_index = {}    # type: dict[OrderId, ClientOrderId]
        self._event_buffer = {}      # type: dict[OrderId, list]

        # Scheduled tasks
        self._update_instruments_task = None

        # Streaming tasks
        self._watch_balances_task = None
        self._watch_orders_task = None
        self._watch_my_trades_task = None

    cpdef void connect(self) except *:
        """
        Connect the client.
        """
        self._log.info("Connecting...")

        if self._client.check_required_credentials():
            self._log.info("API credentials validated.")
        else:
            self._log.error("API credentials missing or invalid.")
            self._log.error(f"Required: {self._client.required_credentials()}.")

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
        # self._watch_my_trades_task = self._loop.create_task(self._watch_my_trades())

        self.is_connected = True
        self._log.info("Connected.")

    cpdef void disconnect(self) except *:
        """
        Disconnect the client.
        """
        self._loop.create_task(self._disconnect())

    async def _disconnect(self):
        self._log.info("Disconnecting...")

        stop_tasks = []
        # Cancel update instruments
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
        if self._watch_my_trades_task:
            self._watch_my_trades_task.cancel()
            # TODO: CCXT Pro issues for exchange.close()
            # stop_tasks.append(self._watch_my_trades_task)

        # Wait for all tasks to complete
        if stop_tasks:
            await asyncio.gather(*stop_tasks)

        # Ensure ccxt closed
        self._log.info("Closing WebSocket(s)...")
        await self._client.close()

        self.is_connected = False
        self._log.info("Disconnected.")

    cpdef void reset(self) except *:
        """
        Reset the client.
        """
        if self.is_connected:
            self._log.error("Cannot reset a connected execution client.")
            return

        self._log.info("Resetting...")

        # TODO: Reset client
        self._instrument_provider = CCXTInstrumentProvider(
            client=self._client,
            load_all=False,
        )

        self._order_id_index = {}
        self._event_buffer = {}

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
        self._log.error(f"{type(ex).__name__}: {ex} in {method_name}")

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
        if not self._client.has["fetchBalance"]:
            self._log.error("`fetch_balance` not available.")
            return

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
        if not self._client.has["watchBalance"]:
            self._log.error("`watch_balance` not available.")
            return

        cdef dict params = {'type': 'spot'}  # TODO: Hard coded to spot account for now
        cdef dict response
        cdef bint exiting = False  # Flag to stop loop
        try:
            while True:
                try:
                    response = await self._client.watch_balance(params)
                except CCXTError as ex:
                    self._log_ccxt_error(ex, self._watch_balances.__name__)
                    continue
                except TypeError:
                    # Temporary workaround for testing
                    response = self._client.watch_balance
                    exiting = True

                if response is None:
                    return  # TODO: Temporary workaround for testing

                self._on_account_state(response)

                if exiting:
                    break
        except asyncio.CancelledError as ex:
            self._log.debug(f"Cancelled `_watch_balances` for {self.account_id}.")
        except Exception as ex:
            self._log.exception(ex)

    async def _watch_orders(self):
        if not self._client.has["watchOrders"]:
            self._log.error("`watch_orders` not available.")
            return

        cdef bint exiting = False  # Flag to stop loop
        cdef dict event
        cdef ClientOrderId cl_ord_id
        cdef OrderId order_id
        try:
            while True:
                try:
                    # ArrayCacheBySymbolById
                    response = await self._client.watch_orders()
                except CCXTError as ex:
                    self._log_ccxt_error(ex, self._watch_orders.__name__)
                    continue
                except TypeError:
                    # Temporary workaround for testing
                    response = self._client.watch_orders
                    exiting = True

                if response is None:
                    return  # TODO: Temporary workaround for testing

                # CCXTCache is set to 1 so expecting one response at a time
                event = response[0]
                order_id = OrderId(event["id"])
                self._check_and_process_order_event(order_id, event)

                if exiting:
                    break
        except asyncio.CancelledError as ex:
            self._log.debug(f"Cancelled `_watch_orders`.")
        except Exception as ex:
            self._log.exception(ex)

    async def _watch_my_trades(self):
        if not self._client.has["watchMyTrades"]:
            self._log.error("`watch_my_trades` not available.")
            return

        cdef dict response
        cdef bint exiting = False  # Flag to stop loop
        try:
            while True:
                try:
                    response = await self._client.watch_my_trades()
                except CCXTError as ex:
                    self._log_ccxt_error(ex, self._watch_my_trades.__name__)
                    continue
                except TypeError:
                    # Temporary workaround for testing
                    response = self._client.watch_my_trades
                    exiting = True

                # TODO: Development
                # self._log.critical("_watch_my_trades ran!")
                # with open('res_watch_my_trades.json', 'w') as json_file:
                #     json.dump(response, json_file)

                if exiting:
                    break
        except asyncio.CancelledError as ex:
            self._log.debug(f"Cancelled `_watch_my_trades`.")
        except Exception as ex:
            self._log.exception(ex)

# -- COMMANDS --------------------------------------------------------------------------------------

    async def _submit_order(self, Order order):
        self._log.info(f"Submitting {order}...")
        self._generate_order_submitted(order.cl_ord_id, self._clock.utc_now())

        cdef str order_type = OrderTypeParser.to_str(order.type).lower()
        cdef str order_side = OrderSideParser.to_str(order.side).lower()
        cdef str order_qty = str(order.quantity)

        cdef dict params = {
            "recvWindow": 10000  # TODO: Server time sync issue?
        }

        # Add ClientOrderId to request if possible
        if self.venue.value == "BINANCE":
            params["newClientOrderId"] = order.cl_ord_id.value
        elif self.venue.value == "BITMEX":
            params["clOrdId"] = order.cl_ord_id.value

        cdef dict response
        try:
            if order.type == OrderType.MARKET:
                response = await self._client.create_order(
                    order.symbol.code,
                    order_type,
                    order_side,
                    order_qty,
                    params=params,
                )
            elif order.type == OrderType.LIMIT:
                response = await self._client.create_order(
                    order.symbol.code,
                    order_type,
                    order_side,
                    order_qty,
                    str(order.price),
                    params=params,
                )
            else:
                self._generate_order_denied(
                    order.cl_ord_id,
                    f"OrderType.{OrderTypeParser.to_str(order.type)} "
                    f"not supported by the exchange.")
                return
        except CCXTError as ex:
            self._generate_order_rejected(order.cl_ord_id, str(ex))
            return

        cdef OrderId order_id = OrderId(response["id"])
        self._generate_order_accepted(order.cl_ord_id, order_id, response)

        # Index order_id
        self._order_id_index[order_id] = order.cl_ord_id
        cdef list events = self._event_buffer.pop(order_id, None)
        cdef dict event
        if events:
            # Process buffered events which are any events received before the
            # order_id was indexed. We must wait for the above response with the
            # order_id to ensure order management will work for any exchange
            # regardless of whether a ClientOrderId was able to be set for the order.
            for event in events:
                self._process_order_event(order.cl_ord_id, order_id, event)

    async def _cancel_order(self, ClientOrderId cl_ord_id):
        cdef Order order = self._engine.cache.order(cl_ord_id)
        if order is None:
            self._log.error(f"Cannot cancel order, order for {cl_ord_id} not found.")
            return  # Cannot cancel

        if not order.is_working_c():
            self._log.error(f"Cannot cancel order, order is not working.")
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

        cdef str code
        cdef double amount
        cdef Currency currency

        # Update total balances
        for code, amount in event["total"].items():
            if amount == 0:
                continue
            currency = self._instrument_provider.currency(code)
            if currency is None:
                self._log.error(f"Cannot update total balance for {code} "
                                f"(no currency loaded).")
            balances.append(Money(amount, currency))

        # Update free balances
        for code, amount in event["free"].items():
            if amount == 0:
                continue
            currency = self._instrument_provider.currency(code)
            if currency is None:
                self._log.error(f"Cannot update total balance for {code} "
                                f"(no currency loaded).")
            balances_free.append(Money(amount, currency))

        # Update locked balances
        for code, amount in event["used"].items():
            if amount == 0:
                continue
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
            self._clock.utc_now(),
        )

        self._handle_event(account_state)

    cdef inline void _check_and_process_order_event(self, OrderId order_id, dict event) except *:
        cdef ClientOrderId cl_ord_id = self._order_id_index.get(order_id)
        cdef list events
        if cl_ord_id is None:
            events = self._event_buffer.get(order_id)
            if events is None:
                self._event_buffer[order_id] = [event]
            else:
                events.append(event)
            # Order event received before OrderId was indexed
            # and OrderAccepted event generated.
            return

        self._process_order_event(cl_ord_id, order_id, event)

    cdef inline void _process_order_event(
        self,
        ClientOrderId cl_ord_id,
        OrderId order_id,
        dict event,
    ) except *:
        # Determine event
        if event["filled"] > 0:
            self._generate_order_filled(cl_ord_id, order_id, event)
            return

        cdef str status = event["status"]
        if status == "open":
            self._generate_order_working(cl_ord_id, order_id, event)
        elif status == "canceled":
            self._generate_order_cancelled(cl_ord_id, order_id, event)
        else:
            # TODO: Development
            self._log.critical(str(event))

    cdef inline void _generate_order_denied(
        self,
        ClientOrderId cl_ord_id,
        str reason,
    ) except *:
        # Generate event
        cdef OrderDenied denied = OrderDenied(
            cl_ord_id,
            reason,
            self._uuid_factory.generate(),
            self._clock.utc_now(),
        )
        self._handle_event(denied)

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
            self._clock.utc_now(),
        )
        self._handle_event(submitted)

    cdef inline void _generate_order_rejected(
        self,
        ClientOrderId cl_ord_id,
        str reason,
    ) except *:
        # Generate event
        cdef OrderRejected rejected = OrderRejected(
            self.account_id,
            cl_ord_id,
            self._clock.utc_now(),
            reason,
            self._uuid_factory.generate(),
            self._clock.utc_now(),
        )
        self._handle_event(rejected)

    cdef inline void _generate_order_accepted(
        self,
        ClientOrderId cl_ord_id,
        OrderId order_id,
        dict event,
    ) except *:
        # Generate event
        cdef OrderAccepted accepted = OrderAccepted(
            self.account_id,
            cl_ord_id,
            order_id,
            from_posix_ms(event["timestamp"]),
            self._uuid_factory.generate(),
            self._clock.utc_now(),
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
            self._log.error(f"Cannot fill order with id {order_id}, "
                            f"instrument for {event['symbol']} not found.")
            return  # Cannot fill order

        # Fetch order from cache
        cdef Order order = self._engine.cache.order(cl_ord_id)
        if order is None:
            self._log.error(f"Cannot fill order for cl_ord_id {cl_ord_id}, "
                            f"order_id {order_id} not found in cache.")
            return  # Cannot fill order

        # Determine commission
        cdef Money commission = None
        cdef Currency currency = None
        cdef dict fees = event.get("fee")
        if fees is None:
            commission = Money(0, instrument.quote_currency)
        else:
            currency = self._instrument_provider.currency(fees["currency"])
            if currency is None:
                self._log.error(f"Cannot determine commission for {order_id}, "
                                f"currency for {fees['currency']} not found.")
                commission = Money(0, instrument.quote_currency)
            else:
                commission = Money(fees["cost"], currency)

        # Determine position identifier
        cdef PositionId position_id = self._engine.cache.position_id(order.cl_ord_id)
        if position_id is None:
            position_id = PositionId.null_c()

        # Generate event
        cdef OrderFilled filled = OrderFilled(
            self.account_id,
            order.cl_ord_id,
            order_id,
            ExecutionId("1"),  # TODO: Implement
            position_id,
            order.strategy_id,
            order.symbol,
            order.side,
            Quantity(event["amount"], instrument.size_precision),     # Filled
            Quantity(event["filled"], instrument.size_precision),     # Cumulative
            Quantity(event["remaining"], instrument.size_precision),  # Remaining
            Price(event["average"], instrument.price_precision),
            instrument.quote_currency,
            instrument.is_inverse,
            commission,
            LiquiditySide.TAKER if order.type != OrderType.LIMIT else LiquiditySide.MAKER,  # TODO: Implement
            from_posix_ms(event["timestamp"]),
            self._uuid_factory.generate(),
            self._clock.utc_now(),
        )

        self._handle_event(filled)

    cdef inline void _generate_order_working(
        self,
        ClientOrderId cl_ord_id,
        OrderId order_id,
        dict event,
    ) except *:
        # Fetch order from cache
        cdef Order order = self._engine.cache.order(cl_ord_id)
        if order is None:
            self._log.error(f"Cannot fill order for cl_ord_id {cl_ord_id}, "
                            f"order_id {order_id} not found in cache.")
            return  # Cannot fill order

        if not isinstance(order, PassiveOrder):
            return  # MARKET order

        # Generate event
        cdef OrderWorking working = OrderWorking(
            self.account_id,
            order.cl_ord_id,
            order_id,
            order.symbol,
            order.side,
            order.type,
            order.quantity,
            order.price,
            order.time_in_force,  # TODO: Implement
            order.expire_time,    # TODO: Implement
            self._clock.utc_now(),
            self._uuid_factory.generate(),
            self._clock.utc_now(),
        )

        self._handle_event(working)

    cdef inline void _generate_order_cancelled(
        self,
        ClientOrderId cl_ord_id,
        OrderId order_id,
        dict event,
    ) except *:
        # Generate event
        cdef OrderCancelled cancelled = OrderCancelled(
            self.account_id,
            cl_ord_id,
            order_id,
            from_posix_ms(event["timestamp"]),
            self._uuid_factory.generate(),
            self._clock.utc_now(),
        )

        self._handle_event(cancelled)
