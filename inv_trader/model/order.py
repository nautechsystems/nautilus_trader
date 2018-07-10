#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="order.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import datetime

from decimal import Decimal
from typing import List

from inv_trader.model.enums import OrderSide, OrderType, TimeInForce, OrderStatus
from inv_trader.model.objects import Symbol
from inv_trader.model.events import OrderEvent
from inv_trader.model.events import OrderSubmitted, OrderAccepted, OrderRejected, OrderWorking
from inv_trader.model.events import OrderExpired, OrderModified, OrderCancelled, OrderCancelReject
from inv_trader.model.events import OrderPartiallyFilled, OrderFilled

orders_requiring_prices = [OrderType.LIMIT, OrderType.STOP_MARKET, OrderType.STOP_LIMIT, OrderType.MIT]


class Order:
    """
    Represents an order in a financial market.
    """

    def __init__(self,
                 symbol: Symbol,
                 identifier: str,
                 label: str,
                 order_side: OrderSide,
                 order_type: OrderType,
                 quantity: int,
                 timestamp: datetime.datetime,
                 price: Decimal=None,
                 time_in_force: TimeInForce=None,
                 expire_time: datetime.datetime=None):
        """
        Initializes a new instance of the Order class.

        :param: symbol: The orders symbol.
        :param: identifier: The orders identifier (id).
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
        if symbol is None:
            raise ValueError("The symbol cannot be None.")
        if not isinstance(symbol, Symbol):
            raise TypeError(f"The symbol must be of type Symbol (was {type(symbol)}).")
        if identifier is None:
            raise ValueError("The identifier cannot be None.")
        if not isinstance(identifier, str):
            raise TypeError(f"The identifier must be of type str (was {type(identifier)}).")
        if label is None:
            raise ValueError("The label cannot be None.")
        if not isinstance(label, str):
            raise TypeError(f"The label must be of type str (was {type(label)}).")
        if quantity <= 0:
            raise ValueError(f"The quantity must be positive (was {quantity}).")
        if not isinstance(quantity, int):
            raise TypeError(f"The quantity must be of type int (was {type(quantity)}).")
        if timestamp is None:
            raise ValueError("The timestamp cannot be None.")
        if not isinstance(timestamp, datetime.datetime):
            raise TypeError(f"The timestamp must be of type datetime (was {type(timestamp)}).")
        if time_in_force is TimeInForce.GTD and expire_time is None:
            raise ValueError(f"The expire_time cannot be None for GTD orders.")
        if order_type in orders_requiring_prices and price is None:
            raise ValueError("The price cannot be None.")
        if order_type in orders_requiring_prices and not isinstance(price, Decimal):
            raise TypeError(f"The price must be of type decimal (was {type(price)}).")
        if order_type not in orders_requiring_prices and price is not None:
            raise ValueError(f"{order_type.name} orders cannot have a price.")

        self._symbol = symbol
        self._id = identifier
        self._label = label
        self._order_side = order_side
        self._order_type = order_type
        self._quantity = quantity
        self._timestamp = timestamp
        self._time_in_force = time_in_force  # Can be None
        self._expire_time = expire_time      # Can be None
        self._price = price                  # Can be None
        self._filled_quantity = 0
        self._average_price = Decimal('0')
        self._slippage = Decimal('0')
        self._order_status = OrderStatus.INITIALIZED
        self._order_events = []         # type: List[OrderEvent]
        self._order_ids = []            # type: List[str]
        self._order_broker_ids = []     # type: List[str]
        self._order_execution_ids = []  # type: List[str]
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
        :return: The orders id.
        """
        return self._id

    @property
    def broker_id(self) -> str:
        """
        :return: The orders broker-side order id.
        """
        return self._order_broker_ids[-1]

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
        return self._order_side

    @property
    def type(self) -> OrderType:
        """
        :return: The orders type.
        """
        return self._order_type

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
    def timestamp(self) -> datetime.datetime:
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
    def expire_time(self) -> datetime.datetime:
        """
        :return: The orders expire time (optional could be None).
        """
        return self._expire_time

    @property
    def price(self) -> Decimal:
        """
        :return: The orders price (optional could be None).
        """
        return self._price

    @property
    def average_price(self) -> Decimal:
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
        return self._order_status

    @property
    def is_complete(self) -> bool:
        """
        :return: A value indicating whether the order is complete.
        """
        return (self._order_status is OrderStatus.CANCELLED
                or self._order_status is OrderStatus.EXPIRED
                or self._order_status is OrderStatus.FILLED
                or self._order_status is OrderStatus.REJECTED)

    @property
    def event_count(self) -> int:
        """
        :return: The count of events since the order was initialized.
        """
        return len(self._order_events)

    @property
    def events(self) -> List[OrderEvent]:
        """
        :return: The orders internal events list.
        """
        return self._order_events

    def __eq__(self, other) -> bool:
        """
        Override the default equality comparison.
        """
        if isinstance(other, self.__class__):
            return self.__dict__ == other.__dict__
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
        return f"Order: {self._id}"

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the order.
        """
        return f"<{str(self)} object at {id(self)}>"

    def apply(self, order_event: OrderEvent):
        """
        Applies the given order event to the order.

        :param order_event: The order event to apply.
        """
        # Preconditions
        if not isinstance(order_event, OrderEvent):
            raise TypeError(f"Event must be of type OrderEvent (was {type(order_event)}")
        if order_event.order_id is not self.id:
            raise ValueError(f"Incorrect order id for this event (was {order_event.order_id}).")

        self._order_events.append(order_event)

        # Handle event
        if isinstance(order_event, OrderSubmitted):
            self._order_status = OrderStatus.SUBMITTED

        elif isinstance(order_event, OrderAccepted):
            self._order_status = OrderStatus.ACCEPTED

        elif isinstance(order_event, OrderRejected):
            self._order_status = OrderStatus.REJECTED

        elif isinstance(order_event, OrderWorking):
            self._order_status = OrderStatus.WORKING
            self._order_broker_ids.append(order_event.broker_order_id)

        elif isinstance(order_event, OrderCancelled):
            self._order_status = OrderStatus.CANCELLED

        elif isinstance(order_event, OrderCancelReject):
            pass

        elif isinstance(order_event, OrderExpired):
            self._order_status = OrderStatus.EXPIRED

        elif isinstance(order_event, OrderModified):
            self._order_broker_ids.append(order_event.broker_order_id)
            self._price = order_event.modified_price

        elif isinstance(order_event, OrderFilled):
            self._order_status = OrderStatus.FILLED
            self._order_execution_ids.append(order_event.execution_id)
            self._execution_tickets.append(order_event.execution_ticket)
            self._filled_quantity = order_event.filled_quantity
            self._average_price = order_event.average_price
            self._set_slippage()
            self._check_overfill()

        elif isinstance(order_event, OrderPartiallyFilled):
            self._order_status = OrderStatus.PARTIALLY_FILLED
            self._order_execution_ids.append(order_event.execution_id)
            self._execution_tickets.append(order_event.execution_ticket)
            self._filled_quantity = order_event.filled_quantity
            self._average_price = order_event.average_price
            self._set_slippage()
            self._check_overfill()

    def _set_slippage(self):
        if self._order_type not in orders_requiring_prices:
            # Slippage not applicable to orders with entry prices.
            return

        if self.side is OrderSide.BUY:
            self._slippage = self._average_price - self._price
        else:  # side is OrderSide.SELL:
            self._slippage = (self._price - self._average_price)

    def _check_overfill(self):
        if self._filled_quantity > self._quantity:
            self._order_status = OrderStatus.OVER_FILLED
