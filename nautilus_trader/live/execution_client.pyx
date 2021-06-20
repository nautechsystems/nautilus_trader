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

from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logging cimport LiveLogger
from nautilus_trader.common.logging cimport LogColor
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.providers cimport InstrumentProvider
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport nanos_to_unix_dt
from nautilus_trader.execution.client cimport ExecutionClient
from nautilus_trader.execution.messages cimport ExecutionMassStatus
from nautilus_trader.execution.messages cimport ExecutionReport
from nautilus_trader.execution.messages cimport OrderStatusReport
from nautilus_trader.live.execution_engine cimport LiveExecutionEngine
from nautilus_trader.model.c_enums.account_type cimport AccountType
from nautilus_trader.model.c_enums.order_state cimport OrderState
from nautilus_trader.model.c_enums.order_state cimport OrderStateParser
from nautilus_trader.model.c_enums.venue_type cimport VenueType
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport VenueOrderId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.orders.base cimport Order


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
            The client name.
        config : dict[str, object]
            The client configuration.
        engine : LiveDataEngine
            The clients engine.
        clock : LiveClock
            The clients clock.
        logger : LiveLogger
            The clients logger.
        client_cls : class, optional
            The internal client constructor. This allows external library and
            testing dependency injection.

        Returns
        -------
        LiveExecutionClient

        """
        raise NotImplementedError("method must be implemented in the subclass")


cdef class LiveExecutionClient(ExecutionClient):
    """
    The abstract base class for all live execution clients.

    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        ClientId client_id not None,
        VenueType venue_type,
        AccountId account_id not None,
        AccountType account_type,
        Currency base_currency,  # Can be None
        LiveExecutionEngine engine not None,
        InstrumentProvider instrument_provider not None,
        LiveClock clock not None,
        Logger logger not None,
        dict config=None,
    ):
        """
        Initialize a new instance of the ``LiveExecutionClient`` class.

        Parameters
        ----------
        client_id : ClientId
            The client identifier.
        venue_type : VenueType
            The client venue type.
        account_id : AccountId
            The account identifier for the client.
        account_type : AccountType
            The account type for the client.
        base_currency : Currency, optional
            The account base currency for the client. Use ``None`` for multi-currency accounts.
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
            client_id=client_id,
            venue_type=venue_type,
            account_id=account_id,
            account_type=account_type,
            base_currency=base_currency,
            engine=engine,
            clock=clock,
            logger=logger,
            config=config,
        )

        self._loop: asyncio.AbstractEventLoop = engine.get_event_loop()
        self._instrument_provider = instrument_provider

    cpdef void reset(self) except *:
        """
        Reset the client.
        """
        if self.is_connected:
            self._log.error("Cannot reset a connected execution client.")
            return

        self._log.info("Resetting...")

        self._on_reset()

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

        self._log.info(f"Generating ExecutionMassStatus for {self.id}...")

        cdef ExecutionMassStatus mass_status = ExecutionMassStatus(
            client_id=self.id,
            account_id=self.account_id,
            timestamp_ns=self._clock.timestamp_ns(),
        )

        if not active_orders:
            # Nothing to reconcile
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
                    venue_order_id=order.venue_order_id,
                    symbol=order.instrument_id.symbol,
                    since=nanos_to_unix_dt(nanos=order.timestamp_ns),
                )
                mass_status.add_exec_reports(order.venue_order_id, exec_reports)

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
            If report.client_order_id is not equal to order.client_order_id.
        ValueError
            If report.venue_order_id is not equal to order.venue_order_id.

        Returns
        -------
        bool
            True if reconciliation event generation succeeded, else False.

        """
        Condition.not_none(report, "report")
        if order:
            Condition.equal(report.client_order_id, order.client_order_id, "report.client_order_id", "order.client_order_id")
            Condition.equal(report.venue_order_id, order.venue_order_id, "report.venue_order_id", "order.venue_order_id")
        else:
            order = self._engine.cache.order(report.client_order_id)
            if order is None:
                self._log.error(
                    f"Cannot reconcile state for {repr(report.venue_order_id)}, "
                    f"cannot find order in cache.")
                return False  # Cannot reconcile state

        if order.is_completed_c():
            self._log.warning(
                f"No reconciliation required for completed order {order}.")
            return True

        self._log.info(f"Reconciling state for {repr(order.venue_order_id)}...", color=LogColor.BLUE)

        if report.order_state == OrderState.REJECTED:
            # No VenueOrderId would have been assigned from the exchange
            self._log.info("Generating OrderRejected event...", color=LogColor.BLUE)
            self.generate_order_rejected(report.client_order_id, "unknown", report.timestamp_ns)
            return True
        elif report.order_state == OrderState.EXPIRED:
            self._log.info("Generating OrderExpired event...", color=LogColor.BLUE)
            self.generate_order_expired(report.client_order_id, report.venue_order_id, report.timestamp_ns)
            return True
        elif report.order_state == OrderState.CANCELED:
            self._log.info("Generating OrderCanceled event...", color=LogColor.BLUE)
            self.generate_order_canceled(report.client_order_id, report.venue_order_id, report.timestamp_ns)
            return True
        elif report.order_state == OrderState.ACCEPTED:
            if order.state_c() == OrderState.SUBMITTED:
                self._log.info("Generating OrderAccepted event...", color=LogColor.BLUE)
                self.generate_order_accepted(report.client_order_id, report.venue_order_id, report.timestamp_ns)
            return True
            # TODO: Consider other scenarios

        # OrderState.PARTIALLY_FILLED or FILLED
        if exec_reports is None:
            self._log.error(
                f"Cannot reconcile state for {repr(report.venue_order_id)}, "
                f"no trades given for {OrderStateParser.to_str(report.order_state)} order.")
            return False  # Cannot reconcile state

        cdef ExecutionReport exec_report
        cdef Instrument instrument
        for exec_report in exec_reports:
            if exec_report.id in order.execution_ids_c():
                continue  # Trade already applied
            self._log.info(
                f"Generating OrderFilled event for {repr(exec_report.id)}...",
                color=LogColor.BLUE,
            )

            instrument = self._instrument_provider.find(order.instrument_id)
            if instrument is None:
                self._log.error(f"Cannot fill order: "
                                f"no instrument found for {order.instrument_id}")
                return False  # Cannot reconcile state

            self.generate_order_filled(
                client_order_id=order.client_order_id,
                venue_order_id=order.venue_order_id,
                execution_id=exec_report.id,
                position_id=None,  # Assigned in engine
                instrument_id=order.instrument_id,
                order_side=order.side,
                last_qty=exec_report.last_qty,
                last_px=exec_report.last_px,
                quote_currency=instrument.quote_currency,
                commission=exec_report.commission,
                liquidity_side=exec_report.liquidity_side,
                ts_filled_ns=exec_report.ts_filled_ns,
            )

        return True
