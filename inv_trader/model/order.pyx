#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="order.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False

import datetime as dt

from cpython.datetime cimport datetime
from datetime import timezone
from decimal import Decimal
from typing import Dict, List

from inv_trader.core.precondition cimport Precondition
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
                 price: Decimal or None=None,
                 TimeInForce time_in_force=TimeInForce.NONE,
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
        :param time_in_force: The orders time in force (optional can be None).
        :param expire_time: The orders expire time (optional can be None).
        :raises ValueError: If the order_id is not a valid string.
        :raises ValueError: If the label is not a valid string.
        :raises ValueError: If the quantity is not positive (> 0).
        :raises ValueError: If the order type has no price and the price is not None.
        :raises ValueError: If the order type has a price and the price is None.
        :raises ValueError: If the order type has a price and the price is not positive (> 0).
        :raises ValueError: If the time_in_force is GTD and the expire_time is None.
        """
        Precondition.type_or_none(price, Decimal, 'price')

        if time_in_force == TimeInForce.NONE:
            time_in_force = TimeInForce.DAY

        Precondition.positive(quantity, 'quantity')
        # Orders without prices
        if order_type not in PRICED_ORDER_TYPES:
            Precondition.none(price, 'price')
        # Orders with prices
        if order_type in PRICED_ORDER_TYPES:
            Precondition.not_none(price, 'price')
            Precondition.positive(price, 'price')
        if time_in_force is TimeInForce.GTD:
            Precondition.not_none(expire_time, 'expire_time')

        self.symbol = symbol
        self.id = order_id
        self.label = label
        self.side = order_side
        self.type = order_type
        self.quantity = quantity
        self.timestamp = timestamp
        self.price = price                  # Can be None
        self.time_in_force = time_in_force  # Can be None
        self.expire_time = expire_time      # Can be None
        self.filled_quantity = 0
        self.average_price = Decimal('0.0')
        self.slippage = Decimal('0.0')
        self.status = OrderStatus.INITIALIZED
        self.events = []                # type: List[OrderEvent]
        self._order_ids = [order_id]    # type: List[OrderId]
        self._order_ids_broker = []     # type: List[OrderId]
        self._execution_ids = []        # type: List[ExecutionId]
        self._execution_tickets = []    # type: List[ExecutionTicket]

    def __eq__(self, other) -> bool:
        """
        Override the default equality comparison.
        """
        if isinstance(other, self.__class__):
            return self.id == other.id
        else:
            return False

    def __ne__(self, other) -> bool:
        """
        Override the default not-equals comparison.
        """
        return not self.__eq__(other)

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
        cdef str price = '' if self.price is None else f' {self.price}'
        cdef str expire_time = '' if self.expire_time is None else f' {self.expire_time}'
        return (f"Order(id={self.id}, label={self.label}) "
                f"{order_side_string(self.side)} {quantity} {self.symbol} @ {order_type_string(self.type)}{price} "
                f"{time_in_force_string(self.time_in_force)}{expire_time}")

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the order.
        """
        cdef object attrs = vars(self)
        cdef str props = ', '.join("%s=%s" % item for item in attrs.items()).replace(', _', ', ')
        return f"<{self.__class__.__name__}({props[1:]}) object at {id(self)}>"

    @property
    def id_current(self) -> OrderId:
        """
        :return: The orders current identifier.
        """
        return self._order_ids[-1]

    @property
    def broker_id(self) -> OrderId or None:
        """
        :return: The orders broker-side order identifier.
        """
        if len(self._order_ids_broker) == 0:
            return None
        return self._order_ids_broker[-1]

    @property
    def execution_id(self) -> ExecutionId or None:
        """
        :return: The orders last execution.
        """
        if len(self._execution_ids) == 0:
            return None
        return self._execution_ids[-1]

    @property
    def execution_ticket(self) -> ExecutionTicket or None:
        """
        :return: The orders last execution ticket.
        """
        if len(self._execution_tickets) == 0:
            return None
        return self._execution_tickets[-1]

    @property
    def is_complete(self) -> bool:
        """
        :return: A value indicating whether the order is complete.
        """
        return (self.status is OrderStatus.CANCELLED
                or self.status is OrderStatus.EXPIRED
                or self.status is OrderStatus.FILLED
                or self.status is OrderStatus.REJECTED)

    @property
    def event_count(self) -> int:
        """
        :return: The count of events since the order was initialized (int).
        """
        return len(self.events)

    cpdef void apply(self, OrderEvent order_event):
        """
        Applies the given order event to the order.

        :param order_event: The order event to apply.
        :raises ValueError: If the order_events order_id is not equal to the id.
        """
        Precondition.equal(order_event.order_id, self.id)

        self.events.append(order_event)

        # Handle event
        if isinstance(order_event, OrderSubmitted):
            self.status = OrderStatus.SUBMITTED

        elif isinstance(order_event, OrderAccepted):
            self.status = OrderStatus.ACCEPTED

        elif isinstance(order_event, OrderRejected):
            self.status = OrderStatus.REJECTED

        elif isinstance(order_event, OrderWorking):
            self.status = OrderStatus.WORKING
            self._order_ids_broker.append(order_event.broker_order_id)

        elif isinstance(order_event, OrderCancelled):
            self.status = OrderStatus.CANCELLED

        elif isinstance(order_event, OrderCancelReject):
            pass

        elif isinstance(order_event, OrderExpired):
            self.status = OrderStatus.EXPIRED

        elif isinstance(order_event, OrderModified):
            self._order_ids_broker.append(order_event.broker_order_id)
            self.price = order_event.modified_price

        elif isinstance(order_event, OrderFilled):
            self.status = OrderStatus.FILLED
            self._execution_ids.append(order_event.execution_id)
            self._execution_tickets.append(order_event.execution_ticket)
            self.filled_quantity = order_event.filled_quantity
            self.average_price = order_event.average_price
            self._set_slippage()
            self._check_overfill()

        elif isinstance(order_event, OrderPartiallyFilled):
            self.status = OrderStatus.PARTIALLY_FILLED
            self._execution_ids.append(order_event.execution_id)
            self._execution_tickets.append(order_event.execution_ticket)
            self.filled_quantity = order_event.filled_quantity
            self.average_price = order_event.average_price
            self._set_slippage()
            self._check_overfill()

    cdef object _set_slippage(self):
        if self.type not in PRICED_ORDER_TYPES:
            # Slippage not applicable to orders with entry prices.
            return

        if self.side is OrderSide.BUY:
            self.slippage = self.average_price - self.price
        else:  # side is OrderSide.SELL:
            self.slippage = (self.price - self.average_price)

    cdef object _check_overfill(self):
        if self.filled_quantity > self.quantity:
            self.status = OrderStatus.OVER_FILLED


# Unix epoch is the UTC time at 00:00:00 on 1/1/1970
cdef object UNIX_EPOCH = datetime(1970, 1, 1, 0, 0, 0, 0, timezone.utc)
cdef str SEPARATOR = '-'
cdef int MILLISECONDS_PER_SECOND = 1000


cdef class OrderIdGenerator:
    """
    Provides a generator for unique order identifiers.
    """
    cdef str _order_id_tag
    cdef object _order_symbol_counts
    cdef list _order_ids

    def __init__(self, str order_id_tag):
        """
        Initializes a new instance of the OrderIdentifierFactory class.

        :param order_id_tag: The generators unique order identifier tag.
        :raises ValueError: If the order_id_tag is not a valid string.
        """
        Precondition.valid_string(order_id_tag, 'order_id_tag')

        self._order_id_tag = order_id_tag
        self._order_symbol_counts = {}  # type: Dict[Symbol, int]
        self._order_ids = []            # type: List[OrderId]

    cpdef OrderId generate(self, Symbol order_symbol):
        """
        Create a unique order identifier for the strategy using the given symbol.

        :param order_symbol: The order symbol for the unique identifier.
        :return: The unique OrderIdentifier.
        """
        if order_symbol not in self._order_symbol_counts:
            self._order_symbol_counts[order_symbol] = 0

        self._order_symbol_counts[order_symbol] += 1
        cdef str milliseconds = str(OrderIdGenerator._milliseconds_since_unix_epoch())
        cdef str order_count = str(self._order_symbol_counts[order_symbol])
        cdef OrderId order_id = OrderId(str(order_symbol.code)
                                       + SEPARATOR + order_symbol.venue_string()
                                       + SEPARATOR + order_count
                                       + SEPARATOR + self._order_id_tag
                                       + SEPARATOR + milliseconds)

        if order_id in self._order_ids:
            return self.generate(order_symbol)
        self._order_ids.append(order_id)
        return order_id

    @staticmethod
    cdef long _milliseconds_since_unix_epoch():
        """
        Returns the number of ticks of the given time now since the Unix Epoch.

        :return: The milliseconds since the Unix Epoch.
        """
        return (dt.datetime.now(timezone.utc) - UNIX_EPOCH).total_seconds() * MILLISECONDS_PER_SECOND


cdef class OrderFactory:
    """
    A static factory class which provides different order types.
    """

    @staticmethod
    def market(
            Symbol symbol,
            OrderId order_id,
            Label label,
            OrderSide order_side,
            int quantity) -> Order:
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
                     dt.datetime.now(timezone.utc),
                     price=None,
                     time_in_force=TimeInForce.NONE,
                     expire_time=None)

    @staticmethod
    def limit(
            Symbol symbol,
            OrderId order_id,
            Label label,
            OrderSide order_side,
            int quantity,
            price: Decimal,
            TimeInForce time_in_force=TimeInForce.NONE,
            datetime expire_time=None) -> Order:
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
                     dt.datetime.now(timezone.utc),
                     price,
                     time_in_force,
                     expire_time)

    @staticmethod
    def stop(
            Symbol symbol,
            OrderId order_id,
            Label label,
            OrderSide order_side,
            int quantity,
            price: Decimal,
            TimeInForce time_in_force=TimeInForce.NONE,
            datetime expire_time=None) -> Order:
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
                     dt.datetime.now(timezone.utc),
                     price,
                     time_in_force,
                     expire_time)

    @staticmethod
    def stop_limit(
            Symbol symbol,
            OrderId order_id,
            Label label,
            OrderSide order_side,
            int quantity,
            price: Decimal,
            TimeInForce time_in_force=TimeInForce.NONE,
            datetime expire_time=None) -> Order:
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
                     dt.datetime.now(timezone.utc),
                     price,
                     time_in_force,
                     expire_time)

    @staticmethod
    def market_if_touched(
            Symbol symbol,
            OrderId order_id,
            Label label,
            OrderSide order_side,
            int quantity,
            price: Decimal,
            TimeInForce time_in_force=TimeInForce.NONE,
            datetime expire_time=None) -> Order:
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
                     dt.datetime.now(timezone.utc),
                     price,
                     time_in_force,
                     expire_time)

    @staticmethod
    def fill_or_kill(
            Symbol symbol,
            OrderId order_id,
            Label label,
            OrderSide order_side,
            int quantity) -> Order:
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
                     dt.datetime.now(timezone.utc),
                     price=None,
                     time_in_force=TimeInForce.FOC,
                     expire_time=None)

    @staticmethod
    def immediate_or_cancel(
            Symbol symbol,
            OrderId order_id,
            Label label,
            OrderSide order_side,
            int quantity) -> Order:
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
                     dt.datetime.now(timezone.utc),
                     price=None,
                     time_in_force=TimeInForce.IOC,
                     expire_time=None)
