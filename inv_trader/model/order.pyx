#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="order.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

from cpython.datetime cimport datetime
from decimal import Decimal
from typing import List

from inv_trader.core.precondition cimport Precondition
from inv_trader.common.clock cimport Clock, LiveClock
from inv_trader.enums.order_side cimport OrderSide, order_side_string
from inv_trader.enums.order_type cimport OrderType, order_type_string
from inv_trader.enums.order_status cimport OrderStatus, order_status_string
from inv_trader.enums.time_in_force cimport TimeInForce, time_in_force_string
from inv_trader.model.objects cimport Quantity, Symbol, Price
from inv_trader.model.events cimport OrderEvent
from inv_trader.model.events cimport OrderSubmitted, OrderAccepted, OrderRejected, OrderWorking
from inv_trader.model.events cimport OrderExpired, OrderModified, OrderCancelled, OrderCancelReject
from inv_trader.model.events cimport OrderPartiallyFilled, OrderFilled
from inv_trader.model.identifiers cimport Label, OrderId, ExecutionId, ExecutionTicket
from inv_trader.model.identifiers cimport OrderIdGenerator


# Order types which require a price to be valid
cdef set PRICED_ORDER_TYPES = {
    OrderType.LIMIT,
    OrderType.STOP_MARKET,
    OrderType.STOP_LIMIT,
    OrderType.MIT}


cdef class Order:
    """
    Represents an order for a financial market instrument.
    """

    def __init__(self,
                 Symbol symbol,
                 OrderId order_id,
                 OrderSide order_side,
                 OrderType order_type,
                 Quantity quantity,
                 datetime timestamp,
                 Price price=None,
                 Label label=None,
                 TimeInForce time_in_force=TimeInForce.DAY,
                 datetime expire_time=None):
        """
        Initializes a new instance of the Order class.

        :param symbol: The orders symbol.
        :param order_id: The orders identifier.
        :param order_side: The orders side.
        :param order_type: The orders type.
        :param quantity: The orders quantity (> 0).
        :param timestamp: The orders initialization timestamp.
        :param price: The orders price (must be None for non priced orders).
        :param label: The optional order label / secondary identifier (can be None).
        :param time_in_force: The orders time in force (default DAY).
        :param expire_time: The optional order expire time (can be None).
        :raises ValueError: If the order quantity is not positive (> 0).
        :raises ValueError: If the order side is UNKNOWN.
        :raises ValueError: If the order type should not have a price and the price is not None.
        :raises ValueError: If the order type should have a price and the price is None.
        :raises ValueError: If the time_in_force is GTD and the expire_time is None.
        """
        Precondition.positive(quantity.value, 'quantity')
        Precondition.true(order_side != OrderSide.UNKNOWN, 'order_side != UNKNOWN')

        # For orders which require a price
        if order_type in PRICED_ORDER_TYPES:
            Precondition.not_none(price, 'price')
        # For orders which require no price
        else:
            Precondition.none(price, 'price')

        if time_in_force is TimeInForce.GTD:
            Precondition.not_none(expire_time, 'expire_time')

        self._order_ids_broker = []   # type: List[OrderId]
        self._execution_ids = []      # type: List[ExecutionId]
        self._execution_tickets = []  # type: List[ExecutionTicket]
        self._events = []             # type: List[OrderEvent]

        self.symbol = symbol
        self.id = order_id
        self.broker_id = None               # Can be None
        self.execution_id = None            # Can be None
        self.execution_ticket = None        # Can be None
        self.side = order_side
        self.type = order_type
        self.quantity = quantity
        self.timestamp = timestamp
        self.price = price                  # Can be None
        self.label = label                  # Can be None
        self.time_in_force = time_in_force  # Can be None
        self.expire_time = expire_time      # Can be None
        self.filled_quantity = Quantity(0)
        self.filled_timestamp = None        # Can be None
        self.average_price = None           # Can be None
        self.slippage = Decimal(0.0)
        self.status = OrderStatus.INITIALIZED
        self.last_event = None              # Can be None
        self.is_buy = True if self.side == OrderSide.BUY else False
        self.is_sell = True if self.side == OrderSide.SELL else False
        self.is_active = False
        self.is_complete = False

    cdef bint equals(self, Order other):
        """
        Compare if the object equals the given object.
        
        :param other: The other object to compare
        :return: True if the objects are equal, otherwise False.
        """
        return self.id.equals(other.id)

    def __eq__(self, other) -> bool:
        """
        Override the default equality comparison.
        """
        return self.equals(other)

    def __ne__(self, other) -> bool:
        """
        Override the default not-equals comparison.
        """
        return not self.equals(other)

    def __hash__(self) -> int:
        """"
        Override the default hash implementation.
        """
        return hash(self.id)

    def __str__(self) -> str:
        """
        :return: The str() string representation of the order.
        """
        cdef str quantity = '{:,}'.format(self.quantity.value)
        cdef str label = '' if self.label is None else f', label={self.label.value}'
        cdef str price = '' if self.price is None else f'@ {self.price} '
        cdef str expire_time = '' if self.expire_time is None else f' {self.expire_time}'
        return (f"Order({self.id.value}{label}, status={order_status_string(self.status)}) "
                f"{order_side_string(self.side)} {quantity} {self.symbol} {order_type_string(self.type)} {price}"
                f"{time_in_force_string(self.time_in_force)}{expire_time}")

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the order.
        """
        return f"<{str(self)} object at {id(self)}>"

    cpdef str status_as_string(self):
        """
        :return: The order status as a string.
        """
        return order_status_string(self.status)

    cpdef list get_order_ids_broker(self):
        """
        Return a list of broker order identifiers.
        
        :return: List[OrderId]. 
        """
        return self._order_ids_broker.copy()

    cpdef list get_execution_ids(self):
        """
        Return a list of execution identifiers.
        
        :return: List[ExecutionId].
        """
        return self._execution_ids.copy()

    cpdef list get_execution_tickets(self):
        """
        Return a list of execution tickets.
        
        :return: List[ExecutionTicket]. 
        """
        return self._execution_tickets.copy()

    cpdef list get_events(self):
        """
        Return a list or order events.
        
        :return: List[OrderEvent]. 
        """
        return self._events.copy()

    cpdef int event_count(self):
        """
        Return the count of events applied to the order.
        
        :return: int.
        """
        return len(self._events)

    cpdef void apply(self, OrderEvent event):
        """
        Apply the given order event to the order.

        :param event: The order event to apply.
        :raises ValueError: If the order_events order_id is not equal to the order identifier.
        """
        Precondition.equal(event.order_id, self.id)

        # Update events
        self._events.append(event)
        self.last_event = event

        # Handle event
        if isinstance(event, OrderSubmitted):
            self.status = OrderStatus.SUBMITTED

        elif isinstance(event, OrderAccepted):
            self.status = OrderStatus.ACCEPTED

        elif isinstance(event, OrderRejected):
            self.status = OrderStatus.REJECTED
            self.is_complete = True

        elif isinstance(event, OrderWorking):
            self.status = OrderStatus.WORKING
            self._order_ids_broker.append(event.broker_order_id)
            self.broker_id = event.broker_order_id
            self.is_active = True

        elif isinstance(event, OrderCancelled):
            self.status = OrderStatus.CANCELLED
            self.is_active = False
            self.is_complete = True

        elif isinstance(event, OrderCancelReject):
            pass

        elif isinstance(event, OrderExpired):
            self.status = OrderStatus.EXPIRED
            self.is_active = False
            self.is_complete = True

        elif isinstance(event, OrderModified):
            self._order_ids_broker.append(event.broker_order_id)
            self.broker_id = event.broker_order_id
            self.price = event.modified_price

        elif isinstance(event, (OrderFilled, OrderPartiallyFilled)):
            self._execution_ids.append(event.execution_id)
            self._execution_tickets.append(event.execution_ticket)
            self.execution_id = event.execution_id
            self.execution_ticket = event.execution_ticket
            self.filled_quantity = event.filled_quantity
            self.filled_timestamp = event.timestamp
            self.average_price = event.average_price
            self._set_slippage()
            self._set_fill_status()
            if self.status == OrderStatus.FILLED:
                self.is_active = False
                self.is_complete = True

    cdef void _set_slippage(self):
        if self.type not in PRICED_ORDER_TYPES:
            # Slippage only applicable to priced order types
            return

        if self.side is OrderSide.BUY:
            self.slippage = self.average_price - self.price
        else:  # self.side is OrderSide.SELL:
            self.slippage = self.price - self.average_price

        # Avoid negative zero
        if self.slippage == 0:
            self.slippage = abs(self.slippage)

    cdef void _set_fill_status(self):
        if self.filled_quantity < self.quantity:
            self.status = OrderStatus.PARTIALLY_FILLED
        elif self.filled_quantity == self.quantity:
            self.status = OrderStatus.FILLED
        elif self.filled_quantity > self.quantity:
            self.status = OrderStatus.OVER_FILLED


cdef class AtomicOrder:
    """
    Represents an order for a financial market instrument consisting of a 'parent'
    entry order and 'child' OCO orders representing a stop-loss and optional
    profit target.
    """
    def __init__(self,
                 Order entry,
                 Order stop_loss,
                 Order take_profit=None):
        """
        Initializes a new instance of the AtomicOrder class.

        :param entry: The entry 'parent' order.
        :param stop_loss: The stop-loss (S/L) 'child' order.
        :param take_profit: The optional take-profit (T/P) 'child' order (can be None).
        """
        self.entry = entry
        self.stop_loss = stop_loss
        self.take_profit = take_profit
        self.has_take_profit = take_profit is not None
        self.id = OrderId('A' + entry.id.value)
        self.timestamp = entry.timestamp

    cdef bint equals(self, AtomicOrder other):
        """
        Compare if the object equals the given object.
        
        :param other: The other object to compare
        :return: True if the objects are equal, otherwise False.
        """
        return self.id.equals(other.id)

    def __eq__(self, other) -> bool:
        """
        Override the default equality comparison.
        """
        return self.equals(other)

    def __ne__(self, other) -> bool:
        """
        Override the default not-equals comparison.
        """
        return not self.equals(other)

    def __hash__(self) -> int:
        """"
        Override the default hash implementation.
        """
        return hash(self.id)

    def __str__(self) -> str:
        """
        :return: The str() string representation of the order.
        """
        return f"AtomicOrder(Entry{self.entry}, has_take_profit={self.has_take_profit})"

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the order.
        """
        return f"<{str(self)} object at {id(self)}>"


cdef class OrderFactory:
    """
    A factory class which provides different order types.
    """

    def __init__(self,
                 str id_tag_trader,
                 str id_tag_strategy,
                 Clock clock=LiveClock()):
        """
        Initializes a new instance of the OrderFactory class.

        :param id_tag_trader: The identifier tag for the trader.
        :param id_tag_strategy: The identifier tag for the strategy.
        :param clock: The clock for the component.
        """
        self._clock = clock
        self._id_generator = OrderIdGenerator(
            id_tag_trader=id_tag_trader,
            id_tag_strategy=id_tag_strategy,
            clock=clock)

    cpdef void reset(self):
        """
        Reset the order factory by clearing all stateful internal values.
        """
        self._id_generator.reset()

    cpdef Order market(
            self,
            Symbol symbol,
            OrderSide order_side,
            Quantity quantity,
            Label label=None):
        """
        Return a market order with the given parameters.

        :param symbol: The orders symbol.
        :param order_side: The orders side.
        :param quantity: The orders quantity (> 0).
        :param label: The orders label (can be None).
        :raises ValueError: If the order quantity is not positive (> 0).
        :return: Order.
        """
        return Order(
            symbol,
            self._id_generator.generate(),
            order_side,
            OrderType.MARKET,
            quantity,
            self._clock.time_now(),
            price=None,
            label=label,
            time_in_force=TimeInForce.DAY,
            expire_time=None)

    cpdef Order limit(
            self,
            Symbol symbol,
            OrderSide order_side,
            Quantity quantity,
            Price price,
            Label label=None,
            TimeInForce time_in_force=TimeInForce.DAY,
            datetime expire_time=None):
        """
        Returns a limit order with the given parameters.

        Note: If the time in force is GTD then a valid expire time must be given.
        :param symbol: The orders symbol.
        :param order_side: The orders side.
        :param quantity: The orders quantity (> 0).
        :param price: The orders price.
        :param label: The orders label (can be None).
        :param time_in_force: The orders time in force (can be None).
        :param expire_time: The orders expire time (can be None unless time_in_force is GTD).
        :return: Order.
        :raises ValueError: If the order quantity is not positive (> 0).
        :raises ValueError: If the time_in_force is GTD and the expire_time is None.
        """
        return Order(
            symbol,
            self._id_generator.generate(),
            order_side,
            OrderType.LIMIT,
            quantity,
            self._clock.time_now(),
            price,
            label,
            time_in_force,
            expire_time)

    cpdef Order stop_market(
            self,
            Symbol symbol,
            OrderSide order_side,
            Quantity quantity,
            Price price,
            Label label=None,
            TimeInForce time_in_force=TimeInForce.DAY,
            datetime expire_time=None):
        """
        Returns a stop-market order with the given parameters.

        Note: If the time in force is GTD then a valid expire time must be given.
        :param symbol: The orders symbol.
        :param order_side: The orders side.
        :param quantity: The orders quantity (> 0).
        :param price: The orders price.
        :param label: The orders label (can be None).
        :param time_in_force: The orders time in force (can be None).
        :param expire_time: The orders expire time (can be None unless time_in_force is GTD).
        :return: Order.
        :raises ValueError: If the order quantity is not positive (> 0).
        :raises ValueError: If the time_in_force is GTD and the expire_time is None.
        """
        return Order(
            symbol,
            self._id_generator.generate(),
            order_side,
            OrderType.STOP_MARKET,
            quantity,
            self._clock.time_now(),
            price,
            label,
            time_in_force,
            expire_time)

    cpdef Order stop_limit(
            self,
            Symbol symbol,
            OrderSide order_side,
            Quantity quantity,
            Price price,
            Label label=None,
            TimeInForce time_in_force=TimeInForce.DAY,
            datetime expire_time=None):
        """
        Return a stop-limit order with the given parameters.

        Note: If the time in force is GTD then a valid expire time must be given.
        :param symbol: The orders symbol.
        :param order_side: The orders side.
        :param quantity: The orders quantity (> 0).
        :param price: The orders price.
        :param label: The orders label (can be None).
        :param time_in_force: The orders time in force (can be None).
        :param expire_time: The orders expire time (can be None unless time_in_force is GTD).
        :return: Order.
        :raises ValueError: If the order quantity is not positive (> 0).
        :raises ValueError: If the time_in_force is GTD and the expire_time is None.
        """
        return Order(
            symbol,
            self._id_generator.generate(),
            order_side,
            OrderType.STOP_LIMIT,
            quantity,
            self._clock.time_now(),
            price,
            label,
            time_in_force,
            expire_time)

    cpdef Order market_if_touched(
            self,
            Symbol symbol,
            OrderSide order_side,
            Quantity quantity,
            Price price,
            Label label=None,
            TimeInForce time_in_force=TimeInForce.DAY,
            datetime expire_time=None):
        """
        Return a market-if-touched order with the given parameters.
        
        Note: If the time in force is GTD then a valid expire time must be given.
        :param symbol: The orders symbol.
        :param order_side: The orders side.
        :param quantity: The orders quantity (> 0).
        :param price: The orders price.
        :param label: The orders label (can be None).
        :param time_in_force: The orders time in force (can be None).
        :param expire_time: The orders expire time (can be None unless time_in_force is GTD).
        :return: Order.
        :raises ValueError: If the order quantity is not positive (> 0).
        :raises ValueError: If the time_in_force is GTD and the expire_time is None.
        """
        return Order(
            symbol,
            self._id_generator.generate(),
            order_side,
            OrderType.MIT,
            quantity,
            self._clock.time_now(),
            price,
            label,
            time_in_force,
            expire_time)

    cpdef Order fill_or_kill(
            self,
            Symbol symbol,
            OrderSide order_side,
            Quantity quantity,
            Label label=None):
        """
        Return a fill-or-kill order with the given parameters.

        :param symbol: The orders symbol.
        :param order_side: The orders side.
        :param quantity: The orders quantity (> 0).
        :param label: The orders label (can be None).
        :return: Order.
        :raises ValueError: If the order quantity is not positive (> 0).
        """
        return Order(
            symbol,
            self._id_generator.generate(),
            order_side,
            OrderType.MARKET,
            quantity,
            self._clock.time_now(),
            price=None,
            label=label,
            time_in_force=TimeInForce.FOC,
            expire_time=None)

    cpdef Order immediate_or_cancel(
            self,
            Symbol symbol,
            OrderSide order_side,
            Quantity quantity,
            Label label=None):
        """
        Return a immediate-or-cancel order with the given parameters.

        :param symbol: The orders symbol.
        :param order_side: The orders side.
        :param quantity: The orders quantity (> 0).
        :param label: The orders label (can be None).
        :return: Order.
        :raises ValueError: If the order quantity is not positive (> 0).
        """
        return Order(
            symbol,
            self._id_generator.generate(),
            order_side,
            OrderType.MARKET,
            quantity,
            self._clock.time_now(),
            price=None,
            label=label,
            time_in_force=TimeInForce.IOC,
            expire_time=None)

    cpdef AtomicOrder atomic_market(
            self,
            Symbol symbol,
            OrderSide order_side,
            Quantity quantity,
            Price price_stop_loss,
            Price price_take_profit=None,
            Label label=None):
        """
        Return a market entry atomic order with the given parameters.

        :param symbol: The orders symbol.
        :param order_side: The orders side.
        :param quantity: The orders quantity (> 0).
        :param price_stop_loss: The stop-loss order price.
        :param price_take_profit: The optional take-profit order price (can be None).
        :param label: The orders label (can be None).
        :return: AtomicOrder.
        :raises ValueError: If the order quantity is not positive (> 0).
        """
        cdef Label entry_label = None
        if label is not None:
            entry_label = Label(label.value + '_E')

        cdef Order entry_order = self.market(
            symbol,
            order_side,
            quantity,
            entry_label)

        return self._create_atomic_order(
            entry_order,
            price_stop_loss,
            price_take_profit,
            label)

    cpdef AtomicOrder atomic_limit(
            self,
            Symbol symbol,
            OrderSide order_side,
            Quantity quantity,
            Price price_entry,
            Price price_stop_loss,
            Price price_take_profit=None,
            Label label=None,
            TimeInForce time_in_force=TimeInForce.DAY,
            datetime expire_time=None):
        """
        Return a limit entry atomic order with the given parameters.


        :param symbol: The orders symbol.
        :param order_side: The orders side.
        :param quantity: The orders quantity (> 0).
        :param price_entry: The parent orders entry price.
        :param price_stop_loss: The stop-loss order price.
        :param price_take_profit: The optional take-profit order price (can be None).
        :param label: The optional order label (can be None).
        :param time_in_force: The optional order time in force (can be None).
        :param expire_time: The orders expire time (can be None unless time_in_force is GTD).
        :return: AtomicOrder.
        :raises ValueError: If the order quantity is not positive (> 0).
        :raises ValueError: If the time_in_force is GTD and the expire_time is None.
        """
        cdef Label entry_label = None
        if label is not None:
            entry_label = Label(label.value + '_E')

        cdef Order entry_order = self.limit(
            symbol,
            order_side,
            quantity,
            price_entry,
            label,
            time_in_force,
            expire_time)

        return self._create_atomic_order(
            entry_order,
            price_stop_loss,
            price_take_profit,
            label)

    cpdef AtomicOrder atomic_stop_market(
            self,
            Symbol symbol,
            OrderSide order_side,
            Quantity quantity,
            Price price_entry,
            Price price_stop_loss,
            Price price_take_profit=None,
            Label label=None,
            TimeInForce time_in_force=TimeInForce.DAY,
            datetime expire_time=None):
        """
        Return a stop-market entry atomic order with the given parameters.

        :param symbol: The orders symbol.
        :param order_side: The orders side.
        :param quantity: The orders quantity (> 0).
        :param price_entry: The parent orders entry price.
        :param price_stop_loss: The stop-loss order price.
        :param price_take_profit: The optional take-profit order price (can be None).
        :param label: The orders label (can be None).
        :param time_in_force: The orders time in force (can be None).
        :param expire_time: The orders expire time (can be None unless time_in_force is GTD).
        :return: AtomicOrder.
        :raises ValueError: If the order quantity is not positive (> 0).
        :raises ValueError: If the time_in_force is GTD and the expire_time is None.
        """
        cdef Label entry_label = None
        if label is not None:
            entry_label = Label(label.value + '_E')

        cdef Order entry_order = self.stop_market(
            symbol,
            order_side,
            quantity,
            price_entry,
            label,
            time_in_force,
            expire_time)

        return self._create_atomic_order(
            entry_order,
            price_stop_loss,
            price_take_profit,
            label)

    cdef AtomicOrder _create_atomic_order(
        self,
        Order entry,
        Price price_stop_loss,
        Price price_take_profit,
        Label original_label):
        cdef OrderSide child_order_side = OrderSide.BUY if entry.side is OrderSide.SELL else OrderSide.SELL

        cdef Label label_stop_loss = None
        cdef Label label_take_profit = None
        if original_label is not None:
            label_stop_loss = Label(original_label.value + "_SL")
            label_take_profit = Label(original_label.value + "_PT")

        cdef Order stop_loss = self.stop_market(
            entry.symbol,
            child_order_side,
            entry.quantity,
            price_stop_loss,
            label_stop_loss,
            TimeInForce.GTC,
            expire_time=None)

        cdef Order take_profit = None
        if price_take_profit is not None:
            take_profit = self.limit(
                entry.symbol,
                child_order_side,
                entry.quantity,
                price_take_profit,
                label_take_profit,
                TimeInForce.GTC,
                expire_time=None)

        return AtomicOrder(
            entry,
            stop_loss,
            take_profit)
