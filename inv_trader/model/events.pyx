#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="events.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False

from datetime import datetime
from decimal import Decimal
from uuid import UUID
from typing import Optional

from inv_trader.core.precondition cimport Precondition
from inv_trader.model.enums import CurrencyCode, OrderSide, OrderType, TimeInForce, Broker
from inv_trader.model.identifiers import Label, AccountId, AccountNumber
from inv_trader.model.identifiers import OrderId, ExecutionId, ExecutionTicket
from inv_trader.model.objects import Symbol


cdef class Event:
    """
    The abstract base class for all events.
    """
    cdef readonly object id
    cdef readonly object timestamp

    def __init__(self,
                 identifier: UUID,
                 timestamp: datetime):
        """
        Initializes a new instance of the Event abstract class.

        :param identifier: The events identifier.
        :param timestamp: The events timestamp.
        """
        Precondition.type(identifier, UUID, 'identifier')
        Precondition.type(timestamp, datetime, 'timestamp')

        self.id = identifier
        self.timestamp = timestamp

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
        :return: The str() string representation of the event.
        """
        return f"{self.__class__.__name__}()"

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the event.
        """
        return f"<{str(self)} object at {id(self)}>"


cdef class AccountEvent(Event):
    """
    Represents an account event produced from a collateral report.
    """
    cdef readonly object account_id
    cdef readonly object broker
    cdef readonly object account_number
    cdef readonly object currency
    cdef readonly object cash_balance
    cdef readonly object cash_start_day
    cdef readonly object cash_activity_day
    cdef readonly object margin_used_liquidation
    cdef readonly object margin_used_maintenance
    cdef readonly object margin_ratio
    cdef readonly str margin_call_status

    def __init__(self,
                 account_id: AccountId,
                 broker: Broker,
                 account_number: AccountNumber,
                 currency: CurrencyCode,
                 cash_balance: Decimal,
                 cash_start_day: Decimal,
                 cash_activity_day: Decimal,
                 margin_used_liquidation: Decimal,
                 margin_used_maintenance: Decimal,
                 margin_ratio: Decimal,
                 str margin_call_status,
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
        :param margin_call_status: The events margin call status (can be empty).
        :param event_id: The events identifier.
        :param event_timestamp: The order events timestamp.
        """
        Precondition.type(account_id, AccountId, 'account_id')
        Precondition.type(broker, Broker, 'broker')
        Precondition.type(account_number, AccountNumber, 'account_number')
        Precondition.not_negative(cash_balance, 'cash_balance')
        Precondition.not_negative(cash_start_day, 'cash_start_day')
        Precondition.not_negative(cash_activity_day, 'cash_activity_day')
        Precondition.not_negative(margin_used_liquidation, 'margin_used_liquidation')
        Precondition.not_negative(margin_used_maintenance, 'margin_used_maintenance')
        Precondition.not_negative(margin_ratio, 'margin_ratio')

        super().__init__(event_id, event_timestamp)
        self.account_id = account_id
        self.broker = broker
        self.account_number = account_number
        self.currency = currency
        self.cash_balance = cash_balance
        self.cash_start_day = cash_start_day
        self.cash_activity_day = cash_activity_day
        self.margin_used_liquidation = margin_used_liquidation
        self.margin_used_maintenance = margin_used_maintenance
        self.margin_ratio = margin_ratio
        self.margin_call_status = margin_call_status

    def __str__(self) -> str:
        """
        :return: The str() string representation of the event.
        """
        return (f"{self.__class__.__name__}"
                f"(order_id={self.cash_balance}, margin_used={self.margin_used_maintenance})")

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the event.
        """
        return f"<{str(self)} object at {id(self)}>"


cdef class OrderEvent(Event):
    """
    The abstract base class for all order events.
    """
    cdef readonly object symbol
    cdef readonly object order_id

    def __init__(self,
                 order_symbol: Symbol,
                 order_id: OrderId,
                 event_id: UUID,
                 event_timestamp: datetime):
        """
        Initializes a new instance of the OrderEvent abstract class.

        :param order_symbol: The events order symbol.
        :param order_id: The events order identifier.
        :param event_id: The events identifier.
        :param event_timestamp: The order events timestamp.
        :raises ValueError: If the order_id is not a valid string.
        """
        super().__init__(event_id, event_timestamp)
        self.symbol = order_symbol
        self.order_id = order_id

    def __str__(self) -> str:
        """
        :return: The str() string representation of the event.
        """
        return f"{self.__class__.__name__}(id={self.order_id})"

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the event.
        """
        return f"<{str(self)} object at {id(self)}>"


cdef class OrderSubmitted(OrderEvent):
    """
    Represents an event where an order has been submitted to the execution system.
    """
    cdef readonly object submitted_time

    def __init__(self,
                 symbol: Symbol,
                 order_id: OrderId,
                 submitted_time: datetime,
                 event_id: UUID,
                 event_timestamp: datetime):
        """
        Initializes a new instance of the OrderSubmitted class.

        :param symbol: The events order symbol.
        :param order_id: The events order identifier.
        :param submitted_time: The events order submitted time.
        :param event_id: The events identifier.
        :param event_timestamp: The events timestamp.
        """
        super().__init__(symbol,
                         order_id,
                         event_id,
                         event_timestamp)
        self.submitted_time = submitted_time


cdef class OrderAccepted(OrderEvent):
    """
    Represents an event where an order has been accepted by the broker.
    """
    cdef readonly object accepted_time

    def __init__(self,
                 symbol: Symbol,
                 order_id: OrderId,
                 accepted_time: datetime,
                 event_id: UUID,
                 event_timestamp: datetime):
        """
        Initializes a new instance of the OrderAccepted class.

        :param symbol: The events order symbol.
        :param order_id: The events order identifier.
        :param accepted_time: The events order accepted time.
        :param event_id: The events identifier.
        :param event_timestamp: The events timestamp.
        """
        super().__init__(symbol,
                         order_id,
                         event_id,
                         event_timestamp)
        self.accepted_time = accepted_time


cdef class OrderRejected(OrderEvent):
    """
    Represents an event where an order has been rejected by the broker.
    """
    cdef readonly object rejected_time
    cdef readonly str rejected_reason

    def __init__(self,
                 symbol: Symbol,
                 order_id: OrderId,
                 rejected_time: datetime,
                 str rejected_reason,
                 event_id: UUID,
                 event_timestamp: datetime):
        """
        Initializes a new instance of the OrderRejected class.

        :param symbol: The events order symbol.
        :param order_id: The events order identifier.
        :param rejected_time: The events order rejected time.
        :param rejected_reason: The events order rejected reason.
        :param event_id: The events identifier.
        :param event_timestamp: The events timestamp.
        """
        Precondition.valid_string(rejected_reason, 'rejected_reason')

        super().__init__(symbol,
                         order_id,
                         event_id,
                         event_timestamp)
        self.rejected_time = rejected_time
        self.rejected_reason = rejected_reason


cdef class OrderWorking(OrderEvent):
    """
    Represents an event where an order is working with the broker.
    """
    cdef readonly object broker_order_id
    cdef readonly object label
    cdef readonly object order_side
    cdef readonly object order_type
    cdef readonly int quantity
    cdef readonly object price
    cdef readonly object time_in_force
    cdef readonly object working_time
    cdef readonly object expire_time

    def __init__(self,
                 symbol: Symbol,
                 order_id: OrderId,
                 broker_order_id: OrderId,
                 label: Label,
                 order_side: OrderSide,
                 order_type: OrderType,
                 int quantity,
                 price: Decimal,
                 time_in_force: TimeInForce,
                 working_time: datetime,
                 event_id: UUID,
                 event_timestamp: datetime,
                 expire_time: datetime=None):
        """
        Initializes a new instance of the OrderWorking class.

        :param symbol: The events order symbol.
        :param order_id: The events order identifier.
        :param broker_order_id: The events broker order identifier.
        :param label: The events order label.
        :param order_side: The events order side.
        :param order_type: The events order type.
        :param quantity: The events order quantity.
        :param price: The events order price.
        :param time_in_force: The events order time in force.
        :param working_time: The events order working time.
        :param event_id: The events identifier.
        :param event_timestamp: The events timestamp.
        :param expire_time: The events order expire time (optional can be None).
        """
        Precondition.positive(quantity, 'quantity')

        super().__init__(symbol, order_id, event_id, event_timestamp)
        self.broker_order_id = broker_order_id
        self.label = label
        self.order_side = order_side
        self.order_type = order_type
        self.quantity = quantity
        self.price = price
        self.time_in_force = time_in_force
        self.working_time = working_time
        self.expire_time = expire_time


cdef class OrderCancelled(OrderEvent):
    """
    Represents an event where an order has been cancelled with the broker.
    """
    cdef readonly object cancelled_time

    def __init__(self,
                 symbol: Symbol,
                 order_id: OrderId,
                 cancelled_time: datetime,
                 event_id: UUID,
                 event_timestamp: datetime):
        """
        Initializes a new instance of the OrderCancelled class.

        :param symbol: The events order symbol.
        :param order_id: The events order identifier.
        :param cancelled_time: The events order cancelled time.
        :param event_id: The events identifier.
        :param event_timestamp: The events timestamp.
        """
        super().__init__(symbol,
                         order_id,
                         event_id,
                         event_timestamp)
        self.cancelled_time = cancelled_time


cdef class OrderCancelReject(OrderEvent):
    """
    Represents an event where an order cancel request has been rejected by the broker.
    """
    cdef readonly object cancel_reject_time
    cdef readonly object cancel_reject_response
    cdef readonly str cancel_reject_reason

    def __init__(self,
                 order_symbol: Symbol,
                 order_id: OrderId,
                 cancel_reject_time: datetime,
                 str cancel_response,
                 str cancel_reject_reason,
                 event_id: UUID,
                 event_timestamp: datetime):
        """
        Initializes a new instance of the OrderCancelReject class.

        :param order_symbol: The events order symbol.
        :param order_id: The events order identifier.
        :param cancel_reject_time: The events order cancel reject time.
        :param cancel_response: The events order cancel reject response.
        :param cancel_reject_reason: The events order cancel reject reason.
        :param event_id: The events identifier.
        :param event_timestamp: The events timestamp.
        """
        Precondition.valid_string(cancel_response, 'cancel_response')
        Precondition.valid_string(cancel_reject_reason, 'cancel_reject_reason')

        super().__init__(order_symbol,
                         order_id,
                         event_id,
                         event_timestamp)
        self.cancel_reject_time = cancel_reject_time
        self.cancel_reject_response = cancel_response
        self.cancel_reject_reason = cancel_reject_reason

    def __str__(self) -> str:
        """
        :return: The str() string representation of the event.
        """
        return (f"{self.__class__.__name__}"
                f"(id={self.order_id}, reason={self.cancel_reject_reason})")


cdef class OrderExpired(OrderEvent):
    """
    Represents an event where an order has expired with the broker.
    """
    cdef readonly object expired_time

    def __init__(self,
                 symbol: Symbol,
                 order_id: OrderId,
                 expired_time: datetime,
                 event_id: UUID,
                 event_timestamp: datetime):
        """
        Initializes a new instance of the OrderExpired class.

        :param symbol: The events order symbol.
        :param order_id: The events order identifier.
        :param expired_time: The events order expired time.
        :param event_id: The events identifier.
        :param event_timestamp: The events timestamp.
        """
        super().__init__(symbol,
                         order_id,
                         event_id,
                         event_timestamp)
        self.expired_time = expired_time


cdef class OrderModified(OrderEvent):
    """
    Represents an event where an order has been modified with the broker.
    """
    cdef readonly object broker_order_id
    cdef readonly object modified_price
    cdef readonly object modified_time

    def __init__(self,
                 symbol: Symbol,
                 order_id: OrderId,
                 broker_order_id: OrderId,
                 modified_price: Decimal,
                 modified_time: datetime,
                 event_id: UUID,
                 event_timestamp: datetime):
        """
        Initializes a new instance of the OrderPartiallyFilled class.

        :param symbol: The events order symbol.
        :param order_id: The events order identifier.
        :param broker_order_id: The events order broker identifier.
        :param modified_price: The events modified price.
        :param modified_time: The events modified time.
        :param event_id: The events identifier.
        :param event_timestamp: The events timestamp.
        """
        super().__init__(symbol,
                         order_id,
                         event_id,
                         event_timestamp)
        self.broker_order_id = broker_order_id
        self.modified_price = modified_price
        self.modified_time = modified_time


cdef class OrderFilled(OrderEvent):
    """
    Represents an event where an order has been completely filled with the broker.
    """
    cdef readonly object execution_id
    cdef readonly object execution_ticket
    cdef readonly object order_side
    cdef readonly object filled_quantity
    cdef readonly object average_price
    cdef readonly object execution_time

    def __init__(self,
                 symbol: Symbol,
                 order_id: OrderId,
                 execution_id: ExecutionId,
                 execution_ticket: ExecutionTicket,
                 order_side: OrderSide,
                 int filled_quantity,
                 average_price: Decimal,
                 execution_time: datetime,
                 event_id: UUID,
                 event_timestamp: datetime):
        """
        Initializes a new instance of the OrderFilled class.

        :param symbol: The events order symbol.
        :param order_id: The events order identifier.
        :param execution_id: The events order execution identifier.
        :param execution_ticket: The events order execution ticket.
        :param order_side: The events execution order side.
        :param filled_quantity: The events execution filled quantity.
        :param average_price: The events execution average price.
        :param execution_time: The events execution time.
        :param event_id: The events identifier.
        :param event_timestamp: The events timestamp.
        """
        Precondition.positive(filled_quantity, 'filled_quantity')

        super().__init__(symbol,
                         order_id,
                         event_id,
                         event_timestamp)
        self.execution_id = execution_id
        self.execution_ticket = execution_ticket
        self.order_side = order_side
        self.filled_quantity = filled_quantity
        self.average_price = average_price
        self.execution_time = execution_time


cdef class OrderPartiallyFilled(OrderEvent):
    """
    Represents an event where an order has been partially filled with the broker.
    """
    cdef readonly object execution_id
    cdef readonly object execution_ticket
    cdef readonly object order_side
    cdef readonly object filled_quantity
    cdef readonly object leaves_quantity
    cdef readonly object average_price
    cdef readonly object execution_time

    def __init__(self,
                 symbol: Symbol,
                 order_id: OrderId,
                 execution_id: ExecutionId,
                 execution_ticket: ExecutionTicket,
                 order_side: OrderSide,
                 int filled_quantity,
                 int leaves_quantity,
                 average_price: Decimal,
                 execution_time: datetime,
                 event_id: UUID,
                 event_timestamp: datetime):
        """
        Initializes a new instance of the OrderPartiallyFilled class.

        :param symbol: The events order symbol.
        :param order_id: The events order identifier.
        :param execution_id: The events order execution identifier.
        :param execution_ticket: The events order execution ticket.
        :param order_side: The events execution order side.
        :param filled_quantity: The events execution filled quantity.
        :param leaves_quantity: The events leaves quantity.
        :param average_price: The events execution average price.
        :param execution_time: The events execution time.
        :param event_id: The events identifier.
        :param event_timestamp: The events timestamp.
        """
        Precondition.positive(filled_quantity, 'filled_quantity')
        Precondition.positive(leaves_quantity, 'leaves_quantity')

        super().__init__(symbol,
                         order_id,
                         event_id,
                         event_timestamp)
        self.execution_id = execution_id
        self.execution_ticket = execution_ticket
        self.order_side = order_side
        self.filled_quantity = filled_quantity
        self.leaves_quantity = leaves_quantity
        self.average_price = average_price
        self.execution_time = execution_time


cdef class TimeEvent(Event):
    """
    Represents a time event occurring at the event timestamp.
    """
    cdef readonly object label

    def __init__(self,
                 label: Label,
                 event_id: UUID,
                 event_timestamp: datetime):
        """
        Initializes a new instance of the TimeEvent class.

        :param event_id: The time events identifier.
        :param event_timestamp: The time events timestamp.
        """
        super().__init__(event_id, event_timestamp)
        self.label = label

    def __str__(self) -> str:
        """
        :return: The str() string representation of the event.
        """
        return f"{self.__class__.__name__}(label={self.label}, timestamp={self.timestamp})"

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the event.
        """
        return f"<{str(self)} object at {id(self)}>"
