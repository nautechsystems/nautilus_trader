#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="events.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import abc
import datetime
import uuid

from decimal import Decimal

from inv_trader.model.enums import OrderSide
from inv_trader.model.objects import Symbol


class Event:
    """
    The base class for all events.
    """

    __metaclass__ = abc.ABCMeta

    def __init__(self,
                 identifier: uuid,
                 timestamp: datetime.datetime):
        """
        Initializes a new instance of the Event abstract class.

        :param: identifier: The events identifier.
        :param: uuid: The events timestamp.
        """
        self._id = identifier
        self._timestamp = timestamp

    @property
    def event_id(self) -> uuid:
        """
        :return: The events identifier.
        """
        return self._id

    @property
    def event_timestamp(self) -> datetime.datetime:
        """
        :return: The events timestamp (the time the event was created).
        """
        return self._timestamp

    def __eq__(self, other) -> bool:
        """
        Override the default equality comparison.
        """
        if isinstance(other, self.__class__):
            return self.event_id == other.event_id
        else:
            return False

    def __ne__(self, other):
        """
        Override the default not-equals comparison.
        """
        return not self.__eq__(other)

    def __str__(self) -> str:
        """
        :return: The str() string representation of the tick.
        """
        return (f"{self.__class__.__name__}: "
                f"event_id={self.event_id},"
                f"timestamp={self.event_timestamp.isoformat()}")

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the tick.
        """
        return f"<{str(self)} object at {id(self)}>"


class OrderEvent(Event):
    """
    The base class for all order events.
    """

    __metaclass__ = abc.ABCMeta

    def __init__(self,
                 order_symbol: Symbol,
                 order_id: str,
                 event_id: uuid,
                 event_timestamp: datetime.datetime):
        """
        Initializes a new instance of the OrderEvent abstract class.

        :param: order_symbol: The order events symbol.
        :param: order_id: The order events order identifier.
        :param: event_id: The order events identifier.
        :param: event_timestamp: The order events timestamp.
        """
        super().__init__(event_id, event_timestamp)
        self._symbol = order_symbol
        self._order_id = order_id

    @property
    def symbol(self) -> Symbol:
        """
        :return: The events order symbol.
        """
        return self._symbol

    @property
    def order_id(self) -> str:
        """
        :return: The events order identifier.
        """
        return self._order_id


class OrderSubmitted(OrderEvent):
    """
    Represents an event where an order has been submitted to the execution system.
    """
    def __init__(self,
                 symbol: Symbol,
                 order_id: str,
                 submitted_time: datetime.datetime,
                 event_id: uuid,
                 event_timestamp: datetime.datetime):
        """
        Initializes a new instance of the OrderSubmitted class.

        :param: symbol: The events order symbol.
        :param: order_id: The events order identifier.
        :param: submitted_time: The events order submitted time.
        :param: event_id: The events identifier.
        :param: event_timestamp: The events timestamp.
        """
        super().__init__(symbol, order_id, event_id, event_timestamp)
        self._submitted_time = submitted_time
        self._event_id = event_id
        self._event_timestamp = event_timestamp

    @property
    def submitted_time(self) -> datetime.datetime:
        """
        :return: The events order submitted time.
        """
        return self._submitted_time


class OrderAccepted(OrderEvent):
    """
    Represents an event where an order has been accepted by the broker.
    """
    def __init__(self,
                 symbol: Symbol,
                 order_id: str,
                 accepted_time: datetime.datetime,
                 event_id: uuid,
                 event_timestamp: datetime.datetime):
        """
        Initializes a new instance of the OrderAccepted class.

        :param: symbol: The events order symbol.
        :param: order_id: The events order identifier.
        :param: accepted_time: The events order accepted time.
        :param: event_id: The events identifier.
        :param: event_timestamp: The events timestamp.
        """
        super().__init__(symbol, order_id, event_id, event_timestamp)
        self._accepted_time = accepted_time

    @property
    def accepted_time(self) -> datetime.datetime:
        """
        :return: The events order accepted time.
        """
        return self._accepted_time


class OrderRejected(OrderEvent):
    """
    Represents an event where an order has been rejected by the broker.
    """
    def __init__(self,
                 symbol: Symbol,
                 order_id: str,
                 rejected_time: datetime.datetime,
                 rejected_reason: str,
                 event_id: uuid,
                 event_timestamp: datetime.datetime):
        """
        Initializes a new instance of the OrderRejected class.

        :param: symbol: The events order symbol.
        :param: order_id: The events order identifier.
        :param: rejected_time: The events order rejected time.
        :param: rejected_reason: The events order rejected reason.
        :param: event_id: The events identifier.
        :param: event_timestamp: The events timestamp.
        """
        super().__init__(symbol, order_id, event_id, event_timestamp)
        self._rejected_time = rejected_time
        self._rejected_reason = rejected_reason
        self._event_id = event_id
        self._event_timestamp = event_timestamp

    @property
    def rejected_time(self) -> datetime.datetime:
        """
        :return: The events order rejected time.
        """
        return self._rejected_time

    @property
    def rejected_reason(self) -> str:
        """
        :return: The events order rejected reason.
        """
        return self._rejected_reason


class OrderWorking(OrderEvent):
    """
    Represents an event where an order is working with the broker.
    """
    def __init__(self,
                 symbol: Symbol,
                 order_id: str,
                 broker_order_id: str,
                 working_time: datetime.datetime,
                 event_id: uuid,
                 event_timestamp: datetime.datetime):
        """
        Initializes a new instance of the OrderWorking class.

        :param: symbol: The events order symbol.
        :param: order_id: The events order identifier.
        :param: broker_order_id: The events broker order identifier.
        :param: working_time: The events order working time.
        :param: event_id: The events identifier.
        :param: event_timestamp: The events timestamp.
        """
        super().__init__(symbol, order_id, event_id, event_timestamp)
        self._broker_order_id = broker_order_id
        self._working_time = working_time

    @property
    def broker_order_id(self) -> str:
        """
        :return: The events broker order identifier.
        """
        return self._broker_order_id

    @property
    def working_time(self) -> datetime.datetime:
        """
        :return: The events order working time.
        """
        return self._working_time


class OrderCancelled(OrderEvent):
    """
    Represents an event where an order has been cancelled with the broker.
    """
    def __init__(self,
                 symbol: Symbol,
                 order_id: str,
                 cancelled_time: datetime.datetime,
                 event_id: uuid,
                 event_timestamp: datetime.datetime):
        """
        Initializes a new instance of the OrderCancelled class.

        :param: symbol: The events order symbol.
        :param: order_id: The events order identifier.
        :param: cancelled_time: The events order cancelled time.
        :param: event_id: The events identifier.
        :param: event_timestamp: The events timestamp.
        """
        super().__init__(symbol, order_id, event_id, event_timestamp)
        self._cancelled_time = cancelled_time

    @property
    def cancelled_time(self) -> datetime.datetime:
        """
        :return: The events order cancelled time.
        """
        return self._cancelled_time


class OrderCancelReject(OrderEvent):
    """
    Represents an event where an order cancel request has been rejected by the broker.
    """
    def __init__(self,
                 order_symbol: Symbol,
                 order_id: str,
                 cancel_reject_time: datetime.datetime,
                 cancel_reject_reason: str,
                 event_id: uuid,
                 event_timestamp: datetime.datetime):
        """
        Initializes a new instance of the OrderCancelReject class.

        :param: order_symbol: The events order symbol.
        :param: order_id: The events order identifier.
        :param: cancel_reject_time: The events order cancel reject time.
        :param: event_id: The events identifier.
        :param: event_timestamp: The events timestamp.
        """
        super().__init__(order_symbol, order_id, event_id, event_timestamp)
        self._cancel_reject_time = cancel_reject_time
        self._cancel_reject_reason = cancel_reject_reason

    @property
    def cancel_reject_time(self) -> datetime.datetime:
        """
        :return: The events order cancel reject time.
        """
        return self._cancel_reject_time

    @property
    def cancel_reject_reason(self) -> str:
        """
        :return: The events order cancel reject reason.
        """
        return self._cancel_reject_reason


class OrderExpired(OrderEvent):
    """
    Represents an event where an order has expired with the broker.
    """
    def __init__(self,
                 symbol: Symbol,
                 order_id: str,
                 expired_time: datetime.datetime,
                 event_id: uuid,
                 event_timestamp: datetime.datetime):
        """
        Initializes a new instance of the OrderExpired class.

        :param: symbol: The events order symbol.
        :param: order_id: The events order identifier.
        :param: expired_time: The events order expired time.
        :param: event_id: The events identifier.
        :param: event_timestamp: The events timestamp.
        """
        super().__init__(symbol, order_id, event_id, event_timestamp)
        self._expired_time = expired_time

    @property
    def expired_time(self) -> datetime.datetime:
        """
        :return: The events order expired time.
        """
        return self._expired_time


class OrderModified(OrderEvent):
    """
    Represents an event where an order has been modified with the broker.
    """
    def __init__(self,
                 symbol: Symbol,
                 order_id: str,
                 broker_order_id: str,
                 modified_price: Decimal,
                 modified_time: datetime.datetime,
                 event_id: uuid,
                 event_timestamp: datetime.datetime):
        """
        Initializes a new instance of the OrderPartiallyFilled class.

        :param: symbol: The events order symbol.
        :param: order_id: The events order identifier.
        :param: broker_order_id: The events order broker identifier.
        :param: modified_price: The events modified price.
        :param: modified_time: The events modified time.
        :param: event_id: The events identifier.
        :param: event_timestamp: The events timestamp.
        """
        super().__init__(symbol, order_id, event_id, event_timestamp)
        self._broker_order_id = broker_order_id
        self._modified_price = modified_price
        self._modified_time = modified_time
        self._event_id = event_id
        self._event_timestamp = event_timestamp

    @property
    def broker_order_id(self) -> str:
        """
        :return: The events broker order identifier.
        """
        return self._broker_order_id

    @property
    def modified_price(self) -> Decimal:
        """
        :return: The events modified order price.
        """
        return self._modified_price

    @property
    def modified_time(self) -> datetime.datetime:
        """
        :return: The events order modified time.
        """
        return self._modified_time


class OrderFilled(OrderEvent):
    """
    Represents an event where an order has been completely filled with the broker.
    """
    def __init__(self,
                 symbol: Symbol,
                 order_id: str,
                 execution_id: str,
                 execution_ticket: str,
                 order_side: OrderSide,
                 filled_quantity: int,
                 average_price: Decimal,
                 execution_time: datetime.datetime,
                 event_id: uuid,
                 event_timestamp: datetime.datetime):
        """
        Initializes a new instance of the OrderFilled class.

        :param: symbol: The events order symbol.
        :param: order_id: The events order identifier.
        :param: execution_id: The events order execution identifier.
        :param: execution_ticket: The events order execution ticket.
        :param: order_side: The events execution order side.
        :param: filled_quantity: The events execution filled quantity.
        :param: average_price: The events execution average price.
        :param: execution_time: The events execution time.
        :param: event_id: The events identifier.
        :param: event_timestamp: The events timestamp.
        """
        super().__init__(symbol, order_id, event_id, event_timestamp)
        self._execution_id = execution_id
        self._execution_ticket = execution_ticket
        self._order_side = order_side
        self._filled_quantity = filled_quantity
        self._average_price = average_price
        self._execution_time = execution_time
        self._event_id = event_id
        self._event_timestamp = event_timestamp

    @property
    def execution_id(self) -> str:
        """
        :return: The events order execution identifier.
        """
        return self._execution_id

    @property
    def execution_ticket(self) -> str:
        """
        :return: The events order execution ticket.
        """
        return self._execution_ticket

    @property
    def order_side(self) -> OrderSide:
        """
        :return: The events execution order side.
        """
        return self._order_side

    @property
    def filled_quantity(self) -> int:
        """
        :return: The events execution filled quantity.
        """
        return self._filled_quantity

    @property
    def average_price(self) -> Decimal:
        """
        :return: The events execution average price.
        """
        return self._average_price

    @property
    def execution_time(self) -> datetime.datetime:
        """
        :return: The events execution time.
        """
        return self._execution_time


class OrderPartiallyFilled(OrderEvent):
    """
    Represents an event where an order has been partially filled with the broker.
    """
    def __init__(self,
                 symbol: Symbol,
                 order_id: str,
                 execution_id: str,
                 execution_ticket: str,
                 order_side: OrderSide,
                 filled_quantity: int,
                 leaves_quantity: int,
                 average_price: Decimal,
                 execution_time: datetime.datetime,
                 event_id: uuid,
                 event_timestamp: datetime.datetime):
        """
        Initializes a new instance of the OrderPartiallyFilled class.

        :param: symbol: The events order symbol.
        :param: order_id: The events order identifier.
        :param: execution_id: The events order execution identifier.
        :param: execution_ticket: The events order execution ticket.
        :param: order_side: The events execution order side.
        :param: filled_quantity: The events execution filled quantity.
        :param: leaves_quantity: The events leaves quantity.
        :param: average_price: The events execution average price.
        :param: execution_time: The events execution time.
        :param: event_id: The events identifier.
        :param: event_timestamp: The events timestamp.
        """
        super().__init__(symbol, order_id, event_id, event_timestamp)
        self._execution_id = execution_id
        self._execution_ticket = execution_ticket
        self._order_side = order_side
        self._filled_quantity = filled_quantity
        self._leaves_quantity = leaves_quantity
        self._average_price = average_price
        self._execution_time = execution_time
        self._event_id = event_id
        self._event_timestamp = event_timestamp

    @property
    def execution_id(self) -> str:
        """
        :return: The events order execution identifier.
        """
        return self._execution_id

    @property
    def execution_ticket(self) -> str:
        """
        :return: The events order execution ticket.
        """
        return self._execution_ticket

    @property
    def order_side(self) -> OrderSide:
        """
        :return: The events execution order side.
        """
        return self._order_side

    @property
    def filled_quantity(self) -> int:
        """
        :return: The events execution filled quantity.
        """
        return self._filled_quantity

    @property
    def leaves_quantity(self) -> int:
        """
        :return: The events execution leaves quantity.
        """
        return self._leaves_quantity

    @property
    def average_price(self) -> Decimal:
        """
        :return: The events execution average price.
        """
        return self._average_price

    @property
    def execution_time(self) -> datetime.datetime:
        """
        :return: The events execution time.
        """
        return self._execution_time


class AccountEvent(Event):
    """
    Represents an account event where there have been changes to the account.
    """

    def __init__(self,
                 event_id: uuid,
                 event_timestamp: datetime.datetime):
        """
        Initializes a new instance of the AccountEvent class.

        :param: event_id: The account events identifier.
        :param: event_timestamp: The account events timestamp.
        """
        super().__init__(event_id, event_timestamp)


class TimeEvent(Event):
    """
    Represents a time event occurring at the event timestamp.
    """

    def __init__(self,
                 label: str,
                 event_id: uuid,
                 event_timestamp: datetime.datetime):
        """
        Initializes a new instance of the TimeEvent class.

        :param: event_id: The time events identifier.
        :param: event_timestamp: The time events timestamp.
        """
        super().__init__(event_id, event_timestamp)
        self._label = label

    @property
    def label(self) -> str:
        """
        :return: The time events label.
        """
        return self._label
