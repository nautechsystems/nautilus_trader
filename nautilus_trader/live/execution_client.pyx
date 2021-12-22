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

"""
The `LiveExecutionClient` class is responsible for interfacing with a particular
API which may be presented directly by an exchange, or broker intermediary.
"""

import asyncio
import types

import pandas as pd
from cpython.datetime cimport datetime

from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logging cimport LogColor
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.providers cimport InstrumentProvider
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.execution.client cimport ExecutionClient
from nautilus_trader.execution.messages cimport ExecutionMassStatus
from nautilus_trader.execution.messages cimport ExecutionReport
from nautilus_trader.execution.messages cimport OrderStatusReport
from nautilus_trader.model.c_enums.account_type cimport AccountType
from nautilus_trader.model.c_enums.order_status cimport OrderStatus
from nautilus_trader.model.c_enums.order_status cimport OrderStatusParser
from nautilus_trader.model.c_enums.venue_type cimport VenueType
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport VenueOrderId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.msgbus.bus cimport MessageBus


cdef class LiveExecutionClient(ExecutionClient):
    """
    The abstract base class for all live execution clients.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client_id : ClientId
        The client ID.
    instrument_provider : InstrumentProvider
        The instrument provider for the client.
    venue_type : VenueType
        The client venue type.
    account_id : AccountId
        The account ID for the client.
    account_type : AccountType
        The account type for the client.
    base_currency : Currency, optional
        The account base currency for the client. Use ``None`` for multi-currency accounts.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    logger : Logger
        The logger for the client.
    config : dict[str, object], optional
        The configuration for the instance.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        loop not None: asyncio.AbstractEventLoop,
        ClientId client_id not None,
        InstrumentProvider instrument_provider not None,
        VenueType venue_type,
        AccountId account_id not None,
        AccountType account_type,
        Currency base_currency,  # Can be None
        MessageBus msgbus not None,
        Cache cache not None,
        LiveClock clock not None,
        Logger logger not None,
        dict config=None,
    ):
        super().__init__(
            client_id=client_id,
            venue_type=venue_type,
            account_id=account_id,
            account_type=account_type,
            base_currency=base_currency,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            config=config,
        )

        self._loop = loop
        self._instrument_provider = instrument_provider

    def connect(self):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def disconnect(self):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    @types.coroutine
    def sleep0(self):
        # Skip one event loop run cycle.
        #
        # This is equivalent to `asyncio.sleep(0)` however avoids the overhead
        # of the pure Python function call and integer comparison <= 0.
        #
        # Uses a bare 'yield' expression (which Task.__step knows how to handle)
        # instead of creating a Future object.
        yield

    async def run_after_delay(self, delay, coro):
        await asyncio.sleep(delay)
        return await coro

    async def generate_order_status_report(self, Order order):
        """
        Generate an order status report for the given order.

        If an error occurs then logs and returns ``None``.

        Parameters
        ----------
        order : Order
            The order for the report.

        Returns
        -------
        OrderStatusReport or ``None``

        """
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

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
            The venue order ID for the trades.
        symbol : Symbol
            The symbol for the trades.
        since : datetime, optional
            The timestamp to filter trades on.

        Returns
        -------
        list[ExecutionReport]

        """
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

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
            ts_init=self._clock.timestamp_ns(),
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

            if order_report.order_status in (OrderStatus.PARTIALLY_FILLED, OrderStatus.FILLED):
                exec_reports = await self.generate_exec_reports(
                    venue_order_id=order.venue_order_id,
                    symbol=order.instrument_id.symbol,
                    since=pd.Timestamp(order.ts_init, tz="UTC"),
                )
                mass_status.add_exec_reports(order.venue_order_id, exec_reports)

        return mass_status

    async def reconcile_state(
        self,
        OrderStatusReport report,
        Order order=None,
        list exec_reports=None,
    ) -> bool:
        """
        Reconcile the given orders state based on the given report.

        Parameters
        ----------
        report : OrderStatusReport
            The order status report for reconciliation.
        order : Order, optional
            The order for reconciliation. If not supplied then will try to be
            fetched from cache.
        exec_reports : list[ExecutionReport]
            The list of execution reports relating to the order.

        Raises
        ------
        ValueError
            If `report.client_order_id` is not equal to `order.client_order_id`.
        ValueError
            If `report.venue_order_id` is not equal to `order.venue_order_id`.

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
            order = self._cache.order(report.client_order_id)
            if order is None:
                self._log.error(
                    f"Cannot reconcile state for order {repr(report.venue_order_id)}, "
                    f"cannot find order in the cache.")
                return False  # Cannot reconcile state

        if order.is_completed_c():
            self._log.warning(
                f"No reconciliation required for completed order {order}.")
            return True

        self._log.info(f"Reconciling state for {repr(order.venue_order_id)}...", color=LogColor.BLUE)

        if report.order_status == OrderStatus.REJECTED:
            # No VenueOrderId would have been assigned from the exchange
            self._log.info("Generating OrderRejected event...", color=LogColor.BLUE)
            self.generate_order_rejected(
                order.strategy_id,
                order.instrument_id,
                report.client_order_id,
                "unknown",
                report.ts_init,
            )
            return True
        elif report.order_status == OrderStatus.EXPIRED:
            self._log.info("Generating OrderExpired event...", color=LogColor.BLUE)
            self.generate_order_expired(
                order.strategy_id,
                order.instrument_id,
                report.client_order_id,
                report.venue_order_id,
                report.ts_init,
            )
            return True
        elif report.order_status == OrderStatus.CANCELED:
            self._log.info("Generating OrderCanceled event...", color=LogColor.BLUE)
            self.generate_order_canceled(
                order.strategy_id,
                order.instrument_id,
                report.client_order_id,
                report.venue_order_id,
                report.ts_init,
            )
            return True
        elif report.order_status == OrderStatus.ACCEPTED:
            if order.status_c() == OrderStatus.SUBMITTED:
                self._log.info("Generating OrderAccepted event...", color=LogColor.BLUE)
                self.generate_order_accepted(
                    order.strategy_id,
                    order.instrument_id,
                    report.client_order_id,
                    report.venue_order_id,
                    report.ts_init,
                )
            return True

        # OrderStatus.PARTIALLY_FILLED or FILLED
        if exec_reports is None:
            self._log.error(
                f"Cannot reconcile state for {repr(report.venue_order_id)}, "
                f"no trades given for {OrderStatusParser.to_str(report.order_status)} order.")
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
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                venue_order_id=order.venue_order_id,
                venue_position_id=exec_report.venue_position_id,
                execution_id=exec_report.id,
                order_side=order.side,
                order_type=order.type,
                last_qty=exec_report.last_qty,
                last_px=exec_report.last_px,
                quote_currency=instrument.quote_currency,
                commission=exec_report.commission,
                liquidity_side=exec_report.liquidity_side,
                ts_event=exec_report.ts_event,
            )

        return True
