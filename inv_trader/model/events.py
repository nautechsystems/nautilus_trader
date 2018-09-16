#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="events.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import abc
import uuid

from datetime import datetime
from decimal import Decimal
from uuid import UUID
from typing import Optional

from inv_trader.core.precondition import Precondition
from inv_trader.model.enums import CurrencyCode, OrderSide, OrderType, TimeInForce, Broker
from inv_trader.model.objects import Symbol


class Event:
    """
    The abstract base class for all events.
    """

    __metaclass__ = abc.ABCMeta

    def __init__(self,
                 identifier: uuid,
                 timestamp: datetime):
        """
        Initializes a new instance of the Event abstract class.

        :param: identifier: The events identifier.
        :param: uuid: The events timestamp.
        """
        self._event_id = identifier
        self._event_timestamp = timestamp

    @property
    def id(self) -> uuid:
        """
        :return: The events identifier.
        """
        return self._event_id

    @property
    def timestamp(self) -> datetime:
        """
        :return: The events timestamp (the time the event was created).
        """
        return self._event_timestamp

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

    def __hash__(self):
        """"
        Override the default hash implementation.
        """
        return hash(self.id)

    def __str__(self) -> str:
        """
        :return: The str() string representation of the event.
        """
        return f"{self.__class__.__name__}()"

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the event.
        """
        return f"<{str(self)} object at {id(self)}>"


class AccountEvent(Event):
    """
    Represents an account event produced from a collateral report.
    """

    def __init__(self,
                 account_id: str,
                 broker: Broker,
                 account_number: str,
                 currency: CurrencyCode,
                 cash_balance: Decimal,
                 cash_start_day: Decimal,
                 cash_activity_day: Decimal,
                 margin_used_liquidation: Decimal,
                 margin_used_maintenance: Decimal,
                 margin_ratio: Decimal,
                 margin_call_status: str,
                 event_id: UUID,
                 event_timestamp: datetime):
        """
        Initializes a new instance of the Bar class.

        :param currency: The currency for the account.
        :param cash_balance: The events account cash balance.
        :param cash_start_day: The events account cash start of day.
        :param cash_activity_day: The events account activity for the trading day.
        :param margin_used_liquidation: The events margin used before liquidation.
        :param margin_used_maintenance: The events margin used for maintenance.
        :param margin_ratio: The events account margin ratio.
        :param margin_ratio: The events margin call status.
        :param: event_id: The events identifier.
        :param: event_timestamp: The order events timestamp.
        """
        Precondition.valid_string(account_id, 'account_id')
        Precondition.valid_string(account_number, 'account_number')
        Precondition.not_negative(cash_balance, 'cash_balance')
        Precondition.not_negative(cash_start_day, 'cash_start_day')
        Precondition.not_negative(cash_activity_day, 'cash_activity_day')
        Precondition.not_negative(margin_used_liquidation, 'margin_used_liquidation')
        Precondition.not_negative(margin_used_maintenance, 'margin_used_maintenance')
        Precondition.not_negative(margin_ratio, 'margin_ratio')
        # Precondition.valid_string(margin_call_status, 'margin_call_status')

        super().__init__(event_id, event_timestamp)
        self._currency = currency
        self._cash_balance = cash_balance
        self._cash_start_day = cash_start_day
        self._cash_activity_day = cash_activity_day
        self._margin_used_liquidation = margin_used_liquidation
        self._margin_used_maintenance = margin_used_maintenance
        self._margin_ratio = margin_ratio
        self._margin_call_status = margin_call_status

    @property
    def cash_balance(self) -> Decimal:
        """
        :return: The events account cash balance.
        """
        return self._cash_balance

    @property
    def cash_start_day(self) -> Decimal:
        """
        :return: The events account balance at the start of the trading day.
        """
        return self._cash_start_day

    @property
    def cash_activity_day(self) -> Decimal:
        """
        :return: The events account activity for the day.
        """
        return self._cash_start_day

    @property
    def margin_used_liquidation(self) -> Decimal:
        """
        :return: The events account liquidation margin used.
        """
        return self._margin_used_liquidation

    @property
    def margin_used_maintenance(self) -> Decimal:
        """
        :return: The events account maintenance margin used.
        """
        return self._margin_used_maintenance

    @property
    def margin_ratio(self) -> Decimal:
        """
        :return: The events account margin ratio.
        """
        return self._margin_ratio

    @property
    def margin_call_status(self) -> str:
        """
        :return: The events account margin call status.
        """
        return self._margin_call_status

    def __str__(self) -> str:
        """
        :return: The str() string representation of the event.
        """
        return (f"{self.__class__.__name__}"
                f"(order_id={self._cash_balance}, margin_used={self._margin_used_maintenance})")

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the event.
        """
        return f"<{str(self)} object at {id(self)}>"


class OrderEvent(Event):
    """
    The abstract base class for all order events.
    """

    __metaclass__ = abc.ABCMeta

    def __init__(self,
                 order_symbol: Symbol,
                 order_id: str,
                 event_id: UUID,
                 event_timestamp: datetime):
        """
        Initializes a new instance of the OrderEvent abstract class.

        :param: order_symbol: The events order symbol.
        :param: order_id: The events order identifier.
        :param: event_id: The events identifier.
        :param: event_timestamp: The order events timestamp.
        """
        Precondition.valid_string(order_id, 'order_id')

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

    def __str__(self) -> str:
        """
        :return: The str() string representation of the event.
        """
        return (f"{self.__class__.__name__}"
                f"(id={self._order_id}, symbol={self._symbol})")

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the event.
        """
        return f"<{str(self)} object at {id(self)}>"


class OrderSubmitted(OrderEvent):
    """
    Represents an event where an order has been submitted to the execution system.
    """

    def __init__(self,
                 symbol: Symbol,
                 order_id: str,
                 submitted_time: datetime,
                 event_id: UUID,
                 event_timestamp: datetime):
        """
        Initializes a new instance of the OrderSubmitted class.

        :param: symbol: The events order symbol.
        :param: order_id: The events order identifier.
        :param: submitted_time: The events order submitted time.
        :param: event_id: The events identifier.
        :param: event_timestamp: The events timestamp.
        """
        Precondition.valid_string(order_id, 'order_id')

        super().__init__(symbol, order_id, event_id, event_timestamp)
        self._submitted_time = submitted_time

    @property
    def submitted_time(self) -> datetime:
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
                 accepted_time: datetime,
                 event_id: UUID,
                 event_timestamp: datetime):
        """
        Initializes a new instance of the OrderAccepted class.

        :param: symbol: The events order symbol.
        :param: order_id: The events order identifier.
        :param: accepted_time: The events order accepted time.
        :param: event_id: The events identifier.
        :param: event_timestamp: The events timestamp.
        """
        Precondition.valid_string(order_id, 'order_id')

        super().__init__(symbol, order_id, event_id, event_timestamp)
        self._accepted_time = accepted_time

    @property
    def accepted_time(self) -> datetime:
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
                 rejected_time: datetime,
                 rejected_reason: str,
                 event_id: UUID,
                 event_timestamp: datetime):
        """
        Initializes a new instance of the OrderRejected class.

        :param: symbol: The events order symbol.
        :param: order_id: The events order identifier.
        :param: rejected_time: The events order rejected time.
        :param: rejected_reason: The events order rejected reason.
        :param: event_id: The events identifier.
        :param: event_timestamp: The events timestamp.
        """
        Precondition.valid_string(order_id, 'order_id')
        Precondition.valid_string(rejected_reason, 'rejected_reason')

        super().__init__(symbol, order_id, event_id, event_timestamp)
        self._rejected_time = rejected_time
        self._rejected_reason = rejected_reason

    @property
    def rejected_time(self) -> datetime:
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
                 label: str,
                 order_side: OrderSide,
                 order_type: OrderType,
                 quantity: int,
                 price: Decimal,
                 time_in_force: TimeInForce,
                 working_time: datetime,
                 event_id: UUID,
                 event_timestamp: datetime,
                 expire_time: datetime=None):
        """
        Initializes a new instance of the OrderWorking class.

        :param: symbol: The events order symbol.
        :param: order_id: The events order identifier.
        :param: broker_order_id: The events broker order identifier.
        :param: label: The events order label.
        :param: order_side: The events order side.
        :param: order_type: The events order type.
        :param: quantity: The events order quantity.
        :param: price: The events order price.
        :param: time_in_force: The events order time in force.
        :param: working_time: The events order working time.
        :param: event_id: The events identifier.
        :param: event_timestamp: The events timestamp.
        :param: expire_time: The events order expire time (optional can be None).
        """
        Precondition.valid_string(order_id, 'order_id')
        Precondition.valid_string(broker_order_id, 'broker_order_id')
        Precondition.valid_string(label, 'label')
        Precondition.positive(quantity, 'quantity')

        super().__init__(symbol, order_id, event_id, event_timestamp)
        self._broker_order_id = broker_order_id
        self._label = label
        self._order_side = order_side
        self._order_type = order_type
        self._quantity = quantity
        self._price = price
        self._time_in_force = time_in_force
        self._working_time = working_time
        self._expire_time = expire_time

    @property
    def broker_order_id(self) -> str:
        """
        :return: The events broker order identifier.
        """
        return self._broker_order_id

    @property
    def label(self) -> str:
        """
        :return: The events order label.
        """
        return self._label

    @property
    def order_side(self) -> OrderSide:
        """
        :return: The events order side.
        """
        return self._order_side

    @property
    def order_type(self) -> OrderType:
        """
        :return: The events order type.
        """
        return self._order_type

    @property
    def quantity(self) -> int:
        """
        :return: The events order quantity.
        """
        return self._quantity

    @property
    def price(self) -> Decimal:
        """
        :return: The events order price.
        """
        return self._price

    @property
    def time_in_force(self) -> TimeInForce:
        """
        :return: The events order time in force.
        """
        return self._time_in_force

    @property
    def working_time(self) -> datetime:
        """
        :return: The events order working time.
        """
        return self._working_time

    @property
    def expire_time(self) -> Optional[datetime]:
        """
        :return: The events order expire time (optional could be None).
        """
        return self._expire_time


class OrderCancelled(OrderEvent):
    """
    Represents an event where an order has been cancelled with the broker.
    """

    def __init__(self,
                 symbol: Symbol,
                 order_id: str,
                 cancelled_time: datetime,
                 event_id: UUID,
                 event_timestamp: datetime):
        """
        Initializes a new instance of the OrderCancelled class.

        :param: symbol: The events order symbol.
        :param: order_id: The events order identifier.
        :param: cancelled_time: The events order cancelled time.
        :param: event_id: The events identifier.
        :param: event_timestamp: The events timestamp.
        """
        Precondition.valid_string(order_id, 'order_id')

        super().__init__(symbol, order_id, event_id, event_timestamp)
        self._cancelled_time = cancelled_time

    @property
    def cancelled_time(self) -> datetime:
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
                 cancel_reject_time: datetime,
                 cancel_response: str,
                 cancel_reject_reason: str,
                 event_id: UUID,
                 event_timestamp: datetime):
        """
        Initializes a new instance of the OrderCancelReject class.

        :param: order_symbol: The events order symbol.
        :param: order_id: The events order identifier.
        :param: cancel_reject_time: The events order cancel reject time.
        :param cancel_response: The events order cancel reject response.
        :param cancel_reject_reason: The events order cancel reject reason.
        :param: event_id: The events identifier.
        :param: event_timestamp: The events timestamp.
        """
        Precondition.valid_string(order_id, 'order_id')
        Precondition.valid_string(cancel_response, 'cancel_response')
        Precondition.valid_string(cancel_reject_reason, 'cancel_reject_reason')

        super().__init__(order_symbol, order_id, event_id, event_timestamp)
        self._cancel_reject_time = cancel_reject_time
        self._cancel_reject_response = cancel_response
        self._cancel_reject_reason = cancel_reject_reason

    @property
    def cancel_reject_time(self) -> datetime:
        """
        :return: The events order cancel reject time.
        """
        return self._cancel_reject_time

    @property
    def cancel_reject_response(self) -> str:
        """
        :return: The events order cancel reject response to.
        """
        return self._cancel_reject_response

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
                 expired_time: datetime,
                 event_id: UUID,
                 event_timestamp: datetime):
        """
        Initializes a new instance of the OrderExpired class.

        :param: symbol: The events order symbol.
        :param: order_id: The events order identifier.
        :param: expired_time: The events order expired time.
        :param: event_id: The events identifier.
        :param: event_timestamp: The events timestamp.
        """
        Precondition.valid_string(order_id, 'order_id')

        super().__init__(symbol, order_id, event_id, event_timestamp)
        self._expired_time = expired_time

    @property
    def expired_time(self) -> datetime:
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
                 modified_time: datetime,
                 event_id: UUID,
                 event_timestamp: datetime):
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
        Precondition.valid_string(order_id, 'order_id')
        Precondition.valid_string(broker_order_id, 'broker_order_id')

        super().__init__(symbol, order_id, event_id, event_timestamp)
        self._broker_order_id = broker_order_id
        self._modified_price = modified_price
        self._modified_time = modified_time

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
    def modified_time(self) -> datetime:
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
                 execution_time: datetime,
                 event_id: UUID,
                 event_timestamp: datetime):
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
        Precondition.valid_string(order_id, 'order_id')
        Precondition.valid_string(execution_id, 'execution_id')
        Precondition.valid_string(execution_ticket, 'execution_ticket')
        Precondition.positive(filled_quantity, 'filled_quantity')

        super().__init__(symbol, order_id, event_id, event_timestamp)
        self._execution_id = execution_id
        self._execution_ticket = execution_ticket
        self._order_side = order_side
        self._filled_quantity = filled_quantity
        self._average_price = average_price
        self._execution_time = execution_time

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
    def execution_time(self) -> datetime:
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
                 execution_time: datetime,
                 event_id: UUID,
                 event_timestamp: datetime):
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
        Precondition.valid_string(order_id, 'order_id')
        Precondition.valid_string(execution_id, 'execution_id')
        Precondition.valid_string(execution_ticket, 'execution_ticket')
        Precondition.positive(filled_quantity, 'filled_quantity')
        Precondition.positive(leaves_quantity, 'leaves_quantity')

        super().__init__(symbol, order_id, event_id, event_timestamp)
        self._execution_id = execution_id
        self._execution_ticket = execution_ticket
        self._order_side = order_side
        self._filled_quantity = filled_quantity
        self._leaves_quantity = leaves_quantity
        self._average_price = average_price
        self._execution_time = execution_time

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
    def execution_time(self) -> datetime:
        """
        :return: The events execution time.
        """
        return self._execution_time


class TimeEvent(Event):
    """
    Represents a time event occurring at the event timestamp.
    """

    def __init__(self,
                 label: str,
                 event_id: UUID,
                 event_timestamp: datetime):
        """
        Initializes a new instance of the TimeEvent class.

        :param: event_id: The time events identifier.
        :param: event_timestamp: The time events timestamp.
        """
        Precondition.valid_string(label, 'label')

        super().__init__(event_id, event_timestamp)
        self._label = label

    @property
    def label(self) -> str:
        """
        :return: The time events label.
        """
        return self._label

    def __str__(self) -> str:
        """
        :return: The str() string representation of the event.
        """
        return f"{self.__class__.__name__}(label={self._label}, timestamp={self._event_timestamp})"

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the event.
        """
        return f"<{str(self)} object at {id(self)}>"
