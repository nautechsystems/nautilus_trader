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
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.execution.client cimport ExecutionClient
from nautilus_trader.live.execution_engine cimport LiveExecutionEngine
from nautilus_trader.live.providers cimport InstrumentProvider
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.c_enums.order_side cimport OrderSide
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
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class LiveExecutionClient(ExecutionClient):
    """
    The abstract base class for all live execution clients.

    This class should not be used directly, but through its concrete subclasses.
    """

    def __init__(
        self,
        Venue venue not None,
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
        venue : Venue
            The venue for the client.
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
            venue,
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
        cdef Instrument instrument = self._instrument_provider.get(instrument_id)
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
