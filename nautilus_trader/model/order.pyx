# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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
Defines the main Order object, AtomicOrder representing parent and child orders
and an OrderFactory for more convenient creation of order objects.
"""

from cpython.datetime cimport datetime

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.decimal cimport Decimal
from nautilus_trader.core.types cimport Label
from nautilus_trader.core.uuid cimport UUID
from nautilus_trader.core.datetime cimport format_iso8601
from nautilus_trader.model.c_enums.order_side cimport OrderSide, order_side_to_string
from nautilus_trader.model.c_enums.order_type cimport OrderType, order_type_to_string
from nautilus_trader.model.c_enums.order_state cimport OrderState, order_state_to_string
from nautilus_trader.model.c_enums.order_purpose cimport OrderPurpose
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForce, time_in_force_to_string
from nautilus_trader.model.objects cimport Quantity, Price
from nautilus_trader.model.identifiers cimport Symbol, OrderId, ExecutionId
from nautilus_trader.model.events cimport OrderEvent, OrderInitialized, OrderInvalid, OrderDenied
from nautilus_trader.model.events cimport OrderSubmitted, OrderAccepted, OrderRejected
from nautilus_trader.model.events cimport OrderWorking, OrderExpired, OrderCancelled, OrderModified
from nautilus_trader.model.events cimport OrderFillEvent

# Order types which require a price to be valid
cdef set PRICED_ORDER_TYPES = {
    OrderType.LIMIT,
    OrderType.STOP,
    OrderType.STOP_LIMIT,
    OrderType.MIT
}


cdef class Order:
    """
    Represents an order for a financial market instrument.

    Attributes
    ----------
    order_id : OrderId
        The unique user identifier for the order.
    id_broker : OrderIdBroker
        The unique broker identifier for the order.
    account_id : AccountId
        The account identifier associated with the order.
    execution_id : ExecutionId
        The last execution identifier for the order.

    """

    def __init__(self,
                 OrderId order_id not None,
                 Symbol symbol not None,
                 OrderSide order_side,
                 OrderType order_type,  # 'type' hides keyword
                 Quantity quantity not None,
                 UUID init_id not None,
                 datetime timestamp not None,
                 Price price=None,
                 Label label=None,
                 OrderPurpose order_purpose=OrderPurpose.NONE,
                 TimeInForce time_in_force=TimeInForce.DAY,
                 datetime expire_time=None):
        """
        Initializes a new instance of the Order class.

        Parameters
        ----------
        order_id : OrderId
            The order unique identifier.
        symbol : Symbol
            The order symbol identifier.
        order_side : OrderSide (enum)
            The order side (BUY or SELL).
        order_type : OrderType (enum)
            The order type.
        quantity : Quantity
            The order quantity (> 0).
        init_id : UUID
            The order initialization event identifier.
        timestamp : datetime
            The order initialization timestamp.
        price : Price, optional
            The order price - must be None for non-priced orders.
            (default=None).
        label : Label, optional
            The order label / secondary identifier
            (default=None).
        order_purpose : OrderPurpose (enum)
            The specified order purpose.
            (default=OrderPurpose.NONE).
        time_in_force : TimeInForce (enum), optional
            The order time in force.
            (default=TimeInForce.DAY).
        expire_time : datetime, optional
            The order expiry time with the broker - for GTD orders only.
            (default=None).

        Raises
        ------
        ValueError
            If the quantities value is not positive (> 0).
            If the order_side is UNDEFINED.
            If the order_type is UNDEFINED.
            If the order_purpose is UNDEFINED.
            If the time_in_force is UNDEFINED.
            If the order_type should not have a price and the price is not None.
            If the order_type should have a price and the price is None.
            If the time_in_force is GTD and the expire_time is None.
        """
        Condition.not_equal(order_side, OrderSide.UNDEFINED, 'order_side', 'UNDEFINED')
        Condition.not_equal(order_type, OrderType.UNDEFINED, 'order_type', 'UNDEFINED')
        Condition.not_equal(order_purpose, OrderPurpose.UNDEFINED, 'order_purpose', 'UNDEFINED')
        Condition.not_equal(time_in_force, TimeInForce.UNDEFINED, 'time_in_force', 'UNDEFINED')
        Condition.positive(quantity.as_double(), 'quantity')

        # For orders which require a price
        if order_type in PRICED_ORDER_TYPES:
            Condition.not_none(price, 'price')
        # For orders which require no price
        else:
            Condition.none(price, 'price')

        if time_in_force == TimeInForce.GTD:
            # Must have an expire time
            Condition.not_none(expire_time, 'expire_time')

        self._execution_ids = set()         # type: {ExecutionId}
        self._events = []                   # type: [OrderEvent]

        self.id = order_id
        self.id_broker = None               # Can be None
        self.account_id = None              # Can be None
        self.position_id_broker = None      # Can be None
        self.execution_id = None            # Can be None
        self.symbol = symbol
        self.side = order_side
        self.type = order_type
        self.quantity = quantity
        self.timestamp = timestamp
        self.price = price                  # Can be None
        self.label = label                  # Can be None
        self.purpose = order_purpose
        self.time_in_force = time_in_force
        self.expire_time = expire_time      # Can be None
        self.filled_quantity = Quantity.zero()
        self.filled_timestamp = None        # Can be None
        self.average_price = None           # Can be None
        self.slippage = Decimal.zero()
        self.state = OrderState.INITIALIZED
        self.init_id = init_id
        self.is_buy = self.side == OrderSide.BUY
        self.is_sell = self.side == OrderSide.SELL
        self.is_working = False
        self.is_completed = False

        cdef OrderInitialized initialized = OrderInitialized(
            order_id=order_id,
            symbol=symbol,
            label=label,
            order_side=order_side,
            order_type=order_type,
            quantity=quantity,
            price=price,
            order_purpose=order_purpose,
            time_in_force=time_in_force,
            expire_time=expire_time,
            event_id=self.init_id,
            event_timestamp=timestamp)

        # Update events
        self._events.append(initialized)
        self.last_event = initialized
        self.event_count = 1

    @staticmethod
    cdef Order create(OrderInitialized event):
        """
        Return an order from the given initialized event.
        
        :param event: The event to initialize with.
        :return Order.
        """
        Condition.not_none(event, 'event')

        return Order(
            order_id=event.order_id,
            symbol=event.symbol,
            order_side=event.order_side,
            order_type=event.order_type,
            quantity=event.quantity,
            timestamp=event.timestamp,
            price=event.price,
            label=event.label,
            order_purpose=event.order_purpose,
            time_in_force=event.time_in_force,
            expire_time=event.expire_time,
            init_id=event.id)

    cpdef bint equals(self, Order other):
        """
        Return a value indicating whether this object is equal to (==) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.id.equals(other.id)

    def __eq__(self, Order other) -> bool:
        """
        Return a value indicating whether this object is equal to (==) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.equals(other)

    def __ne__(self, Order other) -> bool:
        """
        Return a value indicating whether this object is not equal to (!=) the given object.

        :param other: The other object.
        :return bool.
        """
        return not self.equals(other)

    def __hash__(self) -> int:
        """"
        Return the hash code of this object.

        :return int.
        """
        return hash(self.id)

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        :return str.
        """
        cdef str label = '' if self.label is None else f'label={self.label}, '
        return (f"Order("
                f"id={self.id.value}, "
                f"state={order_state_to_string(self.state)}, "
                f"{label}"
                f"{self.status_string()})")

    def __repr__(self) -> str:
        """
        Return the string representation of this object which includes the objects
        location in memory.

        :return str.
        """
        return f"<{str(self)} object at {id(self)}>"

    cpdef str status_string(self):
        """
        Return the positions status as a string.

        :return str.
        """
        cdef str price = '' if self.price is None else f'@ {self.price} '
        cdef str expire_time = '' if self.expire_time is None else f' {format_iso8601(self.expire_time)}'
        return (f"{order_side_to_string(self.side)} {self.quantity.to_string_formatted()} {self.symbol} "
                f"{order_type_to_string(self.type)} {price}"
                f"{time_in_force_to_string(self.time_in_force)}{expire_time}")

    cpdef str state_as_string(self):
        """
        Return the order state as a string.
        
        :return str.
        """
        return order_state_to_string(self.state)

    cpdef list get_execution_ids(self):
        """
        Return a sorted list of execution identifiers.
        
        :return List[ExecutionId].
        """
        return sorted(self._execution_ids)

    cpdef list get_events(self):
        """
        Return a list or order events.
        
        :return List[OrderEvent].
        """
        return self._events.copy()

    cpdef datetime last_event_time(self):
        """
        Return the last event time for the order.
        
        :return datetime.
        """
        return self._events[-1].timestamp

    cpdef void apply(self, OrderEvent event) except *:
        """
        Apply the given order event to the order.

        :param event: The order event to apply.
        :raises ValueError: If the order_events order_id is not equal to the event.order_id.
        :raises ValueError: If the order account_id is not None and is not equal to the event.account_id.
        """
        Condition.not_none(event, 'event')
        Condition.equal(self.id, event.order_id, 'id', 'event.order_id')
        if self.account_id is not None:
            Condition.equal(self.account_id, event.account_id, 'account_id', 'event.account_id')

        # Update events
        self._events.append(event)
        self.last_event = event
        self.event_count += 1

        # Handle event
        if isinstance(event, OrderInvalid):
            self.state = OrderState.INVALID
            self._set_is_completed_true()
        elif isinstance(event, OrderDenied):
            self.state = OrderState.DENIED
            self._set_is_completed_true()
        elif isinstance(event, OrderSubmitted):
            self.state = OrderState.SUBMITTED
            self.account_id = event.account_id
        elif isinstance(event, OrderRejected):
            self.state = OrderState.REJECTED
            self._set_is_completed_true()
        elif isinstance(event, OrderAccepted):
            self.state = OrderState.ACCEPTED
        elif isinstance(event, OrderWorking):
            self.state = OrderState.WORKING
            self.id_broker = event.order_id_broker
            self._set_is_working_true()
        elif isinstance(event, OrderCancelled):
            self.state = OrderState.CANCELLED
            self._set_is_completed_true()
        elif isinstance(event, OrderExpired):
            self.state = OrderState.EXPIRED
            self._set_is_completed_true()
        elif isinstance(event, OrderModified):
            self.id_broker = event.order_id_broker
            self.quantity = event.modified_quantity
            self.price = event.modified_price
        elif isinstance(event, OrderFillEvent):
            self.position_id_broker = event.position_id_broker
            self._execution_ids.add(event.execution_id)
            self.execution_id = event.execution_id
            self.filled_quantity = event.filled_quantity
            self.filled_timestamp = event.timestamp
            self.average_price = event.average_price
            self._set_slippage()
            self._set_filled_state()
            if self.state == OrderState.FILLED:
                self._set_is_completed_true()

    cdef void _set_is_working_true(self) except *:
        self.is_working = True
        self.is_completed = False

    cdef void _set_is_completed_true(self) except *:
        self.is_working = False
        self.is_completed = True

    cdef void _set_filled_state(self) except *:
        if self.filled_quantity < self.quantity:
            self.state = OrderState.PARTIALLY_FILLED
        elif self.filled_quantity == self.quantity:
            self.state = OrderState.FILLED
        elif self.filled_quantity > self.quantity:
            self.state = OrderState.OVER_FILLED

    cdef void _set_slippage(self) except *:
        if self.type not in PRICED_ORDER_TYPES:
            # Slippage only applicable to priced order types
            return

        if self.side == OrderSide.BUY:
            self.slippage = Decimal(self.average_price.as_double() - self.price.as_double(), self.average_price.precision)
        else:  # self.side == OrderSide.SELL:
            self.slippage = Decimal(self.price.as_double() - self.average_price.as_double(), self.average_price.precision)


cdef class BracketOrder:
    """
    Represents an order for a financial market instrument consisting of a 'parent'
    entry order and 'child' OCO orders representing a stop-loss and optional
    profit target.
    """
    def __init__(self,
                 Order entry not None,
                 Order stop_loss not None,
                 Order take_profit=None):
        """
        Initializes a new instance of the BracketOrder class.

        :param entry: The entry 'parent' order.
        :param stop_loss: The stop-loss (SL) 'child' order.
        :param take_profit: The optional take-profit (TP) 'child' order.
        """
        self.id = BracketOrderId('B' + entry.id.value)
        self.entry = entry
        self.stop_loss = stop_loss
        self.take_profit = take_profit
        self.has_take_profit = take_profit is not None
        self.timestamp = entry.timestamp

    cpdef bint equals(self, BracketOrder other):
        """
        Return a value indicating whether this object is equal to (==) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.id.equals(other.id)

    def __eq__(self, BracketOrder other) -> bool:
        """
        Return a value indicating whether this object is equal to (==) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.equals(other)

    def __ne__(self, BracketOrder other) -> bool:
        """
        Return a value indicating whether this object is not equal to (!=) the given object.

        :param other: The other object.
        :return bool.
        """
        return not self.equals(other)

    def __hash__(self) -> int:
        """"
        Return the hash code of this object.

        :return int.
        """
        return hash(self.id)

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        :return str.
        """
        cdef str take_profit_price = 'NONE' if self.take_profit is None or self.take_profit.price is None else self.take_profit.price.to_string()
        return f"BracketOrder(id={self.id.value}, Entry{self.entry}, SL={self.stop_loss.price}, TP={take_profit_price})"

    def __repr__(self) -> str:
        """
        Return the string representation of this object which includes the objects
        location in memory.

        :return str.
        """
        return f"<{str(self)} object at {id(self)}>"
