#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="order.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False

from datetime import datetime, timezone
from decimal import Decimal
from typing import Dict, List

from inv_trader.core.precondition cimport Precondition
from inv_trader.model.enums import OrderSide, OrderType, TimeInForce, OrderStatus
from inv_trader.model.objects import Symbol
from inv_trader.model.events import OrderEvent
from inv_trader.model.events import OrderSubmitted, OrderAccepted, OrderRejected, OrderWorking
from inv_trader.model.events import OrderExpired, OrderModified, OrderCancelled, OrderCancelReject
from inv_trader.model.events import OrderPartiallyFilled, OrderFilled
from inv_trader.model.identifiers import Label, OrderId, ExecutionId, ExecutionTicket

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
    cdef object _symbol
    cdef object _id
    cdef object _label
    cdef object _side
    cdef object _type
    cdef int _quantity
    cdef object _timestamp
    cdef object _price
    cdef object _time_in_force
    cdef object _expire_time
    cdef int _filled_quantity
    cdef object _average_price
    cdef object _slippage
    cdef object _status
    cdef list _events
    cdef list _order_ids
    cdef list _order_ids_broker
    cdef list _execution_ids
    cdef list _execution_tickets

    def __init__(self,
                 symbol: Symbol,
                 order_id: OrderId,
                 label: Label,
                 order_side: OrderSide,
                 order_type: OrderType,
                 int quantity,
                 timestamp: datetime,
                 price: Decimal or None=None,
                 time_in_force: TimeInForce or None=None,
                 expire_time: datetime or None=None):
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
        Precondition.type(symbol, Symbol, 'symbol')
        Precondition.type(order_id, OrderId, 'order_id')
        Precondition.type(label, Label, 'label')
        Precondition.type(order_side, OrderSide, 'order_side')
        Precondition.type(order_type, OrderType, 'order_type')
        Precondition.type(timestamp, datetime, 'timestamp')
        Precondition.type_or_none(price, Decimal, 'price')
        Precondition.type_or_none(time_in_force, TimeInForce, 'time_in_force')
        Precondition.type_or_none(expire_time, datetime, 'expire_time')

        if time_in_force is None:
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

        self._symbol = symbol
        self._id = order_id
        self._label = label
        self._side = order_side
        self._type = order_type
        self._quantity = quantity
        self._timestamp = timestamp
        self._price = price                  # Can be None
        self._time_in_force = time_in_force  # Can be None
        self._expire_time = expire_time      # Can be None
        self._filled_quantity = 0
        self._average_price = Decimal('0.0')
        self._slippage = Decimal('0.0')
        self._status = OrderStatus.INITIALIZED
        self._events = []               # type: List[OrderEvent]
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
        return hash(self._id)

    def __str__(self) -> str:
        """
        :return: The str() string representation of the order.
        """
        cdef str quantity = '{:,}'.format(self._quantity)
        cdef str price = '' if self._price is None else f' {self.price}'
        cdef str expire_time = '' if self._expire_time is None else f' {self._expire_time}'
        return (f"Order(id={self._id}, label={self._label}) "
                f"{self._side.name} {quantity} {self._symbol} @ {self._type.name}{price} "
                f"{self._time_in_force.name}{expire_time}")

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the order.
        """
        cdef str attrs = vars(self)
        cdef str props = ', '.join("%s=%s" % item for item in attrs.items()).replace(', _', ', ')
        return f"<{self.__class__.__name__}({props[1:]}) object at {id(self)}>"

    @property
    def symbol(self) -> Symbol:
        """
        :return: The orders symbol.
        """
        return self._symbol

    @property
    def id(self) -> OrderId:
        """
        :return: The orders identifier.
        """
        return self._id

    @property
    def id_current(self) -> OrderId:
        """
        :return: The orders current identifier.
        """
        return self._order_ids[-1]

    @property
    def broker_id(self) -> OrderId or None:
        """
        :return: The orders broker-side order identifier (could be None).
        """
        if len(self._order_ids_broker) == 0:
            return None
        return self._order_ids_broker[-1]

    @property
    def execution_id(self) -> ExecutionId or None:
        """
        :return: The orders last execution (could be None).
        """
        if len(self._execution_ids) == 0:
            return None
        return self._execution_ids[-1]

    @property
    def execution_ticket(self) -> ExecutionTicket or None:
        """
        :return: The orders last execution ticket (could be None).
        """
        if len(self._execution_tickets) == 0:
            return None
        return self._execution_tickets[-1]

    @property
    def label(self) -> Label:
        """
        :return: The orders label.
        """
        return self._label

    @property
    def side(self) -> OrderSide:
        """
        :return: The orders side.
        """
        return self._side

    @property
    def type(self) -> OrderType:
        """
        :return: The orders type.
        """
        return self._type

    @property
    def quantity(self) -> int:
        """
        :return: The orders quantity.
        """
        return self._quantity

    @property
    def filled_quantity(self) -> int:
        """
        :return: The orders filled quantity.
        """
        return self._filled_quantity

    @property
    def timestamp(self) -> datetime:
        """
        :return: The orders initialization timestamp.
        """
        return self._timestamp

    @property
    def time_in_force(self) -> TimeInForce:
        """
        :return: The orders time in force.
        """
        return self._time_in_force

    @property
    def expire_time(self) -> datetime or None:
        """
        :return: The orders expire time (optional could be None).
        """
        return self._expire_time

    @property
    def price(self) -> Decimal or None:
        """
        :return: The orders price (optional could be None).
        """
        return self._price

    @property
    def average_price(self) -> Decimal or None:
        """
        :return: The orders average filled price (optional could be None).
        """
        return self._average_price

    @property
    def slippage(self) -> Decimal:
        """
        :return: The orders filled slippage (zero if not filled).
        """
        return self._slippage

    @property
    def status(self) -> OrderStatus:
        """
        :return: The orders status.
        """
        return self._status

    @property
    def is_complete(self) -> bool:
        """
        :return: A value indicating whether the order is complete.
        """
        return (self._status is OrderStatus.CANCELLED
                or self._status is OrderStatus.EXPIRED
                or self._status is OrderStatus.FILLED
                or self._status is OrderStatus.REJECTED)

    @property
    def event_count(self) -> int:
        """
        :return: The count of events since the order was initialized.
        """
        return len(self._events)

    cpdef void apply(self, order_event: OrderEvent):
        """
        Applies the given order event to the order.

        :param order_event: The order event to apply.
        :raises ValueError: If the order_events order_id is not equal to the id.
        """
        Precondition.type(order_event, OrderEvent, 'order_event')
        Precondition.equal(order_event.order_id, self._id)

        self._events.append(order_event)

        # Handle event
        if isinstance(order_event, OrderSubmitted):
            self._status = OrderStatus.SUBMITTED

        elif isinstance(order_event, OrderAccepted):
            self._status = OrderStatus.ACCEPTED

        elif isinstance(order_event, OrderRejected):
            self._status = OrderStatus.REJECTED

        elif isinstance(order_event, OrderWorking):
            self._status = OrderStatus.WORKING
            self._order_ids_broker.append(order_event.broker_order_id)

        elif isinstance(order_event, OrderCancelled):
            self._status = OrderStatus.CANCELLED

        elif isinstance(order_event, OrderCancelReject):
            pass

        elif isinstance(order_event, OrderExpired):
            self._status = OrderStatus.EXPIRED

        elif isinstance(order_event, OrderModified):
            self._order_ids_broker.append(order_event.broker_order_id)
            self._price = order_event.modified_price

        elif isinstance(order_event, OrderFilled):
            self._status = OrderStatus.FILLED
            self._execution_ids.append(order_event.execution_id)
            self._execution_tickets.append(order_event.execution_ticket)
            self._filled_quantity = order_event.filled_quantity
            self._average_price = order_event.average_price
            self._set_slippage()
            self._check_overfill()

        elif isinstance(order_event, OrderPartiallyFilled):
            self._status = OrderStatus.PARTIALLY_FILLED
            self._execution_ids.append(order_event.execution_id)
            self._execution_tickets.append(order_event.execution_ticket)
            self._filled_quantity = order_event.filled_quantity
            self._average_price = order_event.average_price
            self._set_slippage()
            self._check_overfill()

    def get_events(self) -> List[OrderEvent]:
        """
        :return: The orders internal events list.
        """
        return self._events

    def _set_slippage(self):
        if self._type not in PRICED_ORDER_TYPES:
            # Slippage not applicable to orders with entry prices.
            return

        if self.side is OrderSide.BUY:
            self._slippage = self._average_price - self._price
        else:  # side is OrderSide.SELL:
            self._slippage = (self._price - self._average_price)

    def _check_overfill(self):
        if self._filled_quantity > self._quantity:
            self._status = OrderStatus.OVER_FILLED


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

    cpdef object generate(self, order_symbol: Symbol):
        """
        Create a unique order identifier for the strategy using the given symbol.

        :param order_symbol: The order symbol for the unique identifier.
        :return: The unique OrderIdentifier.
        """
        Precondition.type(order_symbol, Symbol, 'order_symbol')

        if order_symbol not in self._order_symbol_counts:
            self._order_symbol_counts[order_symbol] = 0

        self._order_symbol_counts[order_symbol] += 1
        cdef str milliseconds = str(OrderIdGenerator._milliseconds_since_unix_epoch())
        cdef str order_count = str(self._order_symbol_counts[order_symbol])
        cdef object order_id = OrderId(str(order_symbol.code)
                                       + SEPARATOR + str(order_symbol.venue.name)
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
        return (datetime.now(timezone.utc) - UNIX_EPOCH).total_seconds() * MILLISECONDS_PER_SECOND


class OrderFactory:
    """
    A static factory class which provides different order types.
    """

    @staticmethod
    def market(
            symbol: Symbol,
            order_id: OrderId,
            label: Label,
            order_side: OrderSide,
            int quantity) -> Order:
        """
        Creates and returns a new market order with the given parameters.

        :param symbol: The orders symbol.
        :param order_id: The orders identifier (must be unique).
        :param label: The orders label.
        :param order_side: The orders side.
        :param quantity: The orders quantity (> 0).
        :return: The market order.
        :raises ValueError: If the order_id is not a valid string.
        :raises ValueError: If the label is not a valid string.
        :raises ValueError: If the quantity is not positive (> 0).
        """
        # Preconditions checked inside Order.

        return Order(symbol,
                     order_id,
                     label,
                     order_side,
                     OrderType.MARKET,
                     quantity,
                     datetime.now(timezone.utc),
                     price=None,
                     time_in_force=None,
                     expire_time=None)

    @staticmethod
    def limit(
            symbol: Symbol,
            order_id: OrderId,
            label: Label,
            order_side: OrderSide,
            int quantity,
            price: Decimal,
            time_in_force=None,
            expire_time=None) -> Order:
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
        :raises ValueError: If the order_id is not a valid string.
        :raises ValueError: If the label is not a valid string.
        :raises ValueError: If the quantity is not positive (> 0).
        :raises ValueError: If the price is not positive (> 0).
        :raises ValueError: If the time_in_force is GTD and the expire_time is None.
        """
        # Preconditions checked inside Order.

        return Order(symbol,
                     order_id,
                     label,
                     order_side,
                     OrderType.LIMIT,
                     quantity,
                     datetime.now(timezone.utc),
                     price,
                     time_in_force,
                     expire_time)

    @staticmethod
    def stop(
            symbol: Symbol,
            order_id: OrderId,
            label: Label,
            order_side: OrderSide,
            int quantity,
            price: Decimal,
            time_in_force=None,
            expire_time=None) -> Order:
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
        :raises ValueError: If the order_id is not a valid string.
        :raises ValueError: If the label is not a valid string.
        :raises ValueError: If the quantity is not positive (> 0).
        :raises ValueError: If the price is not positive (> 0).
        :raises ValueError: If the time_in_force is GTD and the expire_time is None.
        """
        # Preconditions checked inside Order.

        return Order(symbol,
                     order_id,
                     label,
                     order_side,
                     OrderType.STOP_MARKET,
                     quantity,
                     datetime.now(timezone.utc),
                     price,
                     time_in_force,
                     expire_time)

    @staticmethod
    def stop_limit(
            symbol: Symbol,
            order_id: OrderId,
            label: Label,
            order_side: OrderSide,
            int quantity,
            price: Decimal,
            time_in_force=None,
            expire_time=None) -> Order:
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
        :raises ValueError: If the order_id is not a valid string.
        :raises ValueError: If the label is not a valid string.
        :raises ValueError: If the quantity is not positive (> 0).
        :raises ValueError: If the price is not positive (> 0).
        :raises ValueError: If the time_in_force is GTD and the expire_time is None.
        """
        # Preconditions checked inside Order.

        return Order(symbol,
                     order_id,
                     label,
                     order_side,
                     OrderType.STOP_LIMIT,
                     quantity,
                     datetime.now(timezone.utc),
                     price,
                     time_in_force,
                     expire_time)

    @staticmethod
    def market_if_touched(
            symbol: Symbol,
            order_id: OrderId,
            label: Label,
            order_side: OrderSide,
            int quantity,
            price: Decimal,
            time_in_force=None,
            expire_time=None) -> Order:
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
        :raises ValueError: If the order_id is not a valid string.
        :raises ValueError: If the label is not a valid string.
        :raises ValueError: If the quantity is not positive (> 0).
        :raises ValueError: If the price is not positive (> 0).
        :raises ValueError: If the time_in_force is GTD and the expire_time is None.
        """
        # Preconditions checked inside Order.

        return Order(symbol,
                     order_id,
                     label,
                     order_side,
                     OrderType.MIT,
                     quantity,
                     datetime.now(timezone.utc),
                     price,
                     time_in_force,
                     expire_time)

    @staticmethod
    def fill_or_kill(
            symbol: Symbol,
            order_id: OrderId,
            label: Label,
            order_side: OrderSide,
            int quantity) -> Order:
        """
        Creates and returns a new fill-or-kill order with the given parameters.

        :param symbol: The orders symbol.
        :param order_id: The orders identifier (must be unique).
        :param label: The orders label.
        :param order_side: The orders side.
        :param quantity: The orders quantity (> 0).
        :return: The fill or kill order.
        :raises ValueError: If the order_id is not a valid string.
        :raises ValueError: If the label is not a valid string.
        :raises ValueError: If the quantity is not positive (> 0).
        """
        # Preconditions checked inside Order.

        return Order(symbol,
                     order_id,
                     label,
                     order_side,
                     OrderType.MARKET,
                     quantity,
                     datetime.now(timezone.utc),
                     price=None,
                     time_in_force=TimeInForce.FOC,
                     expire_time=None)

    @staticmethod
    def immediate_or_cancel(
            symbol: Symbol,
            order_id: OrderId,
            label: Label,
            order_side: OrderSide,
            int quantity) -> Order:
        """
        Creates and returns a new immediate-or-cancel order with the given parameters.

        :param symbol: The orders symbol.
        :param order_id: The orders identifier (must be unique).
        :param label: The orders label.
        :param order_side: The orders side.
        :param quantity: The orders quantity (> 0).
        :return: The immediate or cancel order.
        :raises ValueError: If the order_id is not a valid string.
        :raises ValueError: If the label is not a valid string.
        :raises ValueError: If the quantity is not positive (> 0).
        """
        # Preconditions checked inside Order.

        return Order(symbol,
                     order_id,
                     label,
                     order_side,
                     OrderType.MARKET,
                     quantity,
                     datetime.now(timezone.utc),
                     price=None,
                     time_in_force=TimeInForce.IOC,
                     expire_time=None)
