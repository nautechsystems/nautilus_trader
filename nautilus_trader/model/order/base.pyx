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

from cpython.datetime cimport datetime
from libc.stdint cimport int64_t

from nautilus_trader.core.constants cimport *  # str constants only
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport dt_to_unix_nanos
from nautilus_trader.core.datetime cimport format_iso8601
from nautilus_trader.core.uuid cimport UUID
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.order_side cimport OrderSideParser
from nautilus_trader.model.c_enums.order_state cimport OrderState
from nautilus_trader.model.c_enums.order_state cimport OrderStateParser
from nautilus_trader.model.c_enums.order_type cimport OrderType
from nautilus_trader.model.c_enums.order_type cimport OrderTypeParser
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForce
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForceParser
from nautilus_trader.model.events cimport OrderAccepted
from nautilus_trader.model.events cimport OrderAmended
from nautilus_trader.model.events cimport OrderCancelled
from nautilus_trader.model.events cimport OrderDenied
from nautilus_trader.model.events cimport OrderEvent
from nautilus_trader.model.events cimport OrderExpired
from nautilus_trader.model.events cimport OrderFilled
from nautilus_trader.model.events cimport OrderInitialized
from nautilus_trader.model.events cimport OrderInvalid
from nautilus_trader.model.events cimport OrderRejected
from nautilus_trader.model.events cimport OrderSubmitted
from nautilus_trader.model.events cimport OrderTriggered
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport ExecutionId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


# State being used as trigger
cdef dict _ORDER_STATE_TABLE = {
    (OrderState.INITIALIZED, OrderState.INVALID): OrderState.INVALID,
    (OrderState.INITIALIZED, OrderState.DENIED): OrderState.DENIED,
    (OrderState.INITIALIZED, OrderState.SUBMITTED): OrderState.SUBMITTED,
    (OrderState.SUBMITTED, OrderState.REJECTED): OrderState.REJECTED,
    (OrderState.SUBMITTED, OrderState.CANCELLED): OrderState.CANCELLED,
    (OrderState.SUBMITTED, OrderState.ACCEPTED): OrderState.ACCEPTED,
    (OrderState.SUBMITTED, OrderState.PARTIALLY_FILLED): OrderState.PARTIALLY_FILLED,
    (OrderState.SUBMITTED, OrderState.FILLED): OrderState.FILLED,
    (OrderState.ACCEPTED, OrderState.CANCELLED): OrderState.CANCELLED,
    (OrderState.ACCEPTED, OrderState.EXPIRED): OrderState.EXPIRED,
    (OrderState.ACCEPTED, OrderState.TRIGGERED): OrderState.TRIGGERED,
    (OrderState.ACCEPTED, OrderState.PARTIALLY_FILLED): OrderState.PARTIALLY_FILLED,
    (OrderState.ACCEPTED, OrderState.FILLED): OrderState.FILLED,
    (OrderState.TRIGGERED, OrderState.REJECTED): OrderState.REJECTED,
    (OrderState.TRIGGERED, OrderState.CANCELLED): OrderState.CANCELLED,
    (OrderState.TRIGGERED, OrderState.EXPIRED): OrderState.EXPIRED,
    (OrderState.TRIGGERED, OrderState.PARTIALLY_FILLED): OrderState.PARTIALLY_FILLED,
    (OrderState.TRIGGERED, OrderState.FILLED): OrderState.FILLED,
    (OrderState.PARTIALLY_FILLED, OrderState.CANCELLED): OrderState.FILLED,
    (OrderState.PARTIALLY_FILLED, OrderState.PARTIALLY_FILLED): OrderState.PARTIALLY_FILLED,
    (OrderState.PARTIALLY_FILLED, OrderState.FILLED): OrderState.FILLED,
}

# Valid states to amend an order in
cdef tuple _AMENDING_STATES = (OrderState.ACCEPTED, OrderState.TRIGGERED)


cdef class Order:
    """
    The abstract base class for all orders.

    This class should not be used directly, but through its concrete subclasses.
    """

    def __init__(self, OrderInitialized init not None):
        """
        Initialize a new instance of the `Order` class.

        Parameters
        ----------
        init : OrderInitialized
            The order initialized event.

        Raises
        ------
        ValueError
            If event.strategy_id has a 'NULL' value.

        """
        Condition.true(init.strategy_id.not_null(), f"init.strategy_id.value was 'NULL'")

        self._events = [init]    # type: list[OrderEvent]
        self._execution_ids = []  # type: list[ExecutionId]
        self._fsm = FiniteStateMachine(
            state_transition_table=_ORDER_STATE_TABLE,
            initial_state=OrderState.INITIALIZED,
            trigger_parser=OrderStateParser.to_str,  # order_state_to_str correct here
            state_parser=OrderStateParser.to_str,
        )

        self.cl_ord_id = init.cl_ord_id
        self.id = OrderId.null_c()
        self.position_id = PositionId.null_c()
        self.strategy_id = init.strategy_id
        self.account_id = None    # Can be None
        self.execution_id = None  # Can be None
        self.instrument_id = init.instrument_id
        self.symbol = init.instrument_id.symbol
        self.venue = init.instrument_id.venue
        self.side = init.order_side
        self.type = init.order_type
        self.quantity = init.quantity
        self.timestamp_ns = init.timestamp_ns
        self.time_in_force = init.time_in_force
        self.filled_qty = Quantity()
        self.execution_ns = 0
        self.avg_px = None  # Can be None
        self.slippage = Decimal()
        self.init_id = init.id

    def __eq__(self, Order other) -> bool:
        return self.cl_ord_id.value == other.cl_ord_id.value

    def __ne__(self, Order other) -> bool:
        return self.cl_ord_id.value != other.cl_ord_id.value

    def __hash__(self) -> int:
        return hash(self.cl_ord_id.value)

    def __repr__(self) -> str:
        cdef str id_string = f", id={self.id.value})" if self.id.not_null() else ")"
        return (f"{type(self).__name__}("
                f"{self.status_string_c()}, "
                f"state={self._fsm.state_string_c()}, "
                f"cl_ord_id={self.cl_ord_id.value}"
                f"{id_string}")

    cdef OrderState state_c(self) except *:
        return <OrderState>self._fsm.state

    cdef OrderInitialized init_event_c(self):
        return self._events[0]  # Guaranteed to have the initialized event

    cdef OrderEvent last_event_c(self):
        return self._events[-1]  # Guaranteed to have the initialized event

    cdef list events_c(self):
        return self._events.copy()

    cdef list execution_ids_c(self):
        return self._execution_ids.copy()

    cdef int event_count_c(self) except *:
        return len(self._events)

    cdef str state_string_c(self):
        return self._fsm.state_string_c()

    cdef str status_string_c(self):
        raise NotImplemented("method must be implemented in subclass")

    cdef bint is_buy_c(self) except *:
        return self.side == OrderSide.BUY

    cdef bint is_sell_c(self) except *:
        return self.side == OrderSide.SELL

    cdef bint is_passive_c(self) except *:
        return self.type != OrderType.MARKET

    cdef bint is_aggressive_c(self) except *:
        return self.type == OrderType.MARKET

    cdef bint is_working_c(self) except *:
        return self._fsm.state == OrderState.ACCEPTED \
            or self._fsm.state == OrderState.PARTIALLY_FILLED \
            or self._fsm.state == OrderState.TRIGGERED

    cdef bint is_completed_c(self) except *:
        return self._fsm.state == OrderState.INVALID \
            or self._fsm.state == OrderState.DENIED \
            or self._fsm.state == OrderState.REJECTED \
            or self._fsm.state == OrderState.CANCELLED \
            or self._fsm.state == OrderState.EXPIRED \
            or self._fsm.state == OrderState.FILLED

    @property
    def state(self):
        """
        The orders current state.

        Returns
        -------
        OrderState (Enum)

        """
        return self.state_c()

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
        The execution identifiers.

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
        If the order side is `BUY`.

        Returns
        -------
        bool
            True if BUY, else False.

        """
        return self.is_buy_c()

    @property
    def is_sell(self):
        """
        If the order side is `SELL`.

        Returns
        -------
        bool
            True if SELL, else False.

        """
        return self.is_sell_c()

    @property
    def is_passive(self):
        """
        If the order is passive.

        Returns
        -------
        bool
            True if order type not MARKET, else False.

        """
        return self.is_passive_c()

    @property
    def is_aggressive(self):
        """
        If the order is aggressive.

        Returns
        -------
        bool
            True if order type MARKET, else False.

        """
        return self.is_aggressive_c()

    @property
    def is_working(self):
        """
        If the order is open/working at the venue.

        An order is considered working when its state is either `ACCEPTED`,
        `TRIGGERED` or `PARTIALLY_FILLED`.

        Returns
        -------
        bool
            True if working, else False.

        """
        return self.is_working_c()

    @property
    def is_completed(self):
        """
        If the order is closed/completed.

        An order is considered completed when its state can no longer change.
        The possible states of completed orders include; `INVALID`, `DENIED`,
        `REJECTED`, `CANCELLED`, `EXPIRED` and `FILLED`.

        Returns
        -------
        bool
            True if completed, else False.

        """
        return self.is_completed_c()

    @staticmethod
    cdef OrderSide opposite_side_c(OrderSide side) except *:
        if side == OrderSide.BUY:
            return OrderSide.SELL
        elif side == OrderSide.SELL:
            return OrderSide.BUY
        else:
            raise ValueError(f"side was invalid, was {side}")

    @staticmethod
    cdef inline OrderSide flatten_side_c(PositionSide side) except *:
        if side == PositionSide.LONG:
            return OrderSide.SELL
        elif side == PositionSide.SHORT:
            return OrderSide.BUY
        else:
            raise ValueError(f"side was invalid, was {side}")

    @staticmethod
    def opposite_side(OrderSide side) -> OrderSide:
        """
        Return the opposite order side from the given side.

        Parameters
        ----------
        side : OrderSide (Enum)
            The original order side.

        Returns
        -------
        OrderSide

        Raises
        ------
        ValueError
            If side is invalid.

        """
        return Order.opposite_side_c(side)

    @staticmethod
    def flatten_side(PositionSide side) -> OrderSide:
        """
        Return the order side needed to flatten a position from the given side.

        Parameters
        ----------
        side : PositionSide (Enum)
            The position side to flatten.

        Returns
        -------
        OrderSide

        Raises
        ------
        ValueError
            If side is FLAT or invalid.

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
            If self.id and event.order_id are both not 'NULL', and are not equal.
        InvalidStateTrigger
            If event is not a valid trigger from the current order.state.
        KeyError
            If event is OrderFilled and event.execution_id already applied to the order.

        """
        Condition.not_none(event, "event")
        Condition.equal(event.cl_ord_id, self.cl_ord_id, "event.cl_ord_id", "self.cl_ord_id")
        if self.id.not_null() and event.order_id.not_null():
            Condition.equal(event.order_id, self.id, "event.order_id", "self.id")

        # Handle event (FSM can raise InvalidStateTrigger)
        if isinstance(event, OrderInvalid):
            self._fsm.trigger(OrderState.INVALID)
            self._invalid(event)
        elif isinstance(event, OrderDenied):
            self._fsm.trigger(OrderState.DENIED)
            self._denied(event)
        elif isinstance(event, OrderSubmitted):
            self._fsm.trigger(OrderState.SUBMITTED)
            self._submitted(event)
        elif isinstance(event, OrderRejected):
            self._fsm.trigger(OrderState.REJECTED)
            self._rejected(event)
        elif isinstance(event, OrderAccepted):
            self._fsm.trigger(OrderState.ACCEPTED)
            self._accepted(event)
        elif isinstance(event, OrderAmended):
            Condition.true(self._fsm.state in _AMENDING_STATES, "state was invalid for amending")
            self._amended(event)
        elif isinstance(event, OrderCancelled):
            self._fsm.trigger(OrderState.CANCELLED)
            self._cancelled(event)
        elif isinstance(event, OrderExpired):
            self._fsm.trigger(OrderState.EXPIRED)
            self._expired(event)
        elif isinstance(event, OrderTriggered):
            Condition.true(self.type == OrderType.STOP_LIMIT, "can only trigger a STOP_LIMIT order")
            self._fsm.trigger(OrderState.TRIGGERED)
            self._triggered(event)
        elif isinstance(event, OrderFilled):
            if self.id.not_null():
                Condition.not_in(event.execution_id, self._execution_ids, "event.execution_id", "self._execution_ids")
            else:
                self.id = event.order_id
            if self.filled_qty + event.last_qty < self.quantity:
                self._fsm.trigger(OrderState.PARTIALLY_FILLED)
            else:
                self._fsm.trigger(OrderState.FILLED)
            self._filled(event)

        # Update events last as FSM may raise InvalidStateTrigger
        self._events.append(event)

    cdef void _invalid(self, OrderInvalid event) except *:
        pass  # Do nothing else

    cdef void _denied(self, OrderDenied event) except *:
        pass  # Do nothing else

    cdef void _submitted(self, OrderSubmitted event) except *:
        self.account_id = event.account_id

    cdef void _rejected(self, OrderRejected event) except *:
        pass  # Do nothing else

    cdef void _accepted(self, OrderAccepted event) except *:
        self.id = event.order_id

    cdef void _amended(self, OrderAmended event) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplemented("method must be implemented in subclass")

    cdef void _cancelled(self, OrderCancelled event) except *:
        pass  # Do nothing else

    cdef void _expired(self, OrderExpired event) except *:
        pass  # Do nothing else

    cdef void _triggered(self, OrderTriggered event) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplemented("method must be implemented in subclass")

    cdef void _filled(self, OrderFilled event) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplemented("method must be implemented in subclass")

    cdef object _calculate_avg_px(self, Quantity last_qty, Price last_px):
        if self.avg_px is None:
            return last_px

        total_quantity: Decimal = self.filled_qty + last_qty
        return ((self.avg_px * self.filled_qty) + (last_px * last_qty)) / total_quantity


cdef class PassiveOrder(Order):
    """
    The abstract base class for all passive orders.

    This class should not be used directly, but through its concrete subclasses.
    """
    def __init__(
        self,
        ClientOrderId cl_ord_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        OrderSide order_side,
        OrderType order_type,
        Quantity quantity not None,
        Price price not None,
        TimeInForce time_in_force,
        datetime expire_time,  # Can be None
        UUID init_id not None,
        int64_t timestamp_ns,
        dict options not None,
    ):
        """
        Initialize a new instance of the `PassiveOrder` class.

        Parameters
        ----------
        cl_ord_id : ClientOrderId
            The client order identifier.
        strategy_id : StrategyId
            The strategy identifier associated with the order.
        instrument_id : InstrumentId
            The order instrument identifier.
        order_side : OrderSide (Enum)
            The order side (BUY or SELL).
        order_type : OrderType (Enum)
            The order type.
        quantity : Quantity
            The order quantity (> 0).
        price : Price
            The order price.
        time_in_force : TimeInForce (Enum)
            The order time-in-force.
        expire_time : datetime, optional
            The order expiry time - applicable to GTD orders only.
        init_id : UUID
            The order initialization event identifier.
        timestamp_ns : int64
            The order initialization timestamp.
        options : dict
            The order options.

        Raises
        ------
        ValueError
            If quantity is not positive (> 0).
        ValueError
            If time_in_force is GTD and the expire_time is None.

        """
        Condition.positive(quantity, "quantity")
        if time_in_force == TimeInForce.GTD:
            # Must have an expire time
            Condition.not_none(expire_time, "expire_time")
        else:
            # Should not have an expire time
            Condition.none(expire_time, "expire_time")

        options[PRICE] = str(price)  # price checked not None
        if expire_time is not None:
            options[EXPIRE_TIME] = expire_time

        cdef OrderInitialized init = OrderInitialized(
            cl_ord_id=cl_ord_id,
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            order_side=order_side,
            order_type=order_type,
            quantity=quantity,
            time_in_force=time_in_force,
            event_id=init_id,
            timestamp_ns=timestamp_ns,
            options=options,
        )

        super().__init__(init=init)

        self.price = price
        self.liquidity_side = LiquiditySide.NONE
        self.expire_time = expire_time
        self.expire_time_ns = dt_to_unix_nanos(dt=expire_time) if expire_time else 0
        self.slippage = Decimal()

    cdef str status_string_c(self):
        cdef str expire_time = "" if self.expire_time is None else f" {format_iso8601(self.expire_time)}"
        return (f"{OrderSideParser.to_str(self.side)} {self.quantity.to_str()} {self.instrument_id} "
                f"{OrderTypeParser.to_str(self.type)} @ {self.price} "
                f"{TimeInForceParser.to_str(self.time_in_force)}{expire_time}")

    cdef void _amended(self, OrderAmended event) except *:
        self.id = event.order_id
        self.quantity = event.quantity
        self.price = event.price

    cdef void _filled(self, OrderFilled fill) except *:
        self.id = fill.order_id
        self.position_id = fill.position_id
        self.strategy_id = fill.strategy_id
        self._execution_ids.append(fill.execution_id)
        self.execution_id = fill.execution_id
        self.liquidity_side = fill.liquidity_side
        self.filled_qty = Quantity(self.filled_qty + fill.last_qty)
        self.execution_ns = fill.execution_ns
        self.avg_px = self._calculate_avg_px(fill.last_qty, fill.last_px)
        self._set_slippage()

    cdef void _set_slippage(self) except *:
        if self.side == OrderSide.BUY:
            self.slippage = self.avg_px - self.price
        else:  # self.side == OrderSide.SELL:
            self.slippage = self.price - self.avg_px
