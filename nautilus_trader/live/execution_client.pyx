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

from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logging cimport LogColor
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.providers cimport InstrumentProvider
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.execution.client cimport ExecutionClient
from nautilus_trader.execution.messages cimport ExecutionMassStatus
from nautilus_trader.execution.messages cimport ExecutionReport
from nautilus_trader.execution.messages cimport OrderStatusReport
from nautilus_trader.live.execution_engine cimport LiveExecutionEngine
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.order_state cimport OrderState
from nautilus_trader.model.c_enums.order_state cimport OrderStateParser
from nautilus_trader.model.currency cimport Currency
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
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport OrderId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.order.base cimport Order


cdef class LiveExecutionClient(ExecutionClient):
    """
    The abstract base class for all live execution clients.

    This class should not be used directly, but through its concrete subclasses.
    """

    def __init__(
        self,
        str name not None,
        AccountId account_id not None,
        LiveExecutionEngine engine not None,
        InstrumentProvider instrument_provider not None,
        LiveClock clock not None,
        Logger logger not None,
        dict config=None,
    ):
        """
        Initialize a new instance of the `LiveExecutionClient` class.

        Parameters
        ----------
        name : str
            The name of the client.
        account_id : AccountId
            The account identifier for the client.
        engine : LiveDataEngine
            The data engine for the client.
        instrument_provider : InstrumentProvider
            The instrument provider for the client.
        clock : LiveClock
            The clock for the client.
        logger : Logger
            The logger for the client.
        config : dict[str, object], optional
            The configuration options.

        """
        super().__init__(
            name,
            account_id,
            engine,
            clock,
            logger,
            config,
        )

        self._loop: asyncio.AbstractEventLoop = engine.get_event_loop()
        self._instrument_provider = instrument_provider

        self._account_last_free = {}
        self._account_last_used = {}
        self._account_last_total = {}

    cpdef void reset(self) except *:
        """
        Reset the client.
        """
        if self.is_connected:
            self._log.error("Cannot reset a connected execution client.")
            return

        self._log.info("Resetting...")

        self._on_reset()

        self._account_last_free.clear()
        self._account_last_used.clear()
        self._account_last_total.clear()

        self._log.info("Reset.")

    cdef void _on_reset(self) except *:
        """
        Actions to be performed when client is reset.
        """
        pass  # Optionally override in subclass

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
        raise NotImplementedError("method must be implemented in the subclass")

    async def generate_trades_list(self, OrderId order_id, Symbol symbol, datetime since=None):
        """
        Generate a list of trades for the given parameters.

        The returned list may be empty if no trades match the given parameters.

        Parameters
        ----------
        order_id : OrderId
            The order identifier for the trades.
        symbol : Symbol
            The symbol for the trades.
        since : datetime, optional
            The timestamp to filter trades on.

        Returns
        -------
        list[ExecutionReport]

        """
        raise NotImplementedError("method must be implemented in the subclass")

    async def generate_mass_status(self, list active_orders):
        """
        Generate an execution state report based on the given list of active
        orders.

        Parameters
        ----------
        active_orders : list[Order]
            The orders which currently have an 'active' status.

        Returns
        -------
        ExecutionMassStatus

        """
        Condition.not_none(active_orders, "active_orders")

        self._log.info(f"Generating ExecutionMassStatus for {self.name}...")

        cdef ExecutionMassStatus mass_status = ExecutionMassStatus(
            client=self.name,
            account_id=self.account_id,
            timestamp=self._clock.utc_now(),
        )

        if not active_orders:
            # Nothing to resolve
            return mass_status

        cdef Instrument instrument
        cdef Order order
        cdef str status
        cdef dict response
        cdef list trades
        cdef list order_trades
        for order in active_orders:
            report = await self.generate_order_status_report(order)
            if report:
                mass_status.add_order_report(report)

            if report.order_state in (OrderState.PARTIALLY_FILLED, OrderState.FILLED):
                trades = await self.generate_trades_list(
                    order_id=order.id,
                    symbol=order.symbol,
                    since=order.timestamp,
                )
                mass_status.add_trades(order.id, trades)

        return mass_status

    async def reconcile_state(
        self, OrderStatusReport report,
        Order order=None,
        list trades=None,
    ):
        """
        Reconcile the given orders state based on the given report.

        Parameters
        ----------
        report : OrderStatusReport
            The order state report for reconciliation.
        order : Order, optional
            The order for reconciliation. If not supplied then will try to be
            fetched from cache.
        trades : list[ExecutionReport]
            The list of trades relating to the order.

        Raises
        ------
        ValueError
            If report.cl_ord_id is not equal to order.cl_ord_id.
        ValueError
            If report.order_id is not equal to order.id.

        """
        Condition.not_none(report, "report")
        Condition.equal(report.cl_ord_id, order.cl_ord_id, "report.cl_ord_id", "order.cl_ord_id")
        Condition.equal(report.order_id, order.id, "report.order_id", "order.id")

        if order is None:
            order = self._engine.cache(report.cl_ord_id)
            if order is None:
                self._log.warning(
                    f"No reconciliation required for completed order {order}.")
                return  # Cannot reconcile state

        if order.is_completed_c():
            self._log.warning(
                f"No reconciliation required for completed order {order}.")
            return  # Cannot reconcile state

        self._log.info(f"Reconciling state for {repr(order.id)}...", LogColor.BLUE)

        if report.order_state == OrderState.REJECTED:
            # No OrderId would have been assigned from the exchange
            self._log.info("Generating OrderRejected event...", LogColor.GREEN)
            self._generate_order_rejected(report.cl_ord_id, "unknown", report.timestamp)
            return
        elif report.order_state == OrderState.EXPIRED:
            self._log.info("Generating OrderExpired event...", LogColor.GREEN)
            self._generate_order_expired(report.cl_ord_id, report.order_id, report.timestamp)
            return
        elif report.order_state == OrderState.CANCELLED:
            self._log.info("Generating OrderCancelled event...", LogColor.GREEN)
            self._generate_order_cancelled(report.cl_ord_id, report.order_id, report.timestamp)
            return
        elif report.order_state == OrderState.ACCEPTED:
            if order.state_c() == OrderState.SUBMITTED:
                self._log.info("Generating OrderAccepted event...", LogColor.GREEN)
                self._generate_order_accepted(report.cl_ord_id, report.order_id, report.timestamp)
            return
            # TODO: Consider other scenarios

        if trades is None:
            self._log.error(
                f"Cannot reconcile state for {repr(order.cl_ord_id)}, "
                f"not trades given for {OrderStateParser.to_str(order.state_c())} order.")
            return  # Cannot reconcile state

        cdef ExecutionReport trade
        for trade in trades:
            if trade.id in order.execution_ids_c():
                continue  # Trade already applied
            self._log.info(f"Generating OrderFilled event for {repr(trade.id)}...", LogColor.GREEN)
            self._generate_order_filled(
                cl_ord_id=order.cl_ord_id,
                order_id=order.id,
                execution_id=trade.id,
                instrument_id=order.instrument_id,
                order_side=order.side,
                fill_qty=trade.last_qty,
                cum_qty=Decimal(),     # TODO: use hot cache
                leaves_qty=Decimal(),  # TODO: use hot cache
                avg_px=trade.last_px,
                commission_amount=trade.commission_amount,
                commission_currency=trade.commission_currency,
                liquidity_side=trade.liquidity_side,
                timestamp=trade.timestamp,
            )

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
        ExecutionId execution_id,
        InstrumentId instrument_id,
        OrderSide order_side,
        fill_qty: Decimal,
        cum_qty: Decimal,
        leaves_qty: Decimal,
        avg_px: Decimal,
        commission_amount: Decimal,
        str commission_currency,
        LiquiditySide liquidity_side,
        datetime timestamp
    ) except *:
        cdef Instrument instrument = self._instrument_provider.find(instrument_id)
        if instrument is None:
            self._log.error(f"Cannot fill order with {repr(order_id)}, "
                            f"instrument for {instrument_id} not found.")
            return  # Cannot fill order

        # Determine commission
        cdef Money commission
        cdef Currency currency
        if commission_currency is None:
            commission = Money(0, instrument.quote_currency)
        else:
            currency = self._instrument_provider.currency(commission_currency)
            if currency is None:
                self._log.error(f"Cannot determine commission for {repr(order_id)}, "
                                f"currency for {commission_currency} not found.")
                commission = Money(0, instrument.quote_currency)
            else:
                commission = Money(commission_amount, currency)

        # Generate event
        cdef OrderFilled filled = OrderFilled(
            self.account_id,
            cl_ord_id,
            order_id,
            execution_id,
            PositionId.null_c(),  # Assigned in engine
            StrategyId.null_c(),  # Assigned in engine
            instrument_id,
            order_side,
            Quantity(fill_qty, instrument.size_precision),
            Quantity(cum_qty, instrument.size_precision),
            Quantity(leaves_qty, instrument.size_precision),
            Price(avg_px, instrument.price_precision),
            instrument.quote_currency,
            instrument.is_inverse,
            commission,
            liquidity_side,
            timestamp,
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
