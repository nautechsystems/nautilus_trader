# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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
from datetime import timedelta
from typing import Optional

from cpython.datetime cimport datetime

from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.providers cimport InstrumentProvider
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.execution.client cimport ExecutionClient
from nautilus_trader.execution.reports cimport ExecutionMassStatus
from nautilus_trader.model.c_enums.account_type cimport AccountType
from nautilus_trader.model.c_enums.oms_type cimport OMSType
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.identifiers cimport VenueOrderId
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
    venue : Venue, optional
        The client venue. If multi-venue then can be ``None``.
    instrument_provider : InstrumentProvider
        The instrument provider for the client.
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

    Raises
    ------
    ValueError
        If `oms_type` is ``NONE`` value (must be defined).

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        loop not None: asyncio.AbstractEventLoop,
        ClientId client_id not None,
        Venue venue,  # Can be None
        OMSType oms_type,
        AccountType account_type,
        Currency base_currency,  # Can be None
        InstrumentProvider instrument_provider not None,
        MessageBus msgbus not None,
        Cache cache not None,
        LiveClock clock not None,
        Logger logger not None,
        dict config=None,
    ):
        super().__init__(
            client_id=client_id,
            venue=venue,
            oms_type=oms_type,
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

        self.reconciliation_active = False

    def connect(self) -> None:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def disconnect(self) -> None:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    @types.coroutine
    def sleep0(self) -> None:
        # Skip one event loop run cycle.
        #
        # This is equivalent to `asyncio.sleep(0)` however avoids the overhead
        # of the pure Python function call and integer comparison <= 0.
        #
        # Uses a bare 'yield' expression (which Task.__step knows how to handle)
        # instead of creating a Future object.
        yield

    async def run_after_delay(self, delay: float, coro) -> None:
        await asyncio.sleep(delay)
        return await coro

    async def generate_order_status_report(
        self,
        instrument_id: InstrumentId,
        client_order_id: Optional[ClientOrderId] = None,
        venue_order_id: Optional[VenueOrderId] = None,
    ):
        """
        Generate an order status report for the given order identifier parameter(s).

        If the order is not found, or an error occurs, then logs and returns ``None``.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the report.
        client_order_id : ClientOrderId, optional
            The client order ID for the report.
        venue_order_id : VenueOrderId, optional
            The venue order ID for the report.

        Returns
        -------
        OrderStatusReport or ``None``

        Raises
        ------
        ValueError
            If both the `client_order_id` and `venue_order_id` are ``None``.

        """
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    async def generate_order_status_reports(
        self,
        InstrumentId instrument_id=None,
        datetime start=None,
        datetime end=None,
        bint open_only=False,
    ):
        """
        Generate a list of order status reports with optional query filters.

        The returned list may be empty if no orders match the given parameters.

        Parameters
        ----------
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        start : datetime, optional
            The start datetime query filter.
        end : datetime, optional
            The end datetime query filter.
        open_only : bool, default False
            If the query is for open orders only.

        Returns
        -------
        list[OrderStatusReport]

        """
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    async def generate_trade_reports(
        self,
        InstrumentId instrument_id=None,
        VenueOrderId venue_order_id=None,
        datetime start=None,
        datetime end=None,
    ):
        """
        Generate a list of trade reports with optional query filters.

        The returned list may be empty if no trades match the given parameters.

        Parameters
        ----------
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        venue_order_id : VenueOrderId, optional
            The venue order ID (assigned by the venue) query filter.
        start : datetime, optional
            The start datetime query filter.
        end : datetime, optional
            The end datetime query filter.

        Returns
        -------
        list[TradeReport]

        """
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    async def generate_position_status_reports(
        self,
        InstrumentId instrument_id=None,
        datetime start=None,
        datetime end=None,
    ):
        """
        Generate a list of position status reports with optional query filters.

        The returned list may be empty if no positions match the given parameters.

        Parameters
        ----------
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        start : datetime, optional
            The start datetime query filter.
        end : datetime, optional
            The end datetime query filter.

        Returns
        -------
        list[PositionStatusReport]

        """
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    async def generate_mass_status(self, lookback_mins: Optional[int]):
        """
        Generate an execution mass status report.

        Parameters
        ----------
        lookback_mins : int, optional
            The maximum lookback for querying closed orders, trades and positions.

        Returns
        -------
        ExecutionMassStatus

        """
        self._log.info(f"Generating ExecutionMassStatus for {self.id}...")

        self.reconciliation_active = True

        cdef ExecutionMassStatus mass_status = ExecutionMassStatus(
            client_id=self.id,
            account_id=self.account_id,
            venue=self.venue,
            report_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
        )

        since = None
        if lookback_mins is not None:
            since = self._clock.utc_now() - timedelta(minutes=lookback_mins)

        try:
            reports = await asyncio.gather(
                self.generate_order_status_reports(start=since),
                self.generate_trade_reports(start=since),
                self.generate_position_status_reports(start=since),
            )

            mass_status.add_order_reports(reports=reports[0])
            mass_status.add_trade_reports(reports=reports[1])
            mass_status.add_position_reports(reports=reports[2])
        except Exception as e:
            self._log.exception("Cannot reconcile execution state", e)

        self.reconciliation_active = False

        return mass_status
