#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="order.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

from datetime import datetime
from decimal import Decimal
from typing import List, Optional

from inv_trader.core.checks import typechecking
from inv_trader.model.enums import OrderSide, OrderType, TimeInForce, OrderStatus
from inv_trader.model.objects import Symbol
from inv_trader.model.events import OrderEvent
from inv_trader.model.events import OrderSubmitted, OrderAccepted, OrderRejected, OrderWorking
from inv_trader.model.events import OrderExpired, OrderModified, OrderCancelled, OrderCancelReject
from inv_trader.model.events import OrderPartiallyFilled, OrderFilled

# Order types which require prices to be valid.
PRICED_ORDER_TYPES = [
    OrderType.LIMIT,
    OrderType.STOP_MARKET,
    OrderType.STOP_LIMIT,
    OrderType.MIT]


class Order:
    """
    Represents an order in a financial market.
    """

    @typechecking
    def __init__(self,
                 symbol: Symbol,
                 order_id: str,
                 label: str,
                 order_side: OrderSide,
                 order_type: OrderType,
                 quantity: int,
                 timestamp: datetime,
                 price: Optional[Decimal]=None,
                 time_in_force: Optional[TimeInForce]=None,
                 expire_time: Optional[datetime]=None):
        """
        Initializes a new instance of the Order class.

        :param: symbol: The orders symbol.
        :param: order_id: The orders identifier.
        :param: label: The orders label.
        :param: order_side: The orders side.
        :param: order_type: The orders type.
        :param: quantity: The orders quantity (> 0).
        :param: timestamp: The orders initialization timestamp.
        :param: price: The orders price (can be None for market orders > 0).
        :param: time_in_force: The orders time in force (optional can be None).
        :param: expire_time: The orders expire time (optional can be None).
        """
        # Preconditions
        if time_in_force is None:
            time_in_force = TimeInForce.DAY
        if quantity <= 0:
            raise ValueError(f"The quantity must be positive (was {quantity}).")
        if time_in_force is TimeInForce.GTD and expire_time is None:
            raise ValueError(f"The expire_time cannot be None for GTD orders.")
        if order_type in PRICED_ORDER_TYPES and price is None:
            raise ValueError("The price cannot be None.")
        if order_type in PRICED_ORDER_TYPES and not isinstance(price, Decimal):
            raise TypeError(f"The price must be of type decimal (was {type(price)}).")
        if order_type not in PRICED_ORDER_TYPES and price is not None:
            raise ValueError(f"{order_type.name} orders cannot have a price.")

        self._symbol = symbol
        self._id = order_id
        self._label = label
        self._side = order_side
        self._type = order_type
        self._quantity = quantity
        self._timestamp = timestamp
        self._time_in_force = time_in_force  # Can be None
        self._expire_time = expire_time      # Can be None
        self._price = price                  # Can be None
        self._filled_quantity = 0
        self._average_price = Decimal('0')
        self._slippage = Decimal('0')
        self._status = OrderStatus.INITIALIZED
        self._events = []               # type: List[OrderEvent]
        self._order_ids = [order_id]    # type: List[str]
        self._order_ids_broker = []     # type: List[str]
        self._execution_ids = []        # type: List[str]
        self._execution_tickets = []    # type: List[str]

    @property
    def symbol(self) -> Symbol:
        """
        :return: The orders symbol.
        """
        return self._symbol

    @property
    def id(self) -> str:
        """
        :return: The orders identifier.
        """
        return self._id

    @property
    def id_current(self) -> str:
        """
        :return: The orders current identifier.
        """
        return self._order_ids[-1]

    @property
    def broker_id(self) -> str:
        """
        :return: The orders broker-side order identifier (could be an empty string).
        """
        if len(self._order_ids_broker) == 0:
            return ''
        return self._order_ids_broker[-1]

    @property
    def execution_id(self) -> str:
        """
        :return: The orders last execution (could be an empty string).
        """
        if len(self._execution_ids) == 0:
            return ''
        return self._execution_ids[-1]

    @property
    def execution_ticket(self) -> str:
        """
        :return: The orders last execution ticket (could be an empty string).
        """
        if len(self._execution_tickets) == 0:
            return ''
        return self._execution_tickets[-1]

    @property
    def label(self) -> str:
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
        :return: The orders time in force (optional could be None).
        """
        return self._time_in_force

    @property
    def expire_time(self) -> Optional[datetime]:
        """
        :return: The orders expire time (optional could be None).
        """
        return self._expire_time

    @property
    def price(self) -> Optional[Decimal]:
        """
        :return: The orders price (optional could be None).
        """
        return self._price

    @property
    def average_price(self) -> Optional[Decimal]:
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

    def __eq__(self, other) -> bool:
        """
        Override the default equality comparison.
        """
        if isinstance(other, self.__class__):
            return self.id == other.id
        else:
            return False

    def __ne__(self, other):
        """
        Override the default not-equals comparison.
        """
        return not self.__eq__(other)

    def __str__(self) -> str:
        """
        :return: The str() string representation of the order.
        """
        attrs = vars(self)
        props = ', '.join("%s=%s" % item for item in attrs.items()).replace(', _', ', ')
        return f"{self.__class__.__name__}({props})"

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the order.
        """
        return f"<{str(self)} object at {id(self)}>"

    @typechecking
    def apply(self, order_event: OrderEvent):
        """
        Applies the given order event to the order.

        :param order_event: The order event to apply.
        """
        # Preconditions
        if order_event.order_id != self.id:
            raise ValueError(
                f"The event order id is invalid for this order "
                f"(event order id was {order_event.order_id},"
                f"this order id was {self.id}).")

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
