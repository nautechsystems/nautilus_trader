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

from nautilus_trader.core.constants cimport *  # str constants only
from nautilus_trader.core.correctness cimport Condition
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
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport ExecutionId
from nautilus_trader.model.identifiers cimport Symbol
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
    (OrderState.ACCEPTED, OrderState.PARTIALLY_FILLED): OrderState.PARTIALLY_FILLED,
    (OrderState.ACCEPTED, OrderState.FILLED): OrderState.FILLED,
    (OrderState.PARTIALLY_FILLED, OrderState.CANCELLED): OrderState.FILLED,
    (OrderState.PARTIALLY_FILLED, OrderState.PARTIALLY_FILLED): OrderState.PARTIALLY_FILLED,
    (OrderState.PARTIALLY_FILLED, OrderState.FILLED): OrderState.FILLED,
}


cdef class Order:
    """
    The abstract base class for all orders.

    This class should not be used directly, but through its concrete subclasses.
    """

    def __init__(self, OrderInitialized event not None):
        """
        Initialize a new instance of the `Order` class.

        Parameters
        ----------
        event : OrderInitialized
            The order initialized event.

        Raises
        ------
        ValueError
            If event.strategy_id is NULL
        """
        Condition.true(event.strategy_id.not_null(), "event.strategy_id.not_null()")

        self._events = [event]    # type: list[OrderEvent]
        self._execution_ids = []  # type: list[ExecutionId]
        self._fsm = FiniteStateMachine(
            state_transition_table=_ORDER_STATE_TABLE,
            initial_state=OrderState.INITIALIZED,
            trigger_parser=OrderStateParser.to_str,  # order_state_to_str correct here
            state_parser=OrderStateParser.to_str,
        )

        self.cl_ord_id = event.cl_ord_id
        self.id = OrderId.null_c()
        self.position_id = PositionId.null_c()
        self.strategy_id = event.strategy_id
        self.account_id = None        # Can be None
        self.execution_id = None      # Can be None
        self.symbol = event.symbol
        self.side = event.order_side
        self.type = event.order_type
        self.quantity = event.quantity
        self.timestamp = event.timestamp
        self.time_in_force = event.time_in_force
        self.filled_qty = Quantity()
        self.filled_timestamp = None  # Can be None
        self.avg_price = None         # Can be None
        self.slippage = Decimal()
        self.init_id = event.id

    def __eq__(self, Order other) -> bool:
        return self.cl_ord_id.value == other.cl_ord_id.value

    def __ne__(self, Order other) -> bool:
        return self.cl_ord_id.value != other.cl_ord_id.value

    def __hash__(self) -> int:
        return hash(self.cl_ord_id.value)

    def __repr__(self) -> str:
        cdef str id_string = f"id={self.id.value}, " if self.id.not_null() else ""
        return (f"{type(self).__name__}("
                f"cl_ord_id={self.cl_ord_id.value}, "
                f"{id_string}"
                f"state={self._fsm.state_string_c()}, "
                f"{self.status_string_c()})")

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
        return self._fsm.state == OrderState.ACCEPTED\
            or self._fsm.state == OrderState.PARTIALLY_FILLED

    cdef bint is_completed_c(self) except *:
        return self._fsm.state == OrderState.INVALID\
            or self._fsm.state == OrderState.DENIED\
            or self._fsm.state == OrderState.REJECTED\
            or self._fsm.state == OrderState.CANCELLED\
            or self._fsm.state == OrderState.EXPIRED\
            or self._fsm.state == OrderState.FILLED

    cdef bint is_active_c(self) except *:
        return not self.is_completed_c()

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
    def is_active(self):
        """
        If the order is active.

        An order is considered active if it has been initialized in the system,
        but has not yet reached a completed state. The possible states of active
        orders include; `INITIALIZED`, `SUBMITTED`, `ACCEPTED`,  and
        `PARTIALLY_FILLED`.

        Returns
        -------
        bool
            True if active, else False.

        """
        return self.is_active_c()

    @property
    def is_working(self):
        """
        If the order is working at the venue.

        An order is considered working when its state is either `ACCEPTED` or
        `PARTIALLY_FILLED`.

        Returns
        -------
        bool
            True if working, else False.

        """
        return self.is_working_c()

    @property
    def is_completed(self):
        """
        If the order is completed.

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
        Condition.not_equal(side, OrderSide.UNDEFINED, "side", "OrderSide.UNDEFINED")

        return OrderSide.BUY if side == OrderSide.SELL else OrderSide.SELL

    @staticmethod
    cdef inline OrderSide flatten_side_c(PositionSide side) except *:
        Condition.not_equal(side, PositionSide.UNDEFINED, "side", "PositionSide.UNDEFINED")
        Condition.not_equal(side, PositionSide.FLAT, "side", "PositionSide.FLAT")

        return OrderSide.BUY if side == PositionSide.SHORT else OrderSide.SELL

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
            If side is UNDEFINED or FLAT.

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
            If event.order_id not equal to self.id (if assigned and not being amended).
        InvalidStateTrigger
            If event is not a valid trigger from the current order.state.

        """
        self.apply_c(event)

    cdef void apply_c(self, OrderEvent event) except *:
        Condition.not_none(event, "event")
        Condition.equal(event.cl_ord_id, self.cl_ord_id, "event.cl_ord_id", "self.cl_ord_id")

        # Update events
        self._events.append(event)

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
            # TODO: Implement
            Condition.true(self._fsm.state == OrderState.ACCEPTED, "state == OrderState.ACCEPTED")
            self._amended(event)
        elif isinstance(event, OrderCancelled):
            # OrderId should have been assigned
            Condition.equal(self.id, event.order_id, "id", "event.order_id")
            self._fsm.trigger(OrderState.CANCELLED)
            self._cancelled(event)
        elif isinstance(event, OrderExpired):
            # OrderId should have been assigned
            Condition.equal(self.id, event.order_id, "id", "event.order_id")
            self._fsm.trigger(OrderState.EXPIRED)
            self._expired(event)
        elif isinstance(event, OrderFilled):
            if self.id.not_null():
                Condition.equal(self.id, event.order_id, "id", "event.order_id")
            else:
                self.id = event.order_id
            if self.quantity - self.filled_qty - event.fill_qty > 0:
                self._fsm.trigger(OrderState.PARTIALLY_FILLED)
            else:
                self._fsm.trigger(OrderState.FILLED)
            self._filled(event)

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

    cdef void _filled(self, OrderFilled event) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplemented("method must be implemented in subclass")

    cdef object _calculate_avg_price(self, Price fill_price, Quantity fill_quantity):
        if self.avg_price is None:
            return fill_price

        total_quantity: Decimal = self.filled_qty + fill_quantity
        return ((self.avg_price * self.filled_qty) + (fill_price * fill_quantity)) / total_quantity


cdef class PassiveOrder(Order):
    """
    The abstract base class for all passive orders.

    This class should not be used directly, but through its concrete subclasses.
    """
    def __init__(
        self,
        ClientOrderId cl_ord_id not None,
        StrategyId strategy_id not None,
        Symbol symbol not None,
        OrderSide order_side,
        OrderType order_type,  # 'type' hides keyword
        Quantity quantity not None,
        Price price not None,
        TimeInForce time_in_force,
        datetime expire_time,  # Can be None
        UUID init_id not None,
        datetime timestamp not None,
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
        symbol : Symbol
            The order symbol.
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
            The order expiry time - for GTD orders only.
        init_id : UUID
            The order initialization event identifier.
        timestamp : datetime
            The order initialization timestamp.
        options : dict
            The order options.

        Raises
        ------
        ValueError
            If quantity is not positive (> 0).
        ValueError
            If order_side is UNDEFINED.
        ValueError
            If order_type is UNDEFINED.
        ValueError
            If time_in_force is UNDEFINED.
        ValueError
            If time_in_force is GTD and the expire_time is None.

        """
        # Condition for order_side not UNDEFINED checked in OrderInitialized
        # Condition for order_type not UNDEFINED checked in OrderInitialized
        # Condition for time_in_force not UNDEFINED checked in OrderInitialized
        Condition.positive(quantity, "quantity")
        Condition.not_equal(time_in_force, TimeInForce.UNDEFINED, "time_in_force", "UNDEFINED")
        if time_in_force == TimeInForce.GTD:
            # Must have an expire time
            Condition.not_none(expire_time, "expire_time")
        else:
            # Should not have an expire time
            Condition.none(expire_time, "expire_time")

        options[PRICE] = str(price)  # price checked not None
        if expire_time is not None:
            options[EXPIRE_TIME] = expire_time

        cdef OrderInitialized init_event = OrderInitialized(
            cl_ord_id=cl_ord_id,
            strategy_id=strategy_id,
            symbol=symbol,
            order_side=order_side,
            order_type=order_type,
            quantity=quantity,
            time_in_force=time_in_force,
            event_id=init_id,
            event_timestamp=timestamp,
            options=options,
        )

        super().__init__(init_event)

        self.price = price
        self.liquidity_side = LiquiditySide.NONE
        self.expire_time = expire_time
        self.slippage = Decimal()

    cdef str status_string_c(self):
        cdef str expire_time = "" if self.expire_time is None else f" {format_iso8601(self.expire_time)}"
        return (f"{OrderSideParser.to_str(self.side)} {self.quantity.to_str()} {self.symbol} "
                f"{OrderTypeParser.to_str(self.type)} @ {self.price} "
                f"{TimeInForceParser.to_str(self.time_in_force)}{expire_time}")

    cdef void _amended(self, OrderAmended event) except *:
        self.id = event.order_id
        self.quantity = event.quantity
        self.price = event.price

    cdef void _filled(self, OrderFilled event) except *:
        self.id = event.order_id
        self.position_id = event.position_id
        self.strategy_id = event.strategy_id
        self._execution_ids.append(event.execution_id)
        self.execution_id = event.execution_id
        self.liquidity_side = event.liquidity_side
        self.filled_qty = Quantity(self.filled_qty + event.fill_qty)
        self.filled_timestamp = event.timestamp
        self.avg_price = self._calculate_avg_price(event.fill_price, event.fill_qty)
        self._set_slippage()

    cdef void _set_slippage(self) except *:
        if self.side == OrderSide.BUY:
            self.slippage = self.avg_price - self.price
        else:  # self.side == OrderSide.SELL:
            self.slippage = self.price - self.avg_price


cdef set _MARKET_ORDER_VALID_TIF = {
    TimeInForce.GTC,
    TimeInForce.IOC,
    TimeInForce.FOK,
}


cdef class MarketOrder(Order):
    """
    A market order is an order to buy or sell an instrument immediately. This
    type of order guarantees that the order will be executed, but does not
    guarantee the execution price. A market order generally will execute at or
    near the current bid (for a sell order) or ask (for a buy order) price. The
    last-traded price is not necessarily the price at which a market order will
    be executed.
    """
    def __init__(
        self,
        ClientOrderId cl_ord_id not None,
        StrategyId strategy_id not None,
        Symbol symbol not None,
        OrderSide order_side,
        Quantity quantity not None,
        TimeInForce time_in_force,
        UUID init_id not None,
        datetime timestamp not None,
    ):
        """
        Initialize a new instance of the `MarketOrder` class.

        Parameters
        ----------
        cl_ord_id : ClientOrderId
            The client order identifier.
        strategy_id : StrategyId
            The strategy identifier associated with the order.
        symbol : Symbol
            The order symbol.
        order_side : OrderSide (Enum)
            The order side (BUY or SELL).
        quantity : Quantity
            The order quantity (> 0).
        init_id : UUID
            The order initialization event identifier.
        timestamp : datetime
            The order initialization timestamp.

        Raises
        ------
        ValueError
            If quantity is not positive (> 0).
        ValueError
            If order_side is UNDEFINED.
        ValueError
            If time_in_force is UNDEFINED.
        ValueError
            If time_in_force is other than GTC, IOC or FOK.

        """
        Condition.positive(quantity, "quantity")
        Condition.true(time_in_force in _MARKET_ORDER_VALID_TIF, "time_in_force is GTC, IOC or FOK")

        cdef OrderInitialized init_event = OrderInitialized(
            cl_ord_id=cl_ord_id,
            strategy_id=strategy_id,
            symbol=symbol,
            order_side=order_side,
            order_type=OrderType.MARKET,
            quantity=quantity,
            time_in_force=time_in_force,
            event_id=init_id,
            event_timestamp=timestamp,
            options={},
        )

        super().__init__(init_event)

    @staticmethod
    cdef MarketOrder create(OrderInitialized event):
        """
        Return an order from the given initialized event.

        Parameters
        ----------
        event : OrderInitialized
            The event to initialize with.

        Returns
        -------
        MarketOrder

        Raises
        ------
        ValueError
            If event.order_type is not equal to OrderType.MARKET.

        """
        Condition.not_none(event, "event")
        Condition.equal(event.order_type, OrderType.MARKET, "event.order_type", "OrderType")

        return MarketOrder(
            cl_ord_id=event.cl_ord_id,
            strategy_id=event.strategy_id,
            symbol=event.symbol,
            order_side=event.order_side,
            quantity=event.quantity,
            time_in_force=event.time_in_force,
            init_id=event.id,
            timestamp=event.timestamp,
        )

    cdef str status_string_c(self):
        return (f"{OrderSideParser.to_str(self.side)} {self.quantity.to_str()} {self.symbol} "
                f"{OrderTypeParser.to_str(self.type)} "
                f"{TimeInForceParser.to_str(self.time_in_force)}")

    cdef void _amended(self, OrderAmended event) except *:
        raise NotImplemented("Cannot amend a market order")

    cdef void _filled(self, OrderFilled event) except *:
        self.id = event.order_id
        self.position_id = event.position_id
        self.strategy_id = event.strategy_id
        self._execution_ids.append(event.execution_id)
        self.execution_id = event.execution_id
        self.filled_qty = Quantity(self.filled_qty + event.fill_qty)
        self.filled_timestamp = event.timestamp
        self.avg_price = self._calculate_avg_price(event.fill_price, event.fill_qty)


cdef class LimitOrder(PassiveOrder):
    """
    Limit orders are used to specify a maximum or minimum price the trader is
    willing to buy or sell at. Traders use this order type to minimise their
    trading cost, however they are sacrificing guaranteed execution as there is
    a chance the order may not be executed if it is placed deep out of the
    market.
    """
    def __init__(
        self,
        ClientOrderId cl_ord_id not None,
        StrategyId strategy_id not None,
        Symbol symbol not None,
        OrderSide order_side,
        Quantity quantity not None,
        Price price not None,
        TimeInForce time_in_force,
        datetime expire_time,  # Can be None
        UUID init_id not None,
        datetime timestamp not None,
        bint post_only=True,
        bint reduce_only=False,
        bint hidden=False,
    ):
        """
        Initialize a new instance of the `LimitOrder` class.

        Parameters
        ----------
        cl_ord_id : ClientOrderId
            The client order identifier.
        strategy_id : StrategyId
            The strategy identifier associated with the order.
        symbol : Symbol
            The order symbol.
        order_side : OrderSide (Enum)
            The order side (BUY or SELL).
        quantity : Quantity
            The order quantity (> 0).
        price : Price
            The order limit price.
        time_in_force : TimeInForce (Enum)
            The order time-in-force.
        expire_time : datetime, optional
            The order expiry time.
        init_id : UUID
            The order initialization event identifier.
        timestamp : datetime
            The order initialization timestamp.
        post_only : bool, optional
            If the order will only make a market.
        reduce_only : bool, optional
            If the order will only reduce an open position.
        hidden : bool, optional
            If the order will be hidden from the public book.

        Raises
        ------
        ValueError
            If quantity is not positive (> 0).
        ValueError
            If order_side is UNDEFINED.
        ValueError
            If time_in_force is UNDEFINED.
        ValueError
            If time_in_force is GTD and expire_time is None.
        ValueError
            If post_only and hidden.
        ValueError
            If hidden and post_only.

        """
        if post_only:
            Condition.true(not hidden, "A post-only order is not hidden")
        if hidden:
            Condition.true(not post_only, "A hidden order is not post-only")

        cdef dict options = {
            POST_ONLY: post_only,
            REDUCE_ONLY: reduce_only,
            HIDDEN: hidden,
        }

        super().__init__(
            cl_ord_id,
            strategy_id,
            symbol,
            order_side,
            OrderType.LIMIT,
            quantity,
            price,
            time_in_force,
            expire_time,
            init_id,
            timestamp,
            options,
        )

        self.is_post_only = post_only
        self.is_reduce_only = reduce_only
        self.is_hidden = hidden

    @staticmethod
    cdef LimitOrder create(OrderInitialized event):
        """
        Return a limit order from the given initialized event.

        Parameters
        ----------
        event : OrderInitialized
            The event to initialize with.

        Returns
        -------
        LimitOrder

        Raises
        ------
        ValueError
            If event.order_type is not equal to OrderType.LIMIT.

        """
        Condition.not_none(event, "event")
        Condition.equal(event.order_type, OrderType.LIMIT, "event.order_type", "OrderType")

        return LimitOrder(
            cl_ord_id=event.cl_ord_id,
            strategy_id=event.strategy_id,
            symbol=event.symbol,
            order_side=event.order_side,
            quantity=event.quantity,
            price=Price(event.options.get(PRICE)),
            time_in_force=event.time_in_force,
            expire_time=event.options.get(EXPIRE_TIME),
            init_id=event.id,
            timestamp=event.timestamp,
            post_only=event.options[POST_ONLY],
            reduce_only=event.options[REDUCE_ONLY],
            hidden=event.options[HIDDEN],
        )


cdef class StopMarketOrder(PassiveOrder):
    """
    Represents a stop market order. Once the price crosses the predefined
    trigger price, the stop order becomes a market order.
    """
    def __init__(
        self,
        ClientOrderId cl_ord_id not None,
        StrategyId strategy_id not None,
        Symbol symbol not None,
        OrderSide order_side,
        Quantity quantity not None,
        Price price not None,
        TimeInForce time_in_force,
        datetime expire_time,  # Can be None
        UUID init_id not None,
        datetime timestamp not None,
        bint reduce_only=False,
    ):
        """
        Initialize a new instance of the `StopMarketOrder` class.

        Parameters
        ----------
        cl_ord_id : ClientOrderId
            The client order identifier.
        strategy_id : StrategyId
            The strategy identifier associated with the order.
        symbol : Symbol
            The order symbol.
        order_side : OrderSide (Enum)
            The order side (BUY or SELL).
        quantity : Quantity
            The order quantity (> 0).
        price : Price
            The order stop price.
        time_in_force : TimeInForce (Enum)
            The order time-in-force.
        expire_time : datetime, optional
            The order expiry time.
        init_id : UUID
            The order initialization event identifier.
        timestamp : datetime
            The order initialization timestamp.
        reduce_only : bool, optional
            If the order will only reduce an open position.

        Raises
        ------
        ValueError
            If quantity is not positive (> 0).
        ValueError
            If order_side is UNDEFINED.
        ValueError
            If time_in_force is UNDEFINED.
        ValueError
            If time_in_force is GTD and the expire_time is None.

        """
        cdef dict options = {
            REDUCE_ONLY: reduce_only,
        }

        super().__init__(
            cl_ord_id,
            strategy_id,
            symbol,
            order_side,
            OrderType.STOP_MARKET,
            quantity,
            price,
            time_in_force,
            expire_time,
            init_id,
            timestamp,
            options=options,
        )

        self.is_reduce_only = reduce_only

    @staticmethod
    cdef StopMarketOrder create(OrderInitialized event):
        """
        Return a stop order from the given initialized event.

        Parameters
        ----------
        event : OrderInitialized
            The event to initialize with.

        Returns
        -------
        StopMarketOrder

        Raises
        ------
        ValueError
            If event.order_type is not equal to OrderType.STOP_MARKET.

        """
        Condition.not_none(event, "event")
        Condition.equal(event.order_type, OrderType.STOP_MARKET, "event.order_type", "OrderType")

        return StopMarketOrder(
            cl_ord_id=event.cl_ord_id,
            strategy_id=event.strategy_id,
            symbol=event.symbol,
            order_side=event.order_side,
            quantity=event.quantity,
            price=Price(event.options.get(PRICE)),
            time_in_force=event.time_in_force,
            expire_time=event.options.get(EXPIRE_TIME),
            init_id=event.id,
            timestamp=event.timestamp,
            reduce_only=event.options[REDUCE_ONLY],
        )


cdef class BracketOrder:
    """
    Represents a bracket order.

    A bracket order is designed to help limit a traders loss and optionally
    lock in a profit by "bracketing" an entry order with two opposite-side exit
    orders. A BUY order is bracketed by a high-side sell order and a
    low-side sell stop order. A SELL order is bracketed by a high-side buy stop
    order and a low-side buy order.

    Once the 'parent' entry order is triggered the 'child' OCO orders being a
    `StopMarket` and optional take-profit `PassiveOrder` automatically become
    working on the exchange/broker side.
    """
    def __init__(
        self,
        Order entry not None,
        StopMarketOrder stop_loss not None,
        PassiveOrder take_profit=None,
    ):
        """
        Initialize a new instance of the `BracketOrder` class.

        Parameters
        ----------
        entry : Order
            The entry 'parent' order.
        stop_loss : StopMarketOrder
            The stop-loss (SL) 'child' order.
        take_profit : PassiveOrder, optional
            The take-profit (TP) 'child' order. Normally a `LimitOrder`.

        """
        self.id = BracketOrderId(f"B{entry.cl_ord_id.value}")
        self.entry = entry
        self.stop_loss = stop_loss
        self.take_profit = take_profit
        self.timestamp = entry.timestamp

    def __eq__(self, BracketOrder other) -> bool:
        return self.id.value == other.id.value

    def __ne__(self, BracketOrder other) -> bool:
        return self.id.value != other.id.value

    def __repr__(self) -> str:
        cdef str take_profit_price = "NONE" if self.take_profit is None else str(self.take_profit.price)
        return f"BracketOrder(id={self.id.value}, Entry{self.entry}, SL={self.stop_loss.price}, TP={take_profit_price})"
