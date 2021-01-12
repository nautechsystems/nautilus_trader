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
import json

from nautilus_trader.adapters.ccxt.providers import CCXTInstrumentProvider
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logging cimport CMD
from nautilus_trader.common.logging cimport EVT
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport RECV
from nautilus_trader.common.logging cimport SENT
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport from_posix_ms
from nautilus_trader.model.c_enums.currency_type cimport CurrencyType
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.order_side cimport OrderSideParser
from nautilus_trader.model.c_enums.order_type cimport OrderType
from nautilus_trader.model.c_enums.order_type cimport OrderTypeParser
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySideParser
from nautilus_trader.model.commands cimport CancelOrder
from nautilus_trader.model.commands cimport ModifyOrder
from nautilus_trader.model.commands cimport SubmitBracketOrder
from nautilus_trader.model.commands cimport SubmitOrder
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.events cimport AccountState
from nautilus_trader.model.events cimport OrderAccepted
from nautilus_trader.model.events cimport OrderCancelReject
from nautilus_trader.model.events cimport OrderCancelled
from nautilus_trader.model.events cimport OrderDenied
from nautilus_trader.model.events cimport OrderExpired
from nautilus_trader.model.events cimport OrderFilled
from nautilus_trader.model.events cimport OrderModified
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
        self._is_connected = False

        # Scheduled tasks
        self._update_instruments_task = None

        # Streaming tasks
        self._watch_balances_task = None
        self._watch_orders_task = None
        # self._watch_create_order_task = None
        # self._watch_cancel_order_task = None
        self._watch_my_trades_task = None

        self._processing_orders = {}  # type: dict[OrderId, Order]

        self._counter = 0  # TODO: Development only

    cpdef bint is_connected(self) except *:
        """
        Return a value indicating whether the client is connected.

        Returns
        -------
        bool
            True if connected, else False.

        """
        return self._is_connected

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
        update = self._run_after_delay(delay, self._instruments_update(delay))
        self._update_instruments_task = self._loop.create_task(update)

        self._loop.create_task(self._connect())

    async def _connect(self):
        try:
            await self._load_instruments()
            await self._update_balances()
        except Exception as ex:
            self._log.error(f"{type(ex).__name__}: {ex} in _connect")
            return

        # Start streams
        self._watch_balances_task = self._loop.create_task(self._watch_balances())
        self._watch_orders_task = self._loop.create_task(self._watch_orders())
        # self._watch_create_order_task = self._loop.create_task(self._watch_create_order())
        # self._watch_cancel_order_task = self._loop.create_task(self._watch_cancel_order())
        # self._watch_my_trades_task = self._loop.create_task(self._watch_my_trades())

        self._is_connected = True
        self.initialized = True

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
        if self._watch_create_order_task:
            self._watch_create_order_task.cancel()
            # TODO: CCXT Pro issues for exchange.close()
            # stop_tasks.append(self._watch_create_order_task)
        if self._watch_cancel_order_task:
            self._watch_cancel_order_task.cancel()
            # TODO: CCXT Pro issues for exchange.close()
            # stop_tasks.append(self._watch_cancel_order_task)
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

        self._is_connected = False

        self._log.info("Disconnected.")

    cpdef void reset(self) except *:
        """
        Reset the client.
        """
        if self._is_connected:
            self._log.error("Cannot reset a connected execution client.")
            return

        self._log.info("Resetting...")

        # TODO: Reset client
        self._instrument_provider = CCXTInstrumentProvider(
            client=self._client,
            load_all=False,
        )

        self._processing_orders = {}  # type: dict[OrderId, Order]

        self._log.info("Reset.")

    cpdef void dispose(self) except *:
        """
        Dispose the client.
        """
        if self._is_connected:
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

    async def _submit_order(self, Order order):
        # TODO: Check order type and deny if not of supported type
        cdef str order_type = OrderTypeParser.to_str(order.type)
        cdef str order_side = OrderSideParser.to_str(order.side)

        self._log.info(f"Sending {order}...")

        cdef datetime submitted_time = self._clock.utc_now()

        # Submit order and await response
        cdef dict response
        try:
            response = await self._client.create_order(
                order.symbol.code,
                order_type,
                order_side,
                str(order.quantity),
            )
        except Exception as ex:
            self._generate_order_submitted(order.cl_ord_id, submitted_time)
            self._generate_order_rejected(order, str(ex))
            return

        self._generate_order_submitted(order.cl_ord_id, submitted_time)
        self._generate_order_accepted(
            order,
            OrderId(response["id"]),
            from_posix_ms(response["timestamp"]),
        )

        # TODO!
        with open('res_create_limit_order.json', 'w') as json_file:
            json.dump(response, json_file)

    cdef inline void _generate_order_submitted(
        self,
        ClientOrderId cl_ord_id,
        datetime submitted_time,
    ) except *:
        # Generate event
        cdef OrderSubmitted submitted = OrderSubmitted(
            self.account_id,
            cl_ord_id,
            self._clock.utc_now(),
            self._uuid_factory.generate(),
            self._clock.utc_now(),
        )
        self._handle_event(submitted)

    cdef inline void _generate_order_rejected(self, Order order, str reason) except *:
        # Generate event
        cdef OrderRejected rejected = OrderRejected(
            self.account_id,
            order.cl_ord_id,
            self._clock.utc_now(),
            reason,
            self._uuid_factory.generate(),
            self._clock.utc_now(),
        )
        self._handle_event(rejected)

    cdef inline void _generate_order_accepted(
        self,
        Order order,
        OrderId order_id,
        datetime accepted_time,
    ) except *:
        self._processing_orders[order_id] = order

        # Generate event
        cdef OrderAccepted accepted = OrderAccepted(
            self.account_id,
            order.cl_ord_id,
            order_id,
            self._clock.utc_now(),
            self._uuid_factory.generate(),
            self._clock.utc_now(),
        )
        self._handle_event(accepted)

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

    cpdef void modify_order(self, ModifyOrder command) except *:
        """
        Modify the order with parameters contained in the command.

        Parameters
        ----------
        command : ModifyOrder
            The command to execute.

        """
        Condition.not_none(command, "command")

        self._log.error("Cannot modify orders in this version.")

    cpdef void cancel_order(self, CancelOrder command) except *:
        """
        Cancel the order with the `ClientOrderId` contained in the given command.

        Parameters
        ----------
        command : CancelOrder
            The command to execute.

        """
        Condition.not_none(command, "command")

        self._client.cancel_order(command.cl_ord_id.value)

        self._log.debug(f"{CMD}{SENT} {command}.")

    async def _run_after_delay(self, double delay, coro):
        await asyncio.sleep(delay)
        return await coro

    async def _load_instruments(self):
        await self._instrument_provider.load_all_async()
        self._log.info(f"Updated {self._instrument_provider.count} instruments.")

    async def _instruments_update(self, delay):
        await self._load_instruments()

        # Reschedule instruments update
        update = self._run_after_delay(delay, self._instruments_update(delay))
        self._update_instruments_task = self._loop.create_task(update)

    async def _update_balances(self):
        if not self._client.has["fetchBalance"]:
            self._log.error("`fetch_balance` not available.")
            return

        try:
            response = await self._client.fetch_balance({'type': 'spot'})
        except TypeError:
            # Temporary workaround for testing
            response = self._client.fetch_balance
        except Exception as ex:
            self._log.error(f"{type(ex).__name__}: {ex} in _update_balances")
            return

        self._on_account_state(response)

    async def _watch_balances(self):
        if not self._client.has["watchBalance"]:
            self._log.error("`watch_balance` not available.")
            return

        cdef dict params = {'type': 'spot'}  # TODO: Hard coded for now
        cdef dict response
        cdef bint exiting = False  # Flag to stop loop
        try:
            while True:
                try:
                    response = await self._client.watch_balance(params)
                except TypeError:
                    # Temporary workaround for testing
                    response = self._client.watch_balance
                    exiting = True

                if response is None:
                    self._log.critical("Why None?")  # TODO!
                if response:
                    self._on_account_state(response)

                if exiting:
                    break
        except asyncio.CancelledError as ex:
            self._log.debug(f"Cancelled `_watch_balances` for {self.account_id}.")
        except Exception as ex:
            self._log.error(f"{type(ex).__name__}: {ex} in _watch_balances")

    cdef inline void _on_account_state(self, dict response) except *:
        cdef list balances = []
        cdef list balances_free = []
        cdef list balances_locked = []

        cdef str code
        cdef double amount
        cdef Currency currency

        # Update total balances
        for code, amount in response["total"].items():
            if amount == 0:
                continue
            currency = self._instrument_provider.currency(code)
            if currency is None:
                self._log.error(f"Cannot update total balance for {code} "
                                f"(no currency loaded).")
            balances.append(Money(amount, currency))

        # Update free balances
        for code, amount in response["free"].items():
            if amount == 0:
                continue
            currency = self._instrument_provider.currency(code)
            if currency is None:
                self._log.error(f"Cannot update total balance for {code} "
                                f"(no currency loaded).")
            balances_free.append(Money(amount, currency))

        # Update locked balances
        for code, amount in response["used"].items():
            if amount == 0:
                continue
            currency = self._instrument_provider.currency(code)
            if currency is None:
                self._log.error(f"Cannot update total balance for {code} "
                                f"(no currency loaded).")
            balances_locked.append(Money(amount, currency))

        # Generate event
        cdef AccountState event = AccountState(
            self.account_id,
            balances,
            balances_free,
            balances_locked,
            {},
            self._uuid_factory.generate(),
            self._clock.utc_now(),
        )

        self._handle_event(event)

    async def _watch_orders(self):
        if not self._client.has["watchOrders"]:
            self._log.error("`watch_orders` not available.")
            return

        cdef bint exiting = False  # Flag to stop loop
        cdef dict order_event
        try:
            while True:
                try:
                    response = await self._client.watch_orders()
                except TypeError:
                    # Temporary workaround for testing
                    response = self._client.watch_orders
                    exiting = True

                order_event = response[0]

                if order_event["status"] == "closed":
                    self._generate_order_filled(order_event)

                if exiting:
                    break
        except asyncio.CancelledError as ex:
            self._log.debug(f"Cancelled `_watch_orders`.")
        except Exception as ex:
            self._log.exception(ex)  # TODO: During development
            # self._log.error(f"{type(ex).__name__}: {ex} in _watch_orders")

    cdef inline void _generate_order_filled(self, dict response) except *:
        # Parse exchange order identifier
        cdef OrderId order_id = OrderId(response["id"])

        cdef Instrument instrument = self._instrument_provider.get_c(response["symbol"])
        if instrument is None:
            self._log.error(f"Cannot fill order with id {order_id}, "
                            f"instrument for {response['symbol']} not found.")
            return  # Cannot fill order

        # Remove order from processing orders
        cdef Order order = self._processing_orders.pop(order_id, None)
        if order is None:
            self._log.error(f"Cannot fill order with id {order_id}, "
                            f"not found in the _processing_orders dict.")
            return  # Cannot fill order

        # Determine commission
        cdef Money commission = None
        cdef Currency currency = None
        cdef dict fees = response.get("fee")
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
            Quantity(response["amount"], instrument.size_precision),     # Filled
            Quantity(response["filled"], instrument.size_precision),     # Cumulative
            Quantity(response["remaining"], instrument.size_precision),  # Remaining
            Price(response["average"], instrument.price_precision),
            instrument.quote_currency,
            instrument.is_inverse,
            commission,
            LiquiditySide.TAKER if order.type != OrderType.LIMIT else LiquiditySide.MAKER,  # TODO: Implement
            from_posix_ms(response["timestamp"]),
            self._uuid_factory.generate(),
            self._clock.utc_now(),
        )

        self._handle_event(filled)

    # async def _watch_create_order(self):
    #     if not self._client.has["watchCreateOrder"]:
    #         self._log.error("`watch_create_order` not available.")
    #         return
    #
    #     cdef dict response
    #     cdef bint exiting = False  # Flag to stop loop
    #     try:
    #         while True:
    #             try:
    #                 response = await self._client.watch_create_order()
    #             except TypeError:
    #                 # Temporary workaround for testing
    #                 response = self._client.watch_create_order
    #                 exiting = True
    #
    #             # TODO!
    #             with open('res_watch_create_order.json', 'w') as json_file:
    #                 json.dump(response, json_file)
    #
    #             if exiting:
    #                 break
    #     except asyncio.CancelledError as ex:
    #         self._log.debug(f"Cancelled `_watch_create_order`.")
    #     except Exception as ex:
    #         self._log.error(f"{type(ex).__name__}: {ex} in _watch_create_order")
    #
    # async def _watch_cancel_order(self):
    #     if not self._client.has["watchCancelOrder"]:
    #         self._log.error("`watch_cancel_order` not available.")
    #         return
    #
    #     cdef dict response
    #     cdef bint exiting = False  # Flag to stop loop
    #     try:
    #         while True:
    #             try:
    #                 response = await self._client.watch_cancel_order()
    #             except TypeError:
    #                 # Temporary workaround for testing
    #                 response = self._client.watch_cancel_order
    #                 exiting = True
    #
    #             # TODO!
    #             with open('res_watch_cancel_order.json', 'w') as json_file:
    #                 json.dump(response, json_file)
    #
    #             if exiting:
    #                 break
    #     except asyncio.CancelledError as ex:
    #         self._log.debug(f"Cancelled `_watch_create_order`.")
    #     except Exception as ex:
    #         self._log.error(f"{type(ex).__name__}: {ex} in _watch_create_order")
    #

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
                except TypeError:
                    # Temporary workaround for testing
                    response = self._client.watch_my_trades
                    exiting = True

                # TODO!
                # with open('res_watch_my_trades.json', 'w') as json_file:
                #     json.dump(response, json_file)

                if exiting:
                    break
        except asyncio.CancelledError as ex:
            self._log.debug(f"Cancelled `_watch_my_trades`.")
        except Exception as ex:
            self._log.error(f"{type(ex).__name__}: {ex} in _watch_my_trades")
