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

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.commands cimport CancelOrder
from nautilus_trader.model.commands cimport SubmitBracketOrder
from nautilus_trader.model.commands cimport SubmitOrder
from nautilus_trader.model.commands cimport UpdateOrder
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.events cimport AccountState
from nautilus_trader.model.events cimport Event
from nautilus_trader.model.events cimport OrderAccepted
from nautilus_trader.model.events cimport OrderCancelRejected
from nautilus_trader.model.events cimport OrderCancelled
from nautilus_trader.model.events cimport OrderExpired
from nautilus_trader.model.events cimport OrderFilled
from nautilus_trader.model.events cimport OrderInvalid
from nautilus_trader.model.events cimport OrderRejected
from nautilus_trader.model.events cimport OrderSubmitted
from nautilus_trader.model.events cimport OrderTriggered
from nautilus_trader.model.events cimport OrderUpdateRejected
from nautilus_trader.model.events cimport OrderUpdated
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport ExecutionId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport VenueOrderId
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport Order


cdef class ExecutionClient:
    """
    The abstract base class for all execution clients.

    This class should not be used directly, but through its concrete subclasses.
    """

    def __init__(
        self,
        ClientId client_id not None,
        AccountId account_id not None,
        ExecutionEngine engine not None,
        Clock clock not None,
        Logger logger not None,
        dict config=None,
    ):
        """
        Initialize a new instance of the `ExecutionClient` class.

        Parameters
        ----------
        client_id : ClientId
            The client identifier. It is assumed that the client_id will equal
            the venue identifier.
        account_id : AccountId
            The account identifier for the client.
        engine : ExecutionEngine
            The execution engine to connect to the client.
        clock : Clock
            The clock for the component.
        logger : Logger
            The logger for the component.
        config : dict[str, object], optional
            The configuration options.

        """
        Condition.equal(client_id.value, account_id.issuer_as_venue().value, "client_id.value", "account_id.issuer_as_venue().value")

        if config is None:
            config = {}

        self._clock = clock
        self._uuid_factory = UUIDFactory()
        self._log = LoggerAdapter(
            component=config.get("name", f"ExecClient-{client_id.value}"),
            logger=logger,
        )
        self._engine = engine
        self._config = config

        self.id = client_id
        self.venue = Venue(client_id.value)  # Assumption that ClientId == Venue
        self.account_id = account_id
        self.is_connected = False

        self._log.info(f"Initialized.")

    def __repr__(self) -> str:
        return f"{type(self).__name__}-{self.id.value}"

    cpdef void _set_connected(self, bint value=True) except *:
        """
        Setter for pure Python implementations to change the readonly property.

        Parameters
        ----------
        value : bool
            The value to set for is_connected.

        """
        self.is_connected = value

    cpdef void connect(self) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void disconnect(self) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void reset(self) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void dispose(self) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

# -- COMMAND HANDLERS ------------------------------------------------------------------------------

    cpdef void submit_order(self, SubmitOrder command) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void submit_bracket_order(self, SubmitBracketOrder command) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void update_order(self, UpdateOrder command) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void cancel_order(self, CancelOrder command) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

# -- EVENT HANDLERS --------------------------------------------------------------------------------

    cpdef void generate_account_state(
        self,
        list balances,
        list balances_free,
        list balances_locked,
        dict info=None,
    ) except *:
        if info is None:
            info = {}

        # Generate event
        cdef AccountState account_state = AccountState(
            account_id=self.account_id,
            balances=balances,
            balances_free=balances_free,
            balances_locked=balances_locked,
            info=info,
            event_id=self._uuid_factory.generate(),
            timestamp_ns=self._clock.timestamp_ns(),
        )

        self._handle_event(account_state)

    cpdef void generate_order_invalid(
        self,
        ClientOrderId client_order_id,
        str reason,
    ) except *:
        # Generate event
        cdef OrderInvalid invalid = OrderInvalid(
            client_order_id=client_order_id,
            reason=reason,
            event_id=self._uuid_factory.generate(),
            timestamp_ns=self._clock.timestamp_ns(),
        )

        self._handle_event(invalid)

    cpdef void generate_order_submitted(
        self, ClientOrderId client_order_id,
        int64_t submitted_ns,
    ) except *:
        # Generate event
        cdef OrderSubmitted submitted = OrderSubmitted(
            self.account_id,
            client_order_id=client_order_id,
            submitted_ns=submitted_ns,
            event_id=self._uuid_factory.generate(),
            timestamp_ns=self._clock.timestamp_ns(),
        )

        self._handle_event(submitted)

    cpdef void generate_order_rejected(
        self,
        ClientOrderId client_order_id,
        str reason,
        int64_t timestamp_ns,
    ) except *:
        # Generate event
        cdef OrderRejected rejected = OrderRejected(
            self.account_id,
            client_order_id=client_order_id,
            rejected_ns=timestamp_ns,
            reason=reason,
            event_id=self._uuid_factory.generate(),
            timestamp_ns=self._clock.timestamp_ns(),
        )

        self._handle_event(rejected)

    cpdef void generate_order_accepted(
        self,
        ClientOrderId client_order_id,
        VenueOrderId venue_order_id,
        int64_t timestamp_ns,
    ) except *:
        # Generate event
        cdef OrderAccepted accepted = OrderAccepted(
            self.account_id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            accepted_ns=timestamp_ns,
            event_id=self._uuid_factory.generate(),
            timestamp_ns=self._clock.timestamp_ns(),
        )

        self._handle_event(accepted)

    cpdef void generate_order_update_rejected(
        self,
        ClientOrderId client_order_id,
        str response,
        str reason,
        int64_t timestamp_ns,
    ) except *:
        cdef VenueOrderId venue_order_id = None
        cdef Order order = self._engine.cache.order(client_order_id)
        if order is not None:
            venue_order_id = order.venue_order_id
        else:
            venue_order_id = VenueOrderId.null_c()

        # Generate event
        cdef OrderUpdateRejected update_rejected = OrderUpdateRejected(
            account_id=self.account_id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            rejected_ns=timestamp_ns,
            response_to=response,
            reason=reason,
            event_id=self._uuid_factory.generate(),
            timestamp_ns=self._clock.timestamp_ns(),
        )

        self._handle_event(update_rejected)

    cpdef void generate_order_cancel_rejected(
        self,
        ClientOrderId client_order_id,
        str response,
        str reason,
        int64_t timestamp_ns,
    ) except *:
        cdef VenueOrderId venue_order_id = None
        cdef Order order = self._engine.cache.order(client_order_id)
        if order is not None:
            venue_order_id = order.venue_order_id
        else:
            venue_order_id = VenueOrderId.null_c()

        # Generate event
        cdef OrderCancelRejected cancel_rejected = OrderCancelRejected(
            account_id=self.account_id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            rejected_ns=timestamp_ns,
            response_to=response,
            reason=reason,
            event_id=self._uuid_factory.generate(),
            timestamp_ns=self._clock.timestamp_ns(),
        )

        self._handle_event(cancel_rejected)

    cpdef void generate_order_updated(
        self,
        ClientOrderId client_order_id,
        VenueOrderId venue_order_id,
        Quantity quantity,
        Price price,
        int64_t timestamp_ns,
        bint venue_order_id_modified=False,
    ) except *:
        # Check venue_order_id against cache, only allow modification when `venue_order_id_modified=True`
        if not venue_order_id_modified:
            existing = self._engine.cache.venue_order_id(client_order_id)
            Condition.equal(existing, venue_order_id, "existing", "order.venue_order_id")

        # Generate event
        cdef OrderUpdated updated = OrderUpdated(
            account_id=self.account_id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            quantity=quantity,
            price=price,
            updated_ns=timestamp_ns,
            event_id=self._uuid_factory.generate(),
            timestamp_ns=self._clock.timestamp_ns(),
        )

        self._handle_event(updated)

    cpdef void generate_order_triggered(
        self,
        ClientOrderId client_order_id,
        VenueOrderId venue_order_id,
        int64_t timestamp_ns,
    ) except *:
        # Generate event
        cdef OrderTriggered triggered = OrderTriggered(
            account_id=self.account_id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            triggered_ns=self._clock.timestamp_ns(),
            event_id=self._uuid_factory.generate(),
            timestamp_ns=self._clock.timestamp_ns(),
        )

        self._handle_event(triggered)

    cpdef void generate_order_cancelled(
        self,
        ClientOrderId client_order_id,
        VenueOrderId venue_order_id,
        int64_t timestamp_ns,
    ) except *:
        # Generate event
        cdef OrderCancelled cancelled = OrderCancelled(
            account_id=self.account_id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            cancelled_ns=timestamp_ns,
            event_id=self._uuid_factory.generate(),
            timestamp_ns=self._clock.timestamp_ns(),
        )

        self._handle_event(cancelled)

    cpdef void generate_order_expired(
        self,
        ClientOrderId client_order_id,
        VenueOrderId venue_order_id,
        int64_t timestamp_ns,
    ) except *:
        # Generate event
        cdef OrderExpired expired = OrderExpired(
            account_id=self.account_id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            expired_ns=timestamp_ns,
            event_id=self._uuid_factory.generate(),
            timestamp_ns=self._clock.timestamp_ns(),
        )

        self._handle_event(expired)

    cpdef void generate_order_filled(
        self,
        ClientOrderId client_order_id,
        VenueOrderId venue_order_id,
        ExecutionId execution_id,
        PositionId position_id,  # Can be None
        InstrumentId instrument_id,
        OrderSide order_side,
        Quantity last_qty,
        Price last_px,
        Currency quote_currency,
        bint is_inverse,
        Money commission,
        LiquiditySide liquidity_side,
        int64_t timestamp_ns,
    ) except *:
        # Generate event
        cdef OrderFilled fill = OrderFilled(
            account_id=self.account_id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            execution_id=execution_id,
            position_id=position_id or PositionId.null_c(),  # If 'NULL' then assigned in engine
            strategy_id=StrategyId.null_c(),                 # If 'NULL' then assigned in engine
            instrument_id=instrument_id,
            order_side=order_side,
            last_qty=last_qty,
            last_px=last_px,
            currency=quote_currency,
            is_inverse=is_inverse,
            commission=commission,
            liquidity_side=liquidity_side,
            execution_ns=timestamp_ns,
            event_id=self._uuid_factory.generate(),
            timestamp_ns=self._clock.timestamp_ns(),
        )

        self._handle_event(fill)

# -- EVENT HANDLERS --------------------------------------------------------------------------------

    cdef void _handle_event(self, Event event) except *:
        self._engine.process(event)
