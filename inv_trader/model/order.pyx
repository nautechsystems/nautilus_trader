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
from typing import Dict, List

from inv_trader.core.precondition cimport Precondition
from inv_trader.common.clock cimport Clock, LiveClock
from inv_trader.enums.order_side cimport OrderSide, order_side_string
from inv_trader.enums.order_type cimport OrderType, order_type_string
from inv_trader.enums.order_status cimport OrderStatus
from inv_trader.enums.time_in_force cimport TimeInForce, time_in_force_string
from inv_trader.model.objects cimport Symbol
from inv_trader.model.events cimport OrderEvent
from inv_trader.model.events cimport OrderSubmitted, OrderAccepted, OrderRejected, OrderWorking
from inv_trader.model.events cimport OrderExpired, OrderModified, OrderCancelled, OrderCancelReject
from inv_trader.model.events cimport OrderPartiallyFilled, OrderFilled
from inv_trader.model.identifiers cimport Label, OrderId, ExecutionId, ExecutionTicket

# Order types which require prices to be valid.
cdef list PRICED_ORDER_TYPES = [
    OrderType.LIMIT,
    OrderType.STOP_MARKET,
    OrderType.STOP_LIMIT,
    OrderType.MIT]


cdef class Order:
    """
    Represents an order in a financial market.
    """

    def __init__(self,
                 Symbol symbol,
                 OrderId order_id,
                 Label label,
                 OrderSide order_side,
                 OrderType order_type,
                 int quantity,
                 datetime timestamp,
                 Price price=None,
                 TimeInForce time_in_force=TimeInForce.DAY,
                 datetime expire_time=None):
        """
        Initializes a new instance of the Order class.

        :param symbol: The orders symbol.
        :param order_id: The orders identifier.
        :param label: The orders label.
        :param order_side: The orders side.
        :param order_type: The orders type.
        :param quantity: The orders quantity (> 0).
        :param timestamp: The orders initialization timestamp.
        :param price: The orders price (can be None for market orders > 0).
        :param time_in_force: The orders time in force (optional.
        :param expire_time: The orders expire time (optional.
        :raises ValueError: If the quantity is not positive (> 0).
        :raises ValueError: If the order type has no price and the price is not None.
        :raises ValueError: If the order type has a price and the price is None.
        :raises ValueError: If the order type has a price and the price is not positive (> 0).
        :raises ValueError: If the time_in_force is GTD and the expire_time is None.
        """
        Precondition.positive(quantity, 'quantity')

        # Orders with prices
        if order_type in PRICED_ORDER_TYPES:
            Precondition.not_none(price, 'price')
        # Orders without prices
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
        self.broker_id = None
        self.execution_id = None
        self.execution_ticket = None
        self.label = label
        self.side = order_side
        self.type = order_type
        self.quantity = quantity
        self.timestamp = timestamp
        self.price = price                  # Can be None
        self.time_in_force = time_in_force  # Can be None
        self.expire_time = expire_time      # Can be None
        self.filled_quantity = 0
        self.average_price = None
        self.slippage = Decimal(0.0)
        self.status = OrderStatus.INITIALIZED
        self.event_count = 0
        self.last_event = None
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
        cdef str quantity = '{:,}'.format(self.quantity)
        cdef str price = '' if self.price is None else f'@ {self.price} '
        cdef str expire_time = '' if self.expire_time is None else f' {self.expire_time}'
        return (f"Order(id={self.id}, label={self.label}) "
                f"{order_side_string(self.side)} {quantity} {self.symbol} {order_type_string(self.type)} {price}"
                f"{time_in_force_string(self.time_in_force)}{expire_time}")

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the order.
        """
        cdef object attrs = vars(self)
        cdef str props = ', '.join("%s=%s" % item for item in attrs.items()).replace(', _', ', ')
        return f"<{self.__class__.__name__}({props[1:]}) object at {id(self)}>"

    cpdef list get_order_ids_broker(self):
        """
        :return: A copy of the list of internally held broker order ids. 
        """
        return self._order_ids_broker.copy()

    cpdef list get_execution_ids(self):
        """
        :return: A copy of the list of internally held execution ids. 
        """
        return self._execution_ids.copy()

    cpdef list get_execution_tickets(self):
        """
        :return: A copy of the list of internally held execution tickets. 
        """
        return self._execution_tickets.copy()

    cpdef list get_events(self):
        """
        :return: A copy of the list of internally held events. 
        """
        return self._events.copy()

    cpdef void apply(self, OrderEvent event):
        """
        Applies the given order event to the order.

        :param event: The order event to apply.
        :raises ValueError: If the order_events order_id is not equal to the id.
        """
        Precondition.equal(event.order_id, self.id)

        self._events.append(event)
        self.event_count += 1
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

        elif isinstance(event, OrderCancelled):
            self.status = OrderStatus.CANCELLED
            self.is_complete = True

        elif isinstance(event, OrderCancelReject):
            pass

        elif isinstance(event, OrderExpired):
            self.status = OrderStatus.EXPIRED
            self.is_complete = True

        elif isinstance(event, OrderModified):
            self._order_ids_broker.append(event.broker_order_id)
            self.broker_id = event.broker_order_id
            self.price = event.modified_price

        elif isinstance(event, OrderFilled) or isinstance(event, OrderPartiallyFilled):
            self._execution_ids.append(event.execution_id)
            self._execution_tickets.append(event.execution_ticket)
            self.execution_id = event.execution_id
            self.execution_ticket = event.execution_ticket
            self.filled_quantity = event.filled_quantity
            self.average_price = event.average_price
            self._set_slippage()
            self._set_fill_status()
            if self.status == OrderStatus.FILLED:
                self.is_complete = True

    cdef void _set_slippage(self):
        if self.type not in PRICED_ORDER_TYPES:
            # Slippage not applicable to orders with entry prices.
            return

        if self.side is OrderSide.BUY:
            self.slippage = Decimal(f'{round(self.average_price.as_float() - self.price.as_float(), self.price.precision):.{self.price.precision}f}')
        else:  # side is OrderSide.SELL:
            self.slippage = Decimal(f'{round(self.price.as_float() - self.average_price.as_float(), self.price.precision):.{self.price.precision}f}')

    cdef void _set_fill_status(self):
        if self.filled_quantity < self.quantity:
            self.status = OrderStatus.PARTIALLY_FILLED
        elif self.filled_quantity == self.quantity:
            self.status = OrderStatus.FILLED
        elif self.filled_quantity > self.quantity:
            self.status = OrderStatus.OVER_FILLED


cdef str SEPARATOR = '-'


cdef class OrderIdGenerator:
    """
    Provides a generator for unique order identifiers.
    """

    def __init__(self,
                 str order_tag_trader,
                 str order_tag_strategy,
                 Clock clock=LiveClock()):
        """
        Initializes a new instance of the OrderIdGenerator class.

        :param order_tag_trader: The order identifier tag for the trader.
        :param order_tag_strategy: The order identifier tag for the strategy.
        :param clock: The internal clock.
        :raises ValueError: If the order_tag_trader is not a valid string.
        :raises ValueError: If the order_tag_strategy is not a valid string.
        """
        Precondition.valid_string(order_tag_trader, 'order_tag_trader')
        Precondition.valid_string(order_tag_strategy, 'order_tag_strategy')

        self._clock = clock
        self._order_symbol_counts = {}  # type: Dict[Symbol, int]
        self.order_tag_trader = order_tag_trader
        self.order_tag_strategy = order_tag_strategy

    cpdef OrderId generate(self, Symbol order_symbol):
        """
        Create a unique order identifier for the strategy using the given symbol.

        :param order_symbol: The order symbol for the unique identifier.
        :return: The unique OrderIdentifier.
        """
        if order_symbol not in self._order_symbol_counts:
            self._order_symbol_counts[order_symbol] = 0

        self._order_symbol_counts[order_symbol] += 1

        return OrderId(self._clock.get_datetime_tag()
                       + SEPARATOR + self.order_tag_trader
                       + SEPARATOR + self.order_tag_strategy
                       + SEPARATOR + order_symbol.code
                       + SEPARATOR + order_symbol.venue_string()
                       + SEPARATOR + str(self._order_symbol_counts[order_symbol]))


cdef class OrderFactory:
    """
    A factory class which provides different order types.
    """

    def __init__(self, Clock clock=LiveClock()):
        """
        Initializes a new instance of the OrderFactory class.

        :param clock: The internal clock.
        """
        self._clock = clock

    cpdef Order market(
            self,
            Symbol symbol,
            OrderId order_id,
            Label label,
            OrderSide order_side,
            int quantity):
        """
        Creates and returns a new market order with the given parameters.

        :param symbol: The orders symbol.
        :param order_id: The orders identifier (must be unique).
        :param label: The orders label.
        :param order_side: The orders side.
        :param quantity: The orders quantity (> 0).
        :return: The market order.
        :raises ValueError: If the quantity is not positive (> 0).
        """
        return Order(symbol,
                     order_id,
                     label,
                     order_side,
                     OrderType.MARKET,
                     quantity,
                     self._clock.time_now(),
                     price=None,
                     time_in_force=TimeInForce.DAY,
                     expire_time=None)

    cpdef Order limit(
            self,
            Symbol symbol,
            OrderId order_id,
            Label label,
            OrderSide order_side,
            int quantity,
            Price price,
            TimeInForce time_in_force=TimeInForce.DAY,
            datetime expire_time=None):
        """
        Creates and returns a new limit order with the given parameters.
        If the time in force is GTD then a valid expire time must be given.

        :param symbol: The orders symbol.
        :param order_id: The orders identifier (must be unique).
        :param label: The orders label.
        :param order_side: The orders side.
        :param quantity: The orders quantity (> 0).
        :param price: The orders price (> 0).
        :param time_in_force: The orders time in force (can be None).
        :param expire_time: The orders expire time (can be None unless time_in_force is GTD).
        :return: The limit order.
        :raises ValueError: If the quantity is not positive (> 0).
        :raises ValueError: If the price is not positive (> 0).
        :raises ValueError: If the time_in_force is GTD and the expire_time is None.
        """
        return Order(symbol,
                     order_id,
                     label,
                     order_side,
                     OrderType.LIMIT,
                     quantity,
                     self._clock.time_now(),
                     price,
                     time_in_force,
                     expire_time)

    cpdef Order stop_market(
            self,
            Symbol symbol,
            OrderId order_id,
            Label label,
            OrderSide order_side,
            int quantity,
            Price price,
            TimeInForce time_in_force=TimeInForce.DAY,
            datetime expire_time=None):
        """
        Creates and returns a new stop-market order with the given parameters.
        If the time in force is GTD then a valid expire time must be given.

        :param symbol: The orders symbol.
        :param order_id: The orders identifier (must be unique).
        :param label: The orders label.
        :param order_side: The orders side.
        :param quantity: The orders quantity (> 0).
        :param price: The orders price (> 0).
        :param time_in_force: The orders time in force (can be None).
        :param expire_time: The orders expire time (can be None unless time_in_force is GTD).
        :return: The stop-market order.
        :raises ValueError: If the quantity is not positive (> 0).
        :raises ValueError: If the price is not positive (> 0).
        :raises ValueError: If the time_in_force is GTD and the expire_time is None.
        """
        return Order(symbol,
                     order_id,
                     label,
                     order_side,
                     OrderType.STOP_MARKET,
                     quantity,
                     self._clock.time_now(),
                     price,
                     time_in_force,
                     expire_time)

    cpdef Order stop_limit(
            self,
            Symbol symbol,
            OrderId order_id,
            Label label,
            OrderSide order_side,
            int quantity,
            Price price,
            TimeInForce time_in_force=TimeInForce.DAY,
            datetime expire_time=None):
        """
        Creates and returns a new stop-limit order with the given parameters.
        If the time in force is GTD then a valid expire time must be given.

        :param symbol: The orders symbol.
        :param order_id: The orders identifier (must be unique).
        :param label: The orders label.
        :param order_side: The orders side.
        :param quantity: The orders quantity (> 0).
        :param price: The orders price (> 0).
        :param time_in_force: The orders time in force (can be None).
        :param expire_time: The orders expire time (can be None unless time_in_force is GTD).
        :return: The stop-limit order.
        :raises ValueError: If the quantity is not positive (> 0).
        :raises ValueError: If the price is not positive (> 0).
        :raises ValueError: If the time_in_force is GTD and the expire_time is None.
        """
        return Order(symbol,
                     order_id,
                     label,
                     order_side,
                     OrderType.STOP_LIMIT,
                     quantity,
                     self._clock.time_now(),
                     price,
                     time_in_force,
                     expire_time)

    cpdef Order market_if_touched(
            self,
            Symbol symbol,
            OrderId order_id,
            Label label,
            OrderSide order_side,
            int quantity,
            Price price,
            TimeInForce time_in_force=TimeInForce.DAY,
            datetime expire_time=None):
        """
        Creates and returns a new market-if-touched order with the given parameters.
        If the time in force is GTD then a valid expire time must be given.

        :param symbol: The orders symbol.
        :param order_id: The orders identifier (must be unique).
        :param label: The orders label.
        :param order_side: The orders side.
        :param quantity: The orders quantity (> 0).
        :param price: The orders price (> 0).
        :param time_in_force: The orders time in force (can be None).
        :param expire_time: The orders expire time (can be None unless time_in_force is GTD).
        :return: The market-if-touched order.
        :raises ValueError: If the quantity is not positive (> 0).
        :raises ValueError: If the price is not positive (> 0).
        :raises ValueError: If the time_in_force is GTD and the expire_time is None.
        """
        return Order(symbol,
                     order_id,
                     label,
                     order_side,
                     OrderType.MIT,
                     quantity,
                     self._clock.time_now(),
                     price,
                     time_in_force,
                     expire_time)

    cpdef Order fill_or_kill(
            self,
            Symbol symbol,
            OrderId order_id,
            Label label,
            OrderSide order_side,
            int quantity):
        """
        Creates and returns a new fill-or-kill order with the given parameters.

        :param symbol: The orders symbol.
        :param order_id: The orders identifier (must be unique).
        :param label: The orders label.
        :param order_side: The orders side.
        :param quantity: The orders quantity (> 0).
        :return: The fill or kill order.
        :raises ValueError: If the quantity is not positive (> 0).
        """
        return Order(symbol,
                     order_id,
                     label,
                     order_side,
                     OrderType.MARKET,
                     quantity,
                     self._clock.time_now(),
                     price=None,
                     time_in_force=TimeInForce.FOC,
                     expire_time=None)

    cpdef Order immediate_or_cancel(
            self,
            Symbol symbol,
            OrderId order_id,
            Label label,
            OrderSide order_side,
            int quantity):
        """
        Creates and returns a new immediate-or-cancel order with the given parameters.

        :param symbol: The orders symbol.
        :param order_id: The orders identifier (must be unique).
        :param label: The orders label.
        :param order_side: The orders side.
        :param quantity: The orders quantity (> 0).
        :return: The immediate or cancel order.
        :raises ValueError: If the quantity is not positive (> 0).
        """
        return Order(symbol,
                     order_id,
                     label,
                     order_side,
                     OrderType.MARKET,
                     quantity,
                     self._clock.time_now(),
                     price=None,
                     time_in_force=TimeInForce.IOC,
                     expire_time=None)
