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

from decimal import Decimal

from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logging cimport LiveLogger
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.providers cimport InstrumentProvider
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport nanos_to_millis
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


cdef class LiveExecutionClientFactory:
    """
    Provides a factory for creating `LiveDataClient` instances.
    """

    @staticmethod
    def create(
        str name not None,
        dict config not None,
        LiveExecutionEngine engine not None,
        LiveClock clock not None,
        LiveLogger logger not None,
        client_cls=None,
    ):
        """
        Return a new execution client from the given parameters.

        Parameters
        ----------
        name : str
            The name for the client.
        config : dict[str, object]
            The client configuration.
        engine : LiveDataEngine
            The clients engine.
        clock : LiveClock
            The clients clock.
        logger : LiveLogger
            The client logger.
        client_cls : class, optional
            The internal client constructor.

        Returns
        -------
        LiveExecutionClient

        """
        raise NotImplementedError("method must be implemented in the subclass")


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

    async def generate_exec_reports(self, OrderId order_id, Symbol symbol, datetime since=None):
        """
        Generate a list of execution reports.

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
            timestamp_ns=self._clock.timestamp_ns(),
        )

        if not active_orders:
            # Nothing to resolve
            return mass_status

        cdef Order order
        cdef OrderStatusReport order_report
        cdef list exec_reports
        for order in active_orders:
            order_report = await self.generate_order_status_report(order)
            if order_report:
                mass_status.add_order_report(order_report)

            if order_report.order_state in (OrderState.PARTIALLY_FILLED, OrderState.FILLED):
                exec_reports = await self.generate_exec_reports(
                    order_id=order.id,
                    symbol=order.instrument_id.symbol,
                    since=nanos_to_millis(nanos=order.timestamp_ns),
                )
                mass_status.add_exec_reports(order.id, exec_reports)

        return mass_status

    async def reconcile_state(
        self, OrderStatusReport report,
        Order order=None,
        list exec_reports=None,
    ) -> bool:
        """
        Reconcile the given orders state based on the given report.

        Returns the result of the reconciliation.

        Parameters
        ----------
        report : OrderStatusReport
            The order state report for reconciliation.
        order : Order, optional
            The order for reconciliation. If not supplied then will try to be
            fetched from cache.
        exec_reports : list[ExecutionReport]
            The list of execution reports relating to the order.

        Raises
        ------
        ValueError
            If report.cl_ord_id is not equal to order.cl_ord_id.
        ValueError
            If report.order_id is not equal to order.id.

        Returns
        -------
        bool
            True if reconciliation event generation succeeded, else False.

        """
        Condition.not_none(report, "report")
        if order:
            Condition.equal(report.cl_ord_id, order.cl_ord_id, "report.cl_ord_id", "order.cl_ord_id")
            Condition.equal(report.order_id, order.id, "report.order_id", "order.id")
        else:
            order = self._engine.cache.order(report.cl_ord_id)
            if order is None:
                self._log.error(
                    f"Cannot reconcile state for {repr(report.order_id)}, "
                    f"cannot find order in cache.")
                return False  # Cannot reconcile state

        if order.is_completed_c():
            self._log.warning(
                f"No reconciliation required for completed order {order}.")
            return True

        self._log.info_blue(f"Reconciling state for {repr(order.id)}...")

        if report.order_state == OrderState.REJECTED:
            # No OrderId would have been assigned from the exchange
            # TODO: Investigate if exchanges record rejected orders?
            self._log.info_green("Generating OrderRejected event...")
            self._generate_order_rejected(report.cl_ord_id, "unknown", report.timestamp_ns)
            return True
        elif report.order_state == OrderState.EXPIRED:
            self._log.info_green("Generating OrderExpired event...")
            self._generate_order_expired(report.cl_ord_id, report.order_id, report.timestamp_ns)
            return True
        elif report.order_state == OrderState.CANCELLED:
            self._log.info_green("Generating OrderCancelled event...")
            self._generate_order_cancelled(report.cl_ord_id, report.order_id, report.timestamp_ns)
            return True
        elif report.order_state == OrderState.ACCEPTED:
            if order.state_c() == OrderState.SUBMITTED:
                self._log.info_green("Generating OrderAccepted event...")
                self._generate_order_accepted(report.cl_ord_id, report.order_id, report.timestamp_ns)
            return True
            # TODO: Consider other scenarios

        # OrderState.PARTIALLY_FILLED or FILLED
        if exec_reports is None:
            self._log.error(
                f"Cannot reconcile state for {repr(report.order_id)}, "
                f"no trades given for {OrderStateParser.to_str(report.order_state)} order.")
            return False  # Cannot reconcile state

        cdef ExecutionReport exec_report
        for exec_report in exec_reports:
            if exec_report.id in order.execution_ids_c():
                continue  # Trade already applied
            self._log.info_green(f"Generating OrderFilled event for {repr(exec_report.id)}...")
            self._generate_order_filled(
                cl_ord_id=order.cl_ord_id,
                order_id=order.id,
                execution_id=exec_report.id,
                instrument_id=order.instrument_id,
                order_side=order.side,
                last_qty=exec_report.last_qty,
                last_px=exec_report.last_px,
                cum_qty=Decimal(),     # TODO: use hot cache?
                leaves_qty=Decimal(),  # TODO: use hot cache?
                commission_amount=exec_report.commission_amount,
                commission_currency=exec_report.commission_currency,
                liquidity_side=exec_report.liquidity_side,
                timestamp_ns=exec_report.execution_ns,
            )

        return True

    cdef inline void _generate_order_invalid(
        self,
        ClientOrderId cl_ord_id,
        str reason,
    ) except *:
        # Generate event
        cdef OrderInvalid invalid = OrderInvalid(
            cl_ord_id=cl_ord_id,
            reason=reason,
            event_id=self._uuid_factory.generate(),
            timestamp_ns=self._clock.timestamp_ns(),
        )
        self._handle_event(invalid)

    cdef inline void _generate_order_submitted(
        self, ClientOrderId cl_ord_id,
        int64_t submitted_ns,
    ) except *:
        # Generate event
        cdef OrderSubmitted submitted = OrderSubmitted(
            self.account_id,
            cl_ord_id=cl_ord_id,
            submitted_ns=submitted_ns,
            event_id=self._uuid_factory.generate(),
            timestamp_ns=self._clock.timestamp_ns(),
        )
        self._handle_event(submitted)

    cdef inline void _generate_order_rejected(
        self,
        ClientOrderId cl_ord_id,
        str reason,
        int64_t timestamp_ns,
    ) except *:
        # Generate event
        cdef OrderRejected rejected = OrderRejected(
            self.account_id,
            cl_ord_id=cl_ord_id,
            rejected_ns=timestamp_ns,
            reason=reason,
            event_id=self._uuid_factory.generate(),
            timestamp_ns=self._clock.timestamp_ns(),
        )
        self._handle_event(rejected)

    cdef inline void _generate_order_accepted(
        self,
        ClientOrderId cl_ord_id,
        OrderId order_id,
        int64_t timestamp_ns,
    ) except *:
        # Generate event
        cdef OrderAccepted accepted = OrderAccepted(
            self.account_id,
            cl_ord_id=cl_ord_id,
            order_id=order_id,
            accepted_ns=timestamp_ns,
            event_id=self._uuid_factory.generate(),
            timestamp_ns=self._clock.timestamp_ns(),
        )
        self._handle_event(accepted)

    cdef inline void _generate_order_filled(
        self,
        ClientOrderId cl_ord_id,
        OrderId order_id,
        ExecutionId execution_id,
        InstrumentId instrument_id,
        OrderSide order_side,
        last_qty: Decimal,
        last_px: Decimal,     # TODO: Add AvgPx?
        cum_qty: Decimal,     # TODO: Can be None and will use a cache, log warning if different
        leaves_qty: Decimal,  # TODO: Can be None and will use a cache, log warning if different
        commission_amount: Decimal,
        str commission_currency,
        LiquiditySide liquidity_side,
        int64_t timestamp_ns,
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
        cdef OrderFilled fill = OrderFilled(
            self.account_id,
            cl_ord_id=cl_ord_id,
            order_id=order_id,
            execution_id=execution_id,
            position_id=PositionId.null_c(),  # Assigned in engine
            strategy_id=StrategyId.null_c(),  # Assigned in engine
            instrument_id=instrument_id,
            order_side=order_side,
            last_qty=Quantity(last_qty, instrument.size_precision),
            last_px=Price(last_px, instrument.price_precision),
            cum_qty=Quantity(cum_qty, instrument.size_precision),
            leaves_qty=Quantity(leaves_qty, instrument.size_precision),
            currency=instrument.quote_currency,
            is_inverse=instrument.is_inverse,
            commission=commission,
            liquidity_side=liquidity_side,
            execution_ns=timestamp_ns,
            event_id=self._uuid_factory.generate(),
            timestamp_ns=self._clock.timestamp_ns(),
        )

        self._handle_event(fill)

    cdef inline void _generate_order_cancelled(
        self,
        ClientOrderId cl_ord_id,
        OrderId order_id,
        int64_t timestamp_ns,
    ) except *:
        # Generate event
        cdef OrderCancelled cancelled = OrderCancelled(
            account_id=self.account_id,
            cl_ord_id=cl_ord_id,
            order_id=order_id,
            cancelled_ns=timestamp_ns,
            event_id=self._uuid_factory.generate(),
            timestamp_ns=self._clock.timestamp_ns(),
        )

        self._handle_event(cancelled)

    cdef inline void _generate_order_expired(
        self,
        ClientOrderId cl_ord_id,
        OrderId order_id,
        int64_t timestamp_ns,
    ) except *:
        # Generate event
        cdef OrderExpired expired = OrderExpired(
            account_id=self.account_id,
            cl_ord_id=cl_ord_id,
            order_id=order_id,
            expired_ns=timestamp_ns,
            event_id=self._uuid_factory.generate(),
            timestamp_ns=self._clock.timestamp_ns(),
        )

        self._handle_event(expired)
