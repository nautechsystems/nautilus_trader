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
Defines various order types used for trading.
"""

from decimal import Decimal

import pandas as pd
from cpython.datetime cimport datetime
from libc.stdint cimport int64_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport format_iso8601
from nautilus_trader.core.datetime cimport maybe_dt_to_unix_nanos
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.order_side cimport OrderSideParser
from nautilus_trader.model.c_enums.order_status cimport OrderStatus
from nautilus_trader.model.c_enums.order_status cimport OrderStatusParser
from nautilus_trader.model.c_enums.order_type cimport OrderType
from nautilus_trader.model.c_enums.order_type cimport OrderTypeParser
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForce
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForceParser
from nautilus_trader.model.events.order cimport OrderAccepted
from nautilus_trader.model.events.order cimport OrderCanceled
from nautilus_trader.model.events.order cimport OrderCancelRejected
from nautilus_trader.model.events.order cimport OrderDenied
from nautilus_trader.model.events.order cimport OrderEvent
from nautilus_trader.model.events.order cimport OrderExpired
from nautilus_trader.model.events.order cimport OrderFilled
from nautilus_trader.model.events.order cimport OrderInitialized
from nautilus_trader.model.events.order cimport OrderModifyRejected
from nautilus_trader.model.events.order cimport OrderPendingCancel
from nautilus_trader.model.events.order cimport OrderPendingUpdate
from nautilus_trader.model.events.order cimport OrderRejected
from nautilus_trader.model.events.order cimport OrderSubmitted
from nautilus_trader.model.events.order cimport OrderTriggered
from nautilus_trader.model.events.order cimport OrderUpdated
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport ExecutionId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport OrderListId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


# OrderStatus being used as trigger
cdef dict _ORDER_STATE_TABLE = {
    (OrderStatus.INITIALIZED, OrderStatus.DENIED): OrderStatus.DENIED,
    (OrderStatus.INITIALIZED, OrderStatus.SUBMITTED): OrderStatus.SUBMITTED,
    (OrderStatus.SUBMITTED, OrderStatus.REJECTED): OrderStatus.REJECTED,
    (OrderStatus.SUBMITTED, OrderStatus.ACCEPTED): OrderStatus.ACCEPTED,
    (OrderStatus.SUBMITTED, OrderStatus.PARTIALLY_FILLED): OrderStatus.PARTIALLY_FILLED,
    (OrderStatus.SUBMITTED, OrderStatus.FILLED): OrderStatus.FILLED,
    (OrderStatus.ACCEPTED, OrderStatus.REJECTED): OrderStatus.REJECTED,  # Covers StopLimit order
    (OrderStatus.ACCEPTED, OrderStatus.PENDING_UPDATE): OrderStatus.PENDING_UPDATE,
    (OrderStatus.ACCEPTED, OrderStatus.PENDING_CANCEL): OrderStatus.PENDING_CANCEL,
    (OrderStatus.ACCEPTED, OrderStatus.CANCELED): OrderStatus.CANCELED,
    (OrderStatus.ACCEPTED, OrderStatus.TRIGGERED): OrderStatus.TRIGGERED,
    (OrderStatus.ACCEPTED, OrderStatus.EXPIRED): OrderStatus.EXPIRED,
    (OrderStatus.ACCEPTED, OrderStatus.PARTIALLY_FILLED): OrderStatus.PARTIALLY_FILLED,
    (OrderStatus.ACCEPTED, OrderStatus.FILLED): OrderStatus.FILLED,
    (OrderStatus.PENDING_UPDATE, OrderStatus.ACCEPTED): OrderStatus.ACCEPTED,
    (OrderStatus.PENDING_UPDATE, OrderStatus.CANCELED): OrderStatus.CANCELED,
    (OrderStatus.PENDING_UPDATE, OrderStatus.EXPIRED): OrderStatus.EXPIRED,
    (OrderStatus.PENDING_UPDATE, OrderStatus.TRIGGERED): OrderStatus.TRIGGERED,
    (OrderStatus.PENDING_UPDATE, OrderStatus.PENDING_UPDATE): OrderStatus.PENDING_UPDATE,  # Allow multiple requests
    (OrderStatus.PENDING_UPDATE, OrderStatus.PENDING_CANCEL): OrderStatus.PENDING_CANCEL,
    (OrderStatus.PENDING_UPDATE, OrderStatus.PARTIALLY_FILLED): OrderStatus.PARTIALLY_FILLED,
    (OrderStatus.PENDING_UPDATE, OrderStatus.FILLED): OrderStatus.FILLED,
    (OrderStatus.PENDING_CANCEL, OrderStatus.PENDING_CANCEL): OrderStatus.PENDING_CANCEL,  # Allow multiple requests
    (OrderStatus.PENDING_CANCEL, OrderStatus.CANCELED): OrderStatus.CANCELED,
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
    (OrderStatus.PARTIALLY_FILLED, OrderStatus.CANCELED): OrderStatus.FILLED,
    (OrderStatus.PARTIALLY_FILLED, OrderStatus.PARTIALLY_FILLED): OrderStatus.PARTIALLY_FILLED,
    (OrderStatus.PARTIALLY_FILLED, OrderStatus.FILLED): OrderStatus.FILLED,
}


cdef class Order:
    """
    The abstract base class for all orders.

    Parameters
    ----------
    init : OrderInitialized
        The order initialized event.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(self, OrderInitialized init not None):
        self._events = [init]       # type: list[OrderEvent]
        self._venue_order_ids = []  # type: list[VenueOrderId]
        self._execution_ids = []    # type: list[ExecutionId]
        self._fsm = FiniteStateMachine(
            state_transition_table=_ORDER_STATE_TABLE,
            initial_state=OrderStatus.INITIALIZED,
            trigger_parser=OrderStatusParser.to_str,  # .to_str correct here
            state_parser=OrderStatusParser.to_str,
        )
        self._rollback_status = OrderStatus.INITIALIZED

        # Identifiers
        self.trader_id = init.trader_id
        self.strategy_id = init.strategy_id
        self.instrument_id = init.instrument_id
        self.client_order_id = init.client_order_id
        self.order_list_id = init.order_list_id
        self.venue_order_id = None  # Can be None
        self.position_id = None  # Can be None
        self.account_id = None  # Can be None
        self.execution_id = None  # Can be None

        # Properties
        self.side = init.side
        self.type = init.type
        self.quantity = init.quantity
        self.time_in_force = init.time_in_force
        self.is_reduce_only = init.reduce_only
        self.parent_order_id = init.parent_order_id  # Can be None
        self.child_order_ids = init.child_order_ids  # Can be None
        self.contingency = init.contingency
        self.contingency_ids = init.contingency_ids  # Can be None
        self.tags = init.tags

        # Execution
        self.filled_qty = Quantity.zero_c(precision=0)
        self.leaves_qty = init.quantity
        self.avg_px = None  # Can be None
        self.slippage = Decimal(0)

        # Timestamps
        self.init_id = init.id
        self.ts_last = 0  # No fills yet
        self.ts_init = init.ts_init

    def __eq__(self, Order other) -> bool:
        return self.client_order_id.value == other.client_order_id.value

    def __hash__(self) -> int:
        return hash(self.client_order_id.value)

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"{self.info()}, "
                f"status={self._fsm.state_string_c()}, "
                f"client_order_id={self.client_order_id.value}, "
                f"venue_order_id={self.venue_order_id}, "
                f"tags={self.tags})")

    cpdef str info(self):
        """
        Return a summary description of the order.

        Returns
        -------
        str

        """
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef dict to_dict(self):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cdef OrderStatus status_c(self) except *:
        return <OrderStatus>self._fsm.state

    cdef OrderInitialized init_event_c(self):
        return self._events[0]  # Guaranteed to contain the initialized event

    cdef OrderEvent last_event_c(self):
        return self._events[-1]  # Guaranteed to contain the initialized event

    cdef list events_c(self):
        return self._events.copy()

    cdef list execution_ids_c(self):
        return self._execution_ids.copy()

    cdef int event_count_c(self) except *:
        return len(self._events)

    cdef str status_string_c(self):
        return self._fsm.state_string_c()

    cdef str type_string_c(self):
        return OrderTypeParser.to_str(self.type)

    cdef str side_string_c(self):
        return OrderSideParser.to_str(self.side)

    cdef str tif_string_c(self):
        return TimeInForceParser.to_str(self.time_in_force)

    cdef bint is_buy_c(self) except *:
        return self.side == OrderSide.BUY

    cdef bint is_sell_c(self) except *:
        return self.side == OrderSide.SELL

    cdef bint is_passive_c(self) except *:
        return self.type != OrderType.MARKET

    cdef bint is_aggressive_c(self) except *:
        return self.type == OrderType.MARKET

    cdef bint is_contingency_c(self) except *:
        return self.contingency != ContingencyType.NONE

    cdef bint is_parent_order_c(self) except *:
        return self.child_order_ids is not None

    cdef bint is_child_order_c(self) except *:
        return self.parent_order_id is not None

    cdef bint is_active_c(self) except *:
        return (
            self._fsm.state == OrderStatus.INITIALIZED
            or self._fsm.state == OrderStatus.SUBMITTED
            or self._fsm.state == OrderStatus.ACCEPTED
            or self._fsm.state == OrderStatus.TRIGGERED
            or self._fsm.state == OrderStatus.PENDING_CANCEL
            or self._fsm.state == OrderStatus.PENDING_UPDATE
            or self._fsm.state == OrderStatus.PARTIALLY_FILLED
        )

    cdef bint is_inflight_c(self) except *:
        return (
            self._fsm.state == OrderStatus.SUBMITTED
            or self._fsm.state == OrderStatus.PENDING_CANCEL
            or self._fsm.state == OrderStatus.PENDING_UPDATE
        )

    cdef bint is_working_c(self) except *:
        return (
            self._fsm.state == OrderStatus.ACCEPTED
            or self._fsm.state == OrderStatus.TRIGGERED
            or self._fsm.state == OrderStatus.PENDING_CANCEL
            or self._fsm.state == OrderStatus.PENDING_UPDATE
            or self._fsm.state == OrderStatus.PARTIALLY_FILLED
        )

    cdef bint is_pending_update_c(self) except *:
        return self._fsm.state == OrderStatus.PENDING_UPDATE

    cdef bint is_pending_cancel_c(self) except *:
        return self._fsm.state == OrderStatus.PENDING_CANCEL

    cdef bint is_completed_c(self) except *:
        return (
            self._fsm.state == OrderStatus.DENIED
            or self._fsm.state == OrderStatus.REJECTED
            or self._fsm.state == OrderStatus.CANCELED
            or self._fsm.state == OrderStatus.EXPIRED
            or self._fsm.state == OrderStatus.FILLED
        )

    @property
    def symbol(self):
        """
        The orders ticker symbol.

        Returns
        -------
        Symbol

        """
        return self.instrument_id.symbol

    @property
    def venue(self):
        """
        The orders trading venue.

        Returns
        -------
        Venue

        """
        return self.instrument_id.venue

    @property
    def status(self):
        """
        The orders current status.

        Returns
        -------
        OrderStatus

        """
        return self.status_c()

    @property
    def init_event(self):
        """
        The initialization event for the order.

        Returns
        -------
        OrderInitialized

        """
        return self.init_event_c()

    @property
    def last_event(self):
        """
        The last event applied to the order.

        Returns
        -------
        OrderEvent

        """
        return self.last_event_c()

    @property
    def events(self):
        """
        The order events.

        Returns
        -------
        list[OrderEvent]

        """
        return self.events_c()

    @property
    def execution_ids(self):
        """
        The execution IDs.

        Returns
        -------
        list[ExecutionId]

        """
        return self.execution_ids_c()

    @property
    def event_count(self):
        """
        The count of events applied to the order.

        Returns
        -------
        int

        """
        return self.event_count_c()

    @property
    def is_buy(self):
        """
        If the order side is ``BUY``.

        Returns
        -------
        bool

        """
        return self.is_buy_c()

    @property
    def is_sell(self):
        """
        If the order side is ``SELL``.

        Returns
        -------
        bool

        """
        return self.is_sell_c()

    @property
    def is_passive(self):
        """
        If the order is passive (`order.type` **not** ``MARKET``).

        Returns
        -------
        bool

        """
        return self.is_passive_c()

    @property
    def is_aggressive(self):
        """
        If the order is aggressive (`order.type` is ``MARKET``).

        Returns
        -------
        bool

        """
        return self.is_aggressive_c()

    @property
    def is_contingency(self):
        """
        If the order has a contingency (`order.contingency` is not ``NONE``).

        Returns
        -------
        bool

        """
        return self.is_contingency_c()

    @property
    def is_parent_order(self):
        """
        If the order has **at least** one child order.

        Returns
        -------
        bool

        """
        return self.is_parent_order_c()

    @property
    def is_child_order(self):
        """
        If the order has a parent order.

        Returns
        -------
        bool

        """
        return self.is_child_order_c()

    @property
    def is_active(self):
        """
        If the order is active (**not** completed).

        An order is considered active when its state can change.
        The possible states of active orders include;

        - ``INITIALIZED``
        - ``SUBMITTED``
        - ``ACCEPTED``
        - ``TRIGGERED``
        - ``PENDING_CANCEL``
        - ``PENDING_UPDATE``
        - ``PARTIALLY_FILLED``

        Returns
        -------
        bool

        """
        return self.is_active_c()

    @property
    def is_inflight(self):
        """
        If the order is in-flight (order request sent to the trading venue).

        An order is considered in-flight when its status is any of;

        - ``SUBMITTED``
        - ``PENDING_CANCEL``
        - ``PENDING_UPDATE``

        Returns
        -------
        bool

        """
        return self.is_inflight_c()

    @property
    def is_working(self):
        """
        If the order is working (open) at the trading venue.

        An order is considered working when its status is any of;

        - ``ACCEPTED``
        - ``TRIGGERED``
        - ``PENDING_CANCEL``
        - ``PENDING_UPDATE``
        - ``PARTIALLY_FILLED``

        Returns
        -------
        bool

        """
        return self.is_working_c()

    @property
    def is_pending_update(self):
        """
        If current order.status is ``PENDING_UPDATE``.

        Returns
        -------
        bool

        """
        return self.is_pending_update_c()

    @property
    def is_pending_cancel(self):
        """
        If current order.status is ``PENDING_CANCEL``.

        Returns
        -------
        bool

        """
        return self.is_pending_cancel_c()

    @property
    def is_completed(self):
        """
        If the order is completed (closed).

        An order is considered completed when its state can no longer change.
        The possible states of completed orders include;

        - ``INVALID``
        - ``DENIED``
        - ``REJECTED``
        - ``CANCELED``
        - ``EXPIRED``
        - ``FILLED``

        Returns
        -------
        bool

        """
        return self.is_completed_c()

    @staticmethod
    cdef OrderSide opposite_side_c(OrderSide side) except *:
        if side == OrderSide.BUY:
            return OrderSide.SELL
        elif side == OrderSide.SELL:
            return OrderSide.BUY
        else:  # pragma: no cover (design-time error)
            raise ValueError(f"invalid OrderSide, was {side}")

    @staticmethod
    cdef OrderSide flatten_side_c(PositionSide side) except *:
        if side == PositionSide.LONG:
            return OrderSide.SELL
        elif side == PositionSide.SHORT:
            return OrderSide.BUY
        else:  # pragma: no cover (design-time error)
            raise ValueError(f"invalid OrderSide, was {side}")

    @staticmethod
    def opposite_side(OrderSide side) -> OrderSide:
        """
        Return the opposite order side from the given side.

        Parameters
        ----------
        side : OrderSide
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
    def flatten_side(PositionSide side) -> OrderSide:
        """
        Return the order side needed to flatten a position from the given side.

        Parameters
        ----------
        side : PositionSide
            The position side to flatten.

        Returns
        -------
        OrderSide

        Raises
        ------
        ValueError
            If `side` is ``FLAT`` or invalid.

        """
        return Order.flatten_side_c(side)

    cpdef void apply(self, OrderEvent event) except *:
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
            If `event` is not a valid trigger from the current order status.
        KeyError
            If `event` is `OrderFilled` and `event.execution_id` already applied to the order.

        """
        Condition.not_none(event, "event")
        Condition.equal(event.client_order_id, self.client_order_id, "event.client_order_id", "self.client_order_id")
        if self.venue_order_id is not None and event.venue_order_id is not None and not isinstance(event, OrderUpdated):
            Condition.equal(self.venue_order_id, event.venue_order_id, "self.venue_order_id", "event.venue_order_id")

        # Handle event (FSM can raise InvalidStateTrigger)
        if isinstance(event, OrderDenied):
            self._fsm.trigger(OrderStatus.DENIED)
            self._denied(event)
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
            self._rollback_status = <OrderStatus>self._fsm.state
            self._fsm.trigger(OrderStatus.PENDING_UPDATE)
        elif isinstance(event, OrderPendingCancel):
            self._rollback_status = <OrderStatus>self._fsm.state
            self._fsm.trigger(OrderStatus.PENDING_CANCEL)
        elif isinstance(event, OrderModifyRejected):
            if self._fsm.state == OrderStatus.PENDING_UPDATE:
                self._fsm.trigger(self._rollback_status)
        elif isinstance(event, OrderCancelRejected):
            if self._fsm.state == OrderStatus.PENDING_CANCEL:
                self._fsm.trigger(self._rollback_status)
        elif isinstance(event, OrderUpdated):
            if self._fsm.state == OrderStatus.PENDING_UPDATE:
                self._fsm.trigger(self._rollback_status)
            self._updated(event)
        elif isinstance(event, OrderCanceled):
            self._fsm.trigger(OrderStatus.CANCELED)
            self._canceled(event)
        elif isinstance(event, OrderExpired):
            self._fsm.trigger(OrderStatus.EXPIRED)
            self._expired(event)
        elif isinstance(event, OrderTriggered):
            Condition.true(self.type == OrderType.STOP_LIMIT, "can only trigger a STOP_LIMIT order")
            self._fsm.trigger(OrderStatus.TRIGGERED)
            self._triggered(event)
        elif isinstance(event, OrderFilled):
            # Check identifiers
            if self.venue_order_id is None:
                self.venue_order_id = event.venue_order_id
            else:
                Condition.not_in(event.execution_id, self._execution_ids, "event.execution_id", "self._execution_ids")
            # Fill order
            if self.filled_qty + event.last_qty < self.quantity:
                self._fsm.trigger(OrderStatus.PARTIALLY_FILLED)
            else:
                self._fsm.trigger(OrderStatus.FILLED)
            self._filled(event)
        else:  # pragma: no cover (design-time error)
            raise ValueError(f"invalid OrderEvent, was {type(event)}")

        # Update events last as FSM may raise InvalidStateTrigger
        self._events.append(event)

    cdef void _denied(self, OrderDenied event) except *:
        pass  # Do nothing else

    cdef void _submitted(self, OrderSubmitted event) except *:
        self.account_id = event.account_id

    cdef void _rejected(self, OrderRejected event) except *:
        pass  # Do nothing else

    cdef void _accepted(self, OrderAccepted event) except *:
        self.venue_order_id = event.venue_order_id

    cdef void _updated(self, OrderUpdated event) except *:
        if self.venue_order_id != event.venue_order_id:
            self._venue_order_ids.append(self.venue_order_id)
            self.venue_order_id = event.venue_order_id
        if event.quantity is not None:
            self.quantity = event.quantity
            self.leaves_qty = Quantity(self.quantity - self.filled_qty, self.quantity.precision)

    cdef void _canceled(self, OrderCanceled event) except *:
        pass  # Do nothing else

    cdef void _expired(self, OrderExpired event) except *:
        pass  # Do nothing else

    cdef void _triggered(self, OrderTriggered event) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cdef void _filled(self, OrderFilled event) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cdef object _calculate_avg_px(self, Quantity last_qty, Price last_px):
        if self.avg_px is None:
            return last_px

        total_qty: Decimal = self.filled_qty + last_qty
        if total_qty > 0:  # Protect divide by zero
            return ((self.avg_px * self.filled_qty) + (last_px * last_qty)) / total_qty


cdef class PassiveOrder(Order):
    """
    The abstract base class for all passive orders.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        ClientOrderId client_order_id not None,
        OrderSide order_side,
        OrderType order_type,
        Quantity quantity not None,
        Price price not None,
        TimeInForce time_in_force,
        datetime expire_time,  # Can be None
        bint reduce_only,
        dict options not None,
        OrderListId order_list_id,  # Can be None
        ClientOrderId parent_order_id,  # Can be None
        list child_order_ids,  # Can be None
        ContingencyType contingency,
        list contingency_ids,  # Can be None
        str tags,  # Can be None
        UUID4 init_id not None,
        int64_t ts_init,
    ):
        Condition.positive(quantity, "quantity")
        if time_in_force == TimeInForce.GTD:
            # Must have an expire time
            Condition.not_none(expire_time, "expire_time")
        else:
            # Should not have an expire time
            Condition.none(expire_time, "expire_time")

        options["price"] = str(price)  # price checked not None
        if expire_time is not None:
            options["expire_time"] = maybe_dt_to_unix_nanos(expire_time)

        cdef OrderInitialized init = OrderInitialized(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            order_side=order_side,
            order_type=order_type,
            quantity=quantity,
            time_in_force=time_in_force,
            reduce_only=reduce_only,
            options=options,
            order_list_id=order_list_id,
            parent_order_id=parent_order_id,
            child_order_ids=child_order_ids,
            contingency=contingency,
            contingency_ids=contingency_ids,
            tags=tags,
            event_id=init_id,
            ts_init=ts_init,
        )

        super().__init__(init=init)

        self.price = price
        self.liquidity_side = LiquiditySide.NONE
        self.expire_time = expire_time
        self.expire_time_ns = int(pd.Timestamp(expire_time).to_datetime64()) if expire_time else 0
        self.slippage = Decimal(0)

    cpdef str info(self):
        """
        Return a summary description of the order.

        Returns
        -------
        str

        """
        cdef str expire_time = "" if self.expire_time is None else f" {format_iso8601(self.expire_time)}"
        return (f"{OrderSideParser.to_str(self.side)} {self.quantity.to_str()} {self.instrument_id} "
                f"{OrderTypeParser.to_str(self.type)} @ {self.price} "
                f"{TimeInForceParser.to_str(self.time_in_force)}{expire_time}")

    cpdef dict to_dict(self):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cdef list venue_order_ids_c(self):
        return self._venue_order_ids.copy()

    @property
    def venue_order_ids(self):
        """
        The venue order IDs.

        Returns
        -------
        list[VenueOrderId]

        """
        return self.venue_order_ids_c().copy()

    cdef void _updated(self, OrderUpdated event) except *:
        if self.venue_order_id != event.venue_order_id:
            self._venue_order_ids.append(self.venue_order_id)
            self.venue_order_id = event.venue_order_id
        if event.quantity is not None:
            self.quantity = event.quantity
            self.leaves_qty = Quantity(self.quantity - self.filled_qty, self.quantity.precision)
        if event.price is not None:
            self.price = event.price

    cdef void _triggered(self, OrderTriggered event) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cdef void _filled(self, OrderFilled fill) except *:
        self.venue_order_id = fill.venue_order_id
        self.position_id = fill.position_id
        self.strategy_id = fill.strategy_id
        self._execution_ids.append(fill.execution_id)
        self.execution_id = fill.execution_id
        self.liquidity_side = fill.liquidity_side
        filled_qty: Decimal = self.filled_qty.as_decimal() + fill.last_qty.as_decimal()
        leaves_qty: Decimal = self.quantity.as_decimal() - filled_qty
        if leaves_qty < 0:
            raise ValueError(
                f"invalid order.leaves_qty: was {leaves_qty}, "
                f"order.quantity={self.quantity}, "
                f"order.filled_qty={self.filled_qty}, "
                f"fill.last_qty={fill.last_qty}, "
                f"fill={fill}",
            )
        self.filled_qty = Quantity(filled_qty, fill.last_qty.precision)
        self.leaves_qty = Quantity(leaves_qty, fill.last_qty.precision)
        self.ts_last = fill.ts_event
        self.avg_px = self._calculate_avg_px(fill.last_qty, fill.last_px)
        self._set_slippage()

    cdef void _set_slippage(self) except *:
        if self.side == OrderSide.BUY:
            self.slippage = self.avg_px - self.price
        elif self.side == OrderSide.SELL:
            self.slippage = self.price - self.avg_px
