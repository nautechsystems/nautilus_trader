# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from decimal import Decimal

from nautilus_trader.core import nautilus_pyo3

# This needs to be a Python import so it can used in the FSM
from nautilus_trader.model.enums import order_status_to_str

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.rust.model cimport FIXED_SCALAR
from nautilus_trader.core.rust.model cimport ContingencyType
from nautilus_trader.core.rust.model cimport LiquiditySide
from nautilus_trader.core.rust.model cimport OrderSide
from nautilus_trader.core.rust.model cimport OrderStatus
from nautilus_trader.core.rust.model cimport OrderType
from nautilus_trader.core.rust.model cimport PositionSide
from nautilus_trader.core.rust.model cimport PriceRaw
from nautilus_trader.core.rust.model cimport QuantityRaw
from nautilus_trader.model.events.order cimport OrderAccepted
from nautilus_trader.model.events.order cimport OrderCanceled
from nautilus_trader.model.events.order cimport OrderCancelRejected
from nautilus_trader.model.events.order cimport OrderDenied
from nautilus_trader.model.events.order cimport OrderEmulated
from nautilus_trader.model.events.order cimport OrderEvent
from nautilus_trader.model.events.order cimport OrderExpired
from nautilus_trader.model.events.order cimport OrderFilled
from nautilus_trader.model.events.order cimport OrderInitialized
from nautilus_trader.model.events.order cimport OrderModifyRejected
from nautilus_trader.model.events.order cimport OrderPendingCancel
from nautilus_trader.model.events.order cimport OrderPendingUpdate
from nautilus_trader.model.events.order cimport OrderRejected
from nautilus_trader.model.events.order cimport OrderReleased
from nautilus_trader.model.events.order cimport OrderSubmitted
from nautilus_trader.model.events.order cimport OrderTriggered
from nautilus_trader.model.events.order cimport OrderUpdated
from nautilus_trader.model.functions cimport contingency_type_to_str
from nautilus_trader.model.functions cimport order_side_to_pyo3
from nautilus_trader.model.functions cimport order_side_to_str
from nautilus_trader.model.functions cimport order_status_to_pyo3
from nautilus_trader.model.functions cimport order_type_to_pyo3
from nautilus_trader.model.functions cimport order_type_to_str
from nautilus_trader.model.functions cimport position_side_to_str
from nautilus_trader.model.functions cimport time_in_force_to_pyo3
from nautilus_trader.model.functions cimport time_in_force_to_str
from nautilus_trader.model.identifiers cimport TradeId
from nautilus_trader.model.objects cimport Currency
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Quantity


STOP_ORDER_TYPES = {
    OrderType.STOP_MARKET,
    OrderType.STOP_LIMIT,
    OrderType.MARKET_IF_TOUCHED,
    OrderType.LIMIT_IF_TOUCHED,
}

LIMIT_ORDER_TYPES = {
    OrderType.LIMIT,
    OrderType.STOP_LIMIT,
    OrderType.LIMIT_IF_TOUCHED,
    OrderType.MARKET_TO_LIMIT,
}

CANCELLABLE_ORDER_STATUSES = {
    OrderStatus.ACCEPTED,
    OrderStatus.TRIGGERED,
    OrderStatus.PENDING_UPDATE,
    OrderStatus.PARTIALLY_FILLED,
}

LOCAL_ACTIVE_ORDER_STATUSES =  {
    OrderStatus.INITIALIZED,
    OrderStatus.EMULATED,
    OrderStatus.RELEASED,
}

# OrderStatus being used as trigger
cdef dict _ORDER_STATE_TABLE = {
    (OrderStatus.INITIALIZED, OrderStatus.DENIED): OrderStatus.DENIED,
    (OrderStatus.INITIALIZED, OrderStatus.EMULATED): OrderStatus.EMULATED,  # Emulated orders
    (OrderStatus.INITIALIZED, OrderStatus.RELEASED): OrderStatus.RELEASED,  # Emulated orders
    (OrderStatus.INITIALIZED, OrderStatus.SUBMITTED): OrderStatus.SUBMITTED,
    (OrderStatus.INITIALIZED, OrderStatus.REJECTED): OrderStatus.REJECTED,  # External orders
    (OrderStatus.INITIALIZED, OrderStatus.ACCEPTED): OrderStatus.ACCEPTED,  # External orders
    (OrderStatus.INITIALIZED, OrderStatus.CANCELED): OrderStatus.CANCELED,  # External orders
    (OrderStatus.INITIALIZED, OrderStatus.EXPIRED): OrderStatus.EXPIRED,  # External orders
    (OrderStatus.INITIALIZED, OrderStatus.TRIGGERED): OrderStatus.TRIGGERED,  # External orders
    (OrderStatus.EMULATED, OrderStatus.CANCELED): OrderStatus.CANCELED,  # Emulated orders
    (OrderStatus.EMULATED, OrderStatus.EXPIRED): OrderStatus.EXPIRED,  # Emulated orders
    (OrderStatus.EMULATED, OrderStatus.RELEASED): OrderStatus.RELEASED,  # Emulated orders
    (OrderStatus.RELEASED, OrderStatus.DENIED): OrderStatus.DENIED,  # Emulated orders
    (OrderStatus.RELEASED, OrderStatus.SUBMITTED): OrderStatus.SUBMITTED,  # Emulated orders
    (OrderStatus.RELEASED, OrderStatus.CANCELED): OrderStatus.CANCELED,  # Execution Algo
    (OrderStatus.SUBMITTED, OrderStatus.PENDING_UPDATE): OrderStatus.PENDING_UPDATE,
    (OrderStatus.SUBMITTED, OrderStatus.PENDING_CANCEL): OrderStatus.PENDING_CANCEL,
    (OrderStatus.SUBMITTED, OrderStatus.REJECTED): OrderStatus.REJECTED,
    (OrderStatus.SUBMITTED, OrderStatus.CANCELED): OrderStatus.CANCELED,  # FOK and IOC cases
    (OrderStatus.SUBMITTED, OrderStatus.ACCEPTED): OrderStatus.ACCEPTED,
    (OrderStatus.SUBMITTED, OrderStatus.PARTIALLY_FILLED): OrderStatus.PARTIALLY_FILLED,
    (OrderStatus.SUBMITTED, OrderStatus.FILLED): OrderStatus.FILLED,
    (OrderStatus.ACCEPTED, OrderStatus.REJECTED): OrderStatus.REJECTED,  # StopLimit order
    (OrderStatus.ACCEPTED, OrderStatus.PENDING_UPDATE): OrderStatus.PENDING_UPDATE,
    (OrderStatus.ACCEPTED, OrderStatus.PENDING_CANCEL): OrderStatus.PENDING_CANCEL,
    (OrderStatus.ACCEPTED, OrderStatus.CANCELED): OrderStatus.CANCELED,
    (OrderStatus.ACCEPTED, OrderStatus.TRIGGERED): OrderStatus.TRIGGERED,
    (OrderStatus.ACCEPTED, OrderStatus.EXPIRED): OrderStatus.EXPIRED,
    (OrderStatus.ACCEPTED, OrderStatus.PARTIALLY_FILLED): OrderStatus.PARTIALLY_FILLED,
    (OrderStatus.ACCEPTED, OrderStatus.FILLED): OrderStatus.FILLED,
    (OrderStatus.CANCELED, OrderStatus.PARTIALLY_FILLED): OrderStatus.PARTIALLY_FILLED,  # Real world possibility
    (OrderStatus.CANCELED, OrderStatus.FILLED): OrderStatus.FILLED,  # Real world possibility
    (OrderStatus.PENDING_UPDATE, OrderStatus.REJECTED): OrderStatus.REJECTED,  # Real world possibility
    (OrderStatus.PENDING_UPDATE, OrderStatus.ACCEPTED): OrderStatus.ACCEPTED,
    (OrderStatus.PENDING_UPDATE, OrderStatus.CANCELED): OrderStatus.CANCELED,
    (OrderStatus.PENDING_UPDATE, OrderStatus.EXPIRED): OrderStatus.EXPIRED,
    (OrderStatus.PENDING_UPDATE, OrderStatus.TRIGGERED): OrderStatus.TRIGGERED,
    (OrderStatus.PENDING_UPDATE, OrderStatus.SUBMITTED): OrderStatus.PENDING_UPDATE,  # Real world possibility
    (OrderStatus.PENDING_UPDATE, OrderStatus.PENDING_UPDATE): OrderStatus.PENDING_UPDATE,  # Allow multiple requests
    (OrderStatus.PENDING_UPDATE, OrderStatus.PENDING_CANCEL): OrderStatus.PENDING_CANCEL,
    (OrderStatus.PENDING_UPDATE, OrderStatus.PARTIALLY_FILLED): OrderStatus.PARTIALLY_FILLED,
    (OrderStatus.PENDING_UPDATE, OrderStatus.FILLED): OrderStatus.FILLED,
    (OrderStatus.PENDING_CANCEL, OrderStatus.REJECTED): OrderStatus.REJECTED,  # Real world possibility
    (OrderStatus.PENDING_CANCEL, OrderStatus.PENDING_CANCEL): OrderStatus.PENDING_CANCEL,  # Allow multiple requests
    (OrderStatus.PENDING_CANCEL, OrderStatus.CANCELED): OrderStatus.CANCELED,
    (OrderStatus.PENDING_CANCEL, OrderStatus.EXPIRED): OrderStatus.EXPIRED,
    (OrderStatus.PENDING_CANCEL, OrderStatus.ACCEPTED): OrderStatus.ACCEPTED,  # Allows failed cancel requests
    (OrderStatus.PENDING_CANCEL, OrderStatus.PARTIALLY_FILLED): OrderStatus.PARTIALLY_FILLED,
    (OrderStatus.PENDING_CANCEL, OrderStatus.FILLED): OrderStatus.FILLED,
    (OrderStatus.TRIGGERED, OrderStatus.REJECTED): OrderStatus.REJECTED,
    (OrderStatus.TRIGGERED, OrderStatus.PENDING_UPDATE): OrderStatus.PENDING_UPDATE,
    (OrderStatus.TRIGGERED, OrderStatus.PENDING_CANCEL): OrderStatus.PENDING_CANCEL,
    (OrderStatus.TRIGGERED, OrderStatus.CANCELED): OrderStatus.CANCELED,
    (OrderStatus.TRIGGERED, OrderStatus.EXPIRED): OrderStatus.EXPIRED,
    (OrderStatus.TRIGGERED, OrderStatus.PARTIALLY_FILLED): OrderStatus.PARTIALLY_FILLED,
    (OrderStatus.TRIGGERED, OrderStatus.FILLED): OrderStatus.FILLED,
    (OrderStatus.PARTIALLY_FILLED, OrderStatus.PENDING_UPDATE): OrderStatus.PENDING_UPDATE,
    (OrderStatus.PARTIALLY_FILLED, OrderStatus.PENDING_CANCEL): OrderStatus.PENDING_CANCEL,
    (OrderStatus.PARTIALLY_FILLED, OrderStatus.CANCELED): OrderStatus.CANCELED,
    (OrderStatus.PARTIALLY_FILLED, OrderStatus.EXPIRED): OrderStatus.EXPIRED,
    (OrderStatus.PARTIALLY_FILLED, OrderStatus.PARTIALLY_FILLED): OrderStatus.PARTIALLY_FILLED,
    (OrderStatus.PARTIALLY_FILLED, OrderStatus.FILLED): OrderStatus.FILLED,
}


cdef class Order:
    """
    The base class for all orders.

    Parameters
    ----------
    init : OrderInitialized
        The order initialized event.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(self, OrderInitialized init not None):
        Condition.positive(init.quantity, "init.quantity")

        self._events: list[OrderEvent] = [init]
        self._venue_order_ids: list[VenueOrderId] = []
        self._trade_ids: list[TradeId] = []
        self._commissions: dict[Currency, Money] = {}
        self._fsm = FiniteStateMachine(
            state_transition_table=_ORDER_STATE_TABLE,
            initial_state=OrderStatus.INITIALIZED,
            trigger_parser=order_status_to_str,
            state_parser=order_status_to_str,
        )
        self._previous_status = OrderStatus.INITIALIZED
        self._triggered_price = None  # Can be None

        # Identifiers
        self.trader_id = init.trader_id
        self.strategy_id = init.strategy_id
        self.instrument_id = init.instrument_id
        self.client_order_id = init.client_order_id
        self.venue_order_id = None  # Can be None
        self.position_id = None  # Can be None
        self.account_id = None  # Can be None
        self.last_trade_id = None  # Can be None

        # Properties
        self.side = init.side
        self.order_type = init.order_type
        self.quantity = init.quantity
        self.time_in_force = init.time_in_force
        self.liquidity_side = LiquiditySide.NO_LIQUIDITY_SIDE
        self.is_post_only = init.post_only
        self.is_reduce_only = init.reduce_only
        self.is_quote_quantity = init.quote_quantity
        self.emulation_trigger = init.emulation_trigger
        self.trigger_instrument_id = init.trigger_instrument_id
        self.contingency_type = init.contingency_type
        self.order_list_id = init.order_list_id  # Can be None
        self.linked_order_ids = init.linked_order_ids  # Can be None
        self.parent_order_id = init.parent_order_id  # Can be None
        self.exec_algorithm_id = init.exec_algorithm_id  # Can be None
        self.exec_algorithm_params = init.exec_algorithm_params  # Can be None
        self.exec_spawn_id = init.exec_spawn_id  # Can be None
        self.tags = init.tags

        # Execution
        self.filled_qty = Quantity.zero_c(self.quantity._mem.precision)
        self.leaves_qty = init.quantity
        self.avg_px = 0.0  # No fills yet
        self.slippage = 0.0

        # Timestamps
        self.init_id = init.id
        self.ts_init = init.ts_init
        self.ts_submitted = 0
        self.ts_accepted = 0
        self.ts_closed = 0
        self.ts_last = init.ts_init

    def __eq__(self, Order other) -> bool:
        return self.client_order_id == other.client_order_id

    def __hash__(self) -> int:
        return hash(self.client_order_id)

    def __repr__(self) -> str:
        cdef ClientOrderId coi
        cdef str contingency_str = "" if self.contingency_type == ContingencyType.NO_CONTINGENCY else f", contingency_type={contingency_type_to_str(self.contingency_type)}"
        cdef str linked_order_ids_str = "" if self.linked_order_ids is None else f", linked_order_ids=[{', '.join([coi.to_str() for coi in self.linked_order_ids])}]" if self.linked_order_ids is not None else None  # noqa
        cdef str parent_order_id_str = "" if self.parent_order_id is None else f", parent_order_id={self.parent_order_id.to_str()}"
        cdef str exec_algorithm_id_str = "" if self.exec_algorithm_id is None else f", exec_algorithm_id={self.exec_algorithm_id.to_str()}"
        cdef str exec_algorithm_params_str = "" if self.exec_algorithm_params is None else f", exec_algorithm_params={self.exec_algorithm_params}"
        cdef str exec_spawn_id_str = "" if self.exec_spawn_id is None else f", exec_spawn_id={self.exec_spawn_id.to_str()}"
        return (
            f"{type(self).__name__}("
            f"{self.info()}, "
            f"status={self._fsm.state_string_c()}, "
            f"client_order_id={self.client_order_id.to_str()}, "
            f"venue_order_id={self.venue_order_id}, "  # Can be None (no whitespace before contingency_str)
            f"position_id={self.position_id}"  # Can be None (no whitespace before contingency_str)
            f"{contingency_str}"
            f"{linked_order_ids_str}"
            f"{parent_order_id_str}"
            f"{exec_algorithm_id_str}"
            f"{exec_algorithm_params_str}"
            f"{exec_spawn_id_str}"
            f", tags={self.tags})"
        )

    cpdef str status_string(self):
        """
        Return the orders current status as a string.

        Returns
        -------
        str

        """
        return self.status_string_c()

    cpdef str side_string(self):
        """
        Return the orders side as a string.

        Returns
        -------
        str

        """
        return self.side_string_c()

    cpdef str type_string(self):
        """
        Return the orders type as a string.

        Returns
        -------
        str

        """
        return self.type_string_c()

    cpdef str tif_string(self):
        """
        Return the orders time in force as a string.

        Returns
        -------
        str

        """
        return self.tif_string_c()

    cpdef str info(self):
        """
        Return a summary description of the order.

        Returns
        -------
        str

        """
        raise NotImplementedError("method `info` must be implemented in the subclass")  # pragma: no cover

    cpdef dict to_dict(self):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        raise NotImplementedError("method `to_dict` must be implemented in the subclass")  # pragma: no cover

    cpdef void set_quote_quantity(self, bint value):
        self.is_quote_quantity = value

    cdef void set_activated_c(self, Price activation_price):
        raise NotImplementedError("method `set_activated` must be implemented in the subclass")  # pragma: no cover

    cdef void set_triggered_price_c(self, Price triggered_price):
        Condition.not_none(triggered_price, "triggered_price")
        self._triggered_price = triggered_price

    cdef Price get_triggered_price_c(self):
        return self._triggered_price

    cdef OrderStatus status_c(self):
        return <OrderStatus>self._fsm.state

    cdef OrderInitialized init_event_c(self):
        return self._events[0]  # Guaranteed to contain the initialized event

    cdef OrderEvent last_event_c(self):
        return self._events[-1]  # Guaranteed to contain the initialized event

    cdef list events_c(self):
        return self._events.copy()

    cdef list venue_order_ids_c(self):
        return self._venue_order_ids.copy()

    cdef list trade_ids_c(self):
        return self._trade_ids.copy()

    cdef int event_count_c(self):
        return len(self._events)

    cdef str status_string_c(self):
        return self._fsm.state_string_c()

    cdef str type_string_c(self):
        return order_type_to_str(self.order_type)

    cdef str side_string_c(self):
        return order_side_to_str(self.side)

    cdef str tif_string_c(self):
        return time_in_force_to_str(self.time_in_force)

    cdef bint has_price_c(self):
        raise NotImplementedError("method `has_price_c` must be implemented in subclass")  # pragma: no cover

    cdef bint has_activation_price_c(self):
        return False

    cdef bint has_trigger_price_c(self):
        raise NotImplementedError("method `has_trigger_price_c` must be implemented in subclass")  # pragma: no cover

    cdef bint is_buy_c(self):
        return self.side == OrderSide.BUY

    cdef bint is_sell_c(self):
        return self.side == OrderSide.SELL

    cdef bint is_passive_c(self):
        return self.order_type != OrderType.MARKET

    cdef bint is_aggressive_c(self):
        return self.order_type == OrderType.MARKET

    cdef bint is_emulated_c(self):
        return self._fsm.state == OrderStatus.EMULATED

    cdef bint is_active_local_c(self):
        return self._fsm.state in LOCAL_ACTIVE_ORDER_STATUSES

    cdef bint is_primary_c(self):
        return self.exec_algorithm_id is not None and self.exec_spawn_id == self.client_order_id

    cdef bint is_spawned_c(self):
        return self.exec_algorithm_id is not None and self.exec_spawn_id != self.client_order_id

    cdef bint is_contingency_c(self):
        return self.contingency_type != ContingencyType.NO_CONTINGENCY

    cdef bint is_parent_order_c(self):
        return self.contingency_type == ContingencyType.OTO

    cdef bint is_child_order_c(self):
        return self.parent_order_id is not None

    cdef bint is_open_c(self):
        if self.emulation_trigger != TriggerType.NO_TRIGGER:
            return False
        return (
            self._fsm.state == OrderStatus.ACCEPTED
            or self._fsm.state == OrderStatus.TRIGGERED
            or self._fsm.state == OrderStatus.PENDING_CANCEL
            or self._fsm.state == OrderStatus.PENDING_UPDATE
            or self._fsm.state == OrderStatus.PARTIALLY_FILLED
        )

    cdef bint is_canceled_c(self):
        return self._fsm.state == OrderStatus.CANCELED

    cdef bint is_closed_c(self):
        return (
            self._fsm.state == OrderStatus.DENIED
            or self._fsm.state == OrderStatus.REJECTED
            or self._fsm.state == OrderStatus.CANCELED
            or self._fsm.state == OrderStatus.EXPIRED
            or self._fsm.state == OrderStatus.FILLED
        )

    cdef bint is_inflight_c(self):
        return (
            self._fsm.state == OrderStatus.SUBMITTED
            or self._fsm.state == OrderStatus.PENDING_CANCEL
            or self._fsm.state == OrderStatus.PENDING_UPDATE
        )

    cdef bint is_pending_update_c(self):
        return self._fsm.state == OrderStatus.PENDING_UPDATE

    cdef bint is_pending_cancel_c(self):
        return self._fsm.state == OrderStatus.PENDING_CANCEL

    def to_own_book_order(self) -> nautilus_pyo3.OwnBookOrder:
        """
        Returns an own/user order representation of this order.

        Returns
        -------
        nautilus_pyo3.OwnBookOrder

        """
        if not self.has_price_c():
            raise TypeError(f"Cannot initialize {self.type_string_c()} order as `nautilus_pyo3.OwnBookOrder`, no price")

        cdef Price price = self.price
        return nautilus_pyo3.OwnBookOrder(
            trader_id=nautilus_pyo3.TraderId(self.trader_id.value),
            client_order_id=nautilus_pyo3.ClientOrderId(self.client_order_id.value),
            venue_order_id=nautilus_pyo3.VenueOrderId(self.venue_order_id.value) if self.venue_order_id else None,
            side=order_side_to_pyo3(self.side),
            price=nautilus_pyo3.Price(price.as_f64_c(), price._mem.precision),
            size=nautilus_pyo3.Quantity(self.leaves_qty.as_f64_c(), self.leaves_qty._mem.precision),
            order_type=order_type_to_pyo3(self.order_type),
            time_in_force=time_in_force_to_pyo3(self.time_in_force),
            status=order_status_to_pyo3(<OrderStatus>self._fsm.state),
            ts_last=self.ts_last,
            ts_accepted=self.ts_accepted,
            ts_submitted=self.ts_submitted,
            ts_init=self.ts_init,
        )

    @property
    def symbol(self):
        """
        Return the orders ticker symbol.

        Returns
        -------
        Symbol

        """
        return self.instrument_id.symbol

    @property
    def venue(self):
        """
        Return the orders trading venue.

        Returns
        -------
        Venue

        """
        return self.instrument_id.venue

    @property
    def status(self):
        """
        Return the orders current status.

        Returns
        -------
        OrderStatus

        """
        return self.status_c()

    @property
    def init_event(self):
        """
        Return the initialization event for the order.

        Returns
        -------
        OrderInitialized

        """
        return self.init_event_c()

    @property
    def last_event(self):
        """
        Return the last event applied to the order.

        Returns
        -------
        OrderEvent

        """
        return self.last_event_c()

    @property
    def events(self):
        """
        Return the order events.

        Returns
        -------
        list[OrderEvent]

        """
        return self.events_c()

    @property
    def venue_order_ids(self):
        """
        Return the venue order IDs.

        Returns
        -------
        list[VenueOrderId]

        """
        return self.venue_order_ids_c().copy()

    @property
    def trade_ids(self):
        """
        Return the trade match IDs.

        Returns
        -------
        list[TradeId]

        """
        return self.trade_ids_c()

    @property
    def event_count(self):
        """
        Return the count of events applied to the order.

        Returns
        -------
        int

        """
        return self.event_count_c()

    @property
    def has_price(self):
        """
        Return whether the order has a `price` property.

        Returns
        -------
        bool

        """
        return self.has_price_c()

    @property
    def has_trigger_price(self):
        """
        Return whether the order has a `trigger_price` property.

        Returns
        -------
        bool

        """
        return self.has_trigger_price_c()

    @property
    def has_activation_price(self):
        """
        Return whether the order has a `activation_price` property.

        Returns
        -------
        bool

        """
        return self.has_activation_price_c()

    @property
    def is_buy(self):
        """
        Return whether the order side is ``BUY``.

        Returns
        -------
        bool

        """
        return self.is_buy_c()

    @property
    def is_sell(self):
        """
        Return whether the order side is ``SELL``.

        Returns
        -------
        bool

        """
        return self.is_sell_c()

    @property
    def is_passive(self):
        """
        Return whether the order is passive (`order_type` **not** ``MARKET``).

        Returns
        -------
        bool

        """
        return self.is_passive_c()

    @property
    def is_aggressive(self):
        """
        Return whether the order is aggressive (`order_type` is ``MARKET``).

        Returns
        -------
        bool

        """
        return self.is_aggressive_c()

    @property
    def is_emulated(self):
        """
        Return whether the order is emulated and held in the local system.

        Returns
        -------
        bool

        """
        return self.is_emulated_c()

    @property
    def is_active_local(self):
        """
        Return whether the order is active and held in the local system.

        An order is considered active local when its status is any of:
        - ``INITIALIZED``
        - ``EMULATED``
        - ``RELEASED``

        Returns
        -------
        bool

        """
        return self.is_active_local_c()

    @property
    def is_primary(self):
        """
        Return whether the order is the primary for an execution algorithm sequence.

        Returns
        -------
        bool

        """
        return self.is_primary_c()

    @property
    def is_spawned(self):
        """
        Return whether the order was spawned as part of an execution algorithm sequence.

        Returns
        -------
        bool

        """
        return self.is_spawned_c()

    @property
    def is_contingency(self):
        """
        Return whether the order has a contingency (`contingency_type` is not ``NO_CONTINGENCY``).

        Returns
        -------
        bool

        """
        return self.is_contingency_c()

    @property
    def is_parent_order(self):
        """
        Return whether the order has **at least** one child order.

        Returns
        -------
        bool

        """
        return self.is_parent_order_c()

    @property
    def is_child_order(self):
        """
        Return whether the order has a parent order.

        Returns
        -------
        bool

        """
        return self.is_child_order_c()

    @property
    def is_inflight(self):
        """
        Return whether the order is in-flight (order request sent to the trading venue).

        An order is considered in-flight when its status is any of:
        - ``SUBMITTED``
        - ``PENDING_UPDATE``
        - ``PENDING_CANCEL``

        Returns
        -------
        bool

        Warnings
        --------
        An emulated order is never considered in-flight.

        """
        return self.is_inflight_c()

    @property
    def is_open(self):
        """
        Return whether the order is open at the trading venue.

        An order is considered open when its status is any of:
        - ``ACCEPTED``
        - ``TRIGGERED``
        - ``PENDING_UPDATE``
        - ``PENDING_CANCEL``
        - ``PARTIALLY_FILLED``

        Returns
        -------
        bool

        Warnings
        --------
        An emulated order is never considered open.

        """
        return self.is_open_c()

    @property
    def is_canceled(self):
        """
        Return whether current `status` is ``CANCELED``.

        Returns
        -------
        bool

        """
        return self.is_canceled_c()

    @property
    def is_closed(self):
        """
        Return whether the order is closed (lifecycle completed).

        An order is considered closed when its status can no longer change.
        The possible statuses of closed orders include;

        - ``DENIED``
        - ``REJECTED``
        - ``CANCELED``
        - ``EXPIRED``
        - ``FILLED``

        Returns
        -------
        bool

        """
        return self.is_closed_c()

    @property
    def is_pending_update(self):
        """
        Return whether the current `status` is ``PENDING_UPDATE``.

        Returns
        -------
        bool

        """
        return self.is_pending_update_c()

    @property
    def is_pending_cancel(self):
        """
        Return whether the current `status` is ``PENDING_CANCEL``.

        Returns
        -------
        bool

        """
        return self.is_pending_cancel_c()

    @staticmethod
    cdef OrderSide opposite_side_c(OrderSide side):
        if side == OrderSide.BUY:
            return OrderSide.SELL
        elif side == OrderSide.SELL:
            return OrderSide.BUY
        else:
            raise ValueError(  # pragma: no cover (design-time error)
                f"invalid `OrderSide`, was {order_side_to_str(side)}",  # pragma: no cover (design-time error)
            )

    @staticmethod
    cdef OrderSide closing_side_c(PositionSide position_side):
        if position_side == PositionSide.LONG:
            return OrderSide.SELL
        elif position_side == PositionSide.SHORT:
            return OrderSide.BUY
        else:
            raise ValueError(  # pragma: no cover (design-time error)
                f"invalid `PositionSide`, was {position_side_to_str(position_side)}",  # pragma: no cover (design-time error)  # noqa
            )

    @staticmethod
    def opposite_side(OrderSide side) -> OrderSide:
        """
        Return the opposite order side from the given side.

        Parameters
        ----------
        side : OrderSide {``BUY``, ``SELL``}
            The original order side.

        Returns
        -------
        OrderSide

        Raises
        ------
        ValueError
            If `side` is invalid.

        """
        return Order.opposite_side_c(side)

    @staticmethod
    def closing_side(PositionSide position_side) -> OrderSide:
        """
        Return the order side needed to close a position with the given side.

        Parameters
        ----------
        position_side : PositionSide {``LONG``, ``SHORT``}
            The side of the position to close.

        Returns
        -------
        OrderSide

        Raises
        ------
        ValueError
            If `position_side` is ``FLAT`` or invalid.

        """
        return Order.closing_side_c(position_side)

    cpdef signed_decimal_qty(self):
        """
        Return a signed decimal representation of the remaining quantity.

         - If the order is a BUY, the value is positive (e.g. Decimal('10.25'))
         - If the order is a SELL, the value is negative (e.g. Decimal('-10.25'))

        Returns
        -------
        Decimal

        """
        if self.side == OrderSide.BUY:
            return Decimal(f"{self.leaves_qty.as_f64_c():.{self.leaves_qty._mem.precision}}")
        elif self.side == OrderSide.SELL:
            return -Decimal(f"{self.leaves_qty.as_f64_c():.{self.leaves_qty._mem.precision}}")
        else:
            raise ValueError(  # pragma: no cover (design-time error)
                f"invalid `OrderSide`, was {order_side_to_str(self.side)}",  # pragma: no cover (design-time error)
            )

    cpdef bint would_reduce_only(self, PositionSide position_side, Quantity position_qty):
        """
        Whether the current order would only reduce the given position if applied
        in full.

        Parameters
        ----------
        position_side : PositionSide {``FLAT``, ``LONG``, ``SHORT``}
            The side of the position to check against.
        position_qty : Quantity
            The quantity of the position to check against.

        Returns
        -------
        bool

        """
        Condition.not_none(position_qty, "position_qty")

        if position_side == PositionSide.FLAT:
            return False  # Would increase position

        if self.side == OrderSide.BUY:
            if position_side == PositionSide.LONG:
                return False  # Would increase position
            elif position_side == PositionSide.SHORT and self.leaves_qty._mem.raw > position_qty._mem.raw:
                return False  # Would increase position
        elif self.side == OrderSide.SELL:
            if position_side == PositionSide.SHORT:
                return False  # Would increase position
            elif position_side == PositionSide.LONG and self.leaves_qty._mem.raw > position_qty._mem.raw:
                return False  # Would increase position

        return True  # Would reduce only

    cpdef list commissions(self):
        """
        Return the total commissions generated by the order.

        Returns
        -------
        list[Money]

        """
        return sorted(self._commissions.values())

    cpdef void apply(self, OrderEvent event):
        """
        Apply the given order event to the order.

        Parameters
        ----------
        event : OrderEvent
            The order event to apply.

        Raises
        ------
        ValueError
            If `self.client_order_id` is not equal to `event.client_order_id`.
        ValueError
            If `self.venue_order_id` and `event.venue_order_id` are both not ``None``, and are not equal.
        InvalidStateTrigger
            If `event` is not a valid trigger from the current `order.status`.
        KeyError
            If `event` is `OrderFilled` and `event.trade_id` already applied to the order.

        """
        Condition.not_none(event, "event")
        Condition.equal(event.client_order_id, self.client_order_id, "event.client_order_id", "self.client_order_id")
        if self.venue_order_id is not None and event.venue_order_id is not None and not isinstance(event, OrderUpdated):
            Condition.equal(self.venue_order_id, event.venue_order_id, "self.venue_order_id", "event.venue_order_id")

        cdef OrderStatus previous_status = <OrderStatus>self._fsm.state

        # Handle event (FSM can raise InvalidStateTrigger)
        if isinstance(event, OrderInitialized):
            pass  # Do nothing else
        elif isinstance(event, OrderDenied):
            self._fsm.trigger(OrderStatus.DENIED)
            self._denied(event)
        elif isinstance(event, OrderEmulated):
            self._fsm.trigger(OrderStatus.EMULATED)
            # self._emulated(event)
        elif isinstance(event, OrderReleased):
            self._fsm.trigger(OrderStatus.RELEASED)
            self.emulation_trigger = TriggerType.NO_TRIGGER
            # self._released(event)
        elif isinstance(event, OrderSubmitted):
            self._fsm.trigger(OrderStatus.SUBMITTED)
            self._submitted(event)
        elif isinstance(event, OrderRejected):
            self._fsm.trigger(OrderStatus.REJECTED)
            self._rejected(event)
        elif isinstance(event, OrderAccepted):
            self._fsm.trigger(OrderStatus.ACCEPTED)
            self._accepted(event)
        elif isinstance(event, OrderPendingUpdate):
            self._fsm.trigger(OrderStatus.PENDING_UPDATE)
        elif isinstance(event, OrderPendingCancel):
            self._fsm.trigger(OrderStatus.PENDING_CANCEL)
        elif isinstance(event, OrderModifyRejected):
            if self._fsm.state == OrderStatus.PENDING_UPDATE:
                self._fsm.trigger(self._previous_status)
        elif isinstance(event, OrderCancelRejected):
            if self._fsm.state == OrderStatus.PENDING_CANCEL:
                self._fsm.trigger(self._previous_status)
        elif isinstance(event, OrderUpdated):
            if self._fsm.state == OrderStatus.PENDING_UPDATE:
                self._fsm.trigger(self._previous_status)
            self._updated(event)
        elif isinstance(event, OrderTriggered):
            Condition.is_true(
                (
                    self.order_type == OrderType.STOP_LIMIT
                    or self.order_type == OrderType.TRAILING_STOP_LIMIT
                    or self.order_type == OrderType.LIMIT_IF_TOUCHED
                ),
                "can only trigger STOP_LIMIT, TRAILING_STOP_LIMIT and LIMIT_IF_TOUCHED orders",
            )
            self._fsm.trigger(OrderStatus.TRIGGERED)
            self._triggered(event)
        elif isinstance(event, OrderCanceled):
            self._fsm.trigger(OrderStatus.CANCELED)
            self._canceled(event)
        elif isinstance(event, OrderExpired):
            self._fsm.trigger(OrderStatus.EXPIRED)
            self._expired(event)
        elif isinstance(event, OrderFilled):
            # Check identifiers
            if self.venue_order_id is None:
                self.venue_order_id = event.venue_order_id
            else:
                Condition.not_in(event.trade_id, self._trade_ids, "event.trade_id", "_trade_ids")
            # Fill order
            self._filled(event)
        else:
            raise ValueError(  # pragma: no cover (design-time error)
                f"invalid `OrderEvent`, was {type(event)}",  # pragma: no cover (design-time error)
            )

        # Update previous status and events last as FSM may raise `InvalidStateTrigger`
        if previous_status != OrderStatus.PENDING_UPDATE and previous_status != OrderStatus.PENDING_CANCEL:
            self._previous_status = previous_status

        self._events.append(event)
        self.ts_last = event.ts_event

    cdef void _denied(self, OrderDenied event):
        self.ts_closed = event.ts_event

    cdef void _submitted(self, OrderSubmitted event):
        self.account_id = event.account_id
        self.ts_submitted = event.ts_event

    cdef void _rejected(self, OrderRejected event):
        self.ts_closed = event.ts_event

    cdef void _accepted(self, OrderAccepted event):
        self.venue_order_id = event.venue_order_id
        self.ts_accepted = event.ts_event

    cdef void _updated(self, OrderUpdated event):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `_updated` must be implemented in the subclass")  # pragma: no cover

    cdef void _triggered(self, OrderTriggered event):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `_triggered` must be implemented in the subclass")  # pragma: no cover

    cdef void _canceled(self, OrderCanceled event):
        self.ts_closed = event.ts_event

    cdef void _expired(self, OrderExpired event):
        self.ts_closed = event.ts_event

    cdef void _filled(self, OrderFilled fill):
        if self.filled_qty._mem.raw + fill.last_qty._mem.raw < self.quantity._mem.raw:
            self._fsm.trigger(OrderStatus.PARTIALLY_FILLED)
        else:
            self._fsm.trigger(OrderStatus.FILLED)
            self.ts_closed = fill.ts_event

        self.venue_order_id = fill.venue_order_id
        self.position_id = fill.position_id
        self.strategy_id = fill.strategy_id
        self._trade_ids.append(fill.trade_id)
        self.last_trade_id = fill.trade_id
        if self.ts_accepted == 0:
            # Set ts_accepted to time of first fill if not previously set
            self.ts_accepted = fill.ts_event

        cdef QuantityRaw raw_filled_qty = self.filled_qty._mem.raw + fill.last_qty._mem.raw

        # Using `PriceRaw` as temporary hack to access int128_t so that negative values can be represented
        cdef PriceRaw raw_leaves_qty = self.quantity._mem.raw - raw_filled_qty
        if raw_leaves_qty < 0:
            raise ValueError(
                f"invalid order.leaves_qty: was {raw_leaves_qty / FIXED_SCALAR}, "
                f"order.quantity={self.quantity}, "
                f"order.filled_qty={self.filled_qty}, "
                f"fill.last_qty={fill.last_qty}, "
                f"fill={fill}",
            )
        self.filled_qty.add_assign(fill.last_qty)
        self.leaves_qty = Quantity.from_raw_c(<QuantityRaw>raw_leaves_qty, fill.last_qty._mem.precision)
        self.avg_px = self._calculate_avg_px(fill.last_qty.as_f64_c(), fill.last_px.as_f64_c())
        self.liquidity_side = fill.liquidity_side
        self._set_slippage()

        # Calculate cumulative commission
        cdef Currency currency = fill.commission.currency
        cdef Money commissions = self._commissions.get(currency)
        cdef double total_commissions = commissions.as_f64_c() if commissions is not None else 0.0
        self._commissions[currency] = Money(total_commissions + fill.commission.as_f64_c(), currency)

    cdef double _calculate_avg_px(self, double last_qty, double last_px):
        if self.avg_px == 0.0:
            return last_px

        # Use previous filled quantity (before current fill) to avoid double-counting
        # self.filled_qty already includes last_qty at this point
        cdef double filled_qty_f64 = self.filled_qty.as_f64_c()
        cdef double prev_filled_qty = filled_qty_f64 - last_qty
        if filled_qty_f64 > 0:  # Protect divide by zero
            return ((self.avg_px * prev_filled_qty) + (last_px * last_qty)) / filled_qty_f64

    cdef void _set_slippage(self):
        pass  # Optionally implement

    @staticmethod
    cdef void _hydrate_initial_events(Order original, Order transformed):
        cdef list original_events = original.events_c()

        cdef OrderEvent event
        for event in reversed(original_events):
            # Insert each event to the beginning of the events list in reverse
            # to preserve correct order of events.
            transformed._events.insert(0, event)
