# -------------------------------------------------------------------------------------------------
# <copyright file="events.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime

from nautilus_trader.core.types cimport ValidString
from nautilus_trader.core.message cimport Event
from nautilus_trader.model.c_enums.currency cimport Currency
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.order_type cimport OrderType
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForce
from nautilus_trader.model.objects cimport Quantity, Brokerage, Symbol, Price, Money
from nautilus_trader.model.identifiers cimport Label, AccountId, AccountNumber
from nautilus_trader.model.identifiers cimport StrategyId, OrderId, ExecutionId, ExecutionTicket
from nautilus_trader.model.position cimport Position


cdef class AccountEvent(Event):
    """
    Represents an account event produced from a collateral report.
    """
    cdef readonly AccountId account_id
    cdef readonly Brokerage brokerage
    cdef readonly AccountNumber account_number
    cdef readonly Currency currency
    cdef readonly Money cash_balance
    cdef readonly Money cash_start_day
    cdef readonly Money cash_activity_day
    cdef readonly Money margin_used_liquidation
    cdef readonly Money margin_used_maintenance
    cdef readonly object margin_ratio
    cdef readonly ValidString margin_call_status


cdef class OrderEvent(Event):
    """
    The base class for all order events.
    """
    cdef readonly OrderId order_id


cdef class OrderFillEvent(OrderEvent):
    """
    The base class for all order fill events.
    """
    cdef readonly ExecutionId execution_id
    cdef readonly ExecutionTicket execution_ticket
    cdef readonly Symbol symbol
    cdef readonly OrderSide order_side
    cdef readonly Quantity filled_quantity
    cdef readonly Price average_price
    cdef readonly datetime execution_time


cdef class OrderInitialized(OrderEvent):
    """
    Represents an event where an order has been initialized.
    """
    cdef readonly Symbol symbol
    cdef readonly Label label
    cdef readonly OrderSide order_side
    cdef readonly OrderType order_type
    cdef readonly Quantity quantity
    cdef readonly Price price
    cdef readonly TimeInForce time_in_force
    cdef readonly datetime expire_time


cdef class OrderSubmitted(OrderEvent):
    """
    Represents an event where an order has been submitted by the execution system.
    """
    cdef readonly datetime submitted_time


cdef class OrderRejected(OrderEvent):
    """
    Represents an event where an order has been rejected by the broker.
    """
    cdef readonly datetime rejected_time
    cdef readonly ValidString rejected_reason


cdef class OrderAccepted(OrderEvent):
    """
    Represents an event where an order has been accepted by the broker.
    """
    cdef readonly datetime accepted_time


cdef class OrderWorking(OrderEvent):
    """
    Represents an event where an order is working with the broker.
    """
    cdef readonly OrderId order_id_broker
    cdef readonly Symbol symbol
    cdef readonly Label label
    cdef readonly OrderSide order_side
    cdef readonly OrderType order_type
    cdef readonly Quantity quantity
    cdef readonly Price price
    cdef readonly TimeInForce time_in_force
    cdef readonly datetime working_time
    cdef readonly datetime expire_time


cdef class OrderCancelReject(OrderEvent):
    """
    Represents an event where an order cancel request has been rejected by the broker.
    """
    cdef readonly datetime rejected_time
    cdef readonly ValidString rejected_response_to
    cdef readonly ValidString rejected_reason


cdef class OrderCancelled(OrderEvent):
    """
    Represents an event where an order has been cancelled with the broker.
    """
    cdef readonly datetime cancelled_time


cdef class OrderExpired(OrderEvent):
    """
    Represents an event where an order has expired with the broker.
    """
    cdef readonly datetime expired_time


cdef class OrderModified(OrderEvent):
    """
    Represents an event where an order has been modified with the broker.
    """
    cdef readonly OrderId order_id_broker
    cdef readonly Price modified_price
    cdef readonly datetime modified_time


cdef class OrderFilled(OrderFillEvent):
    """
    Represents an event where an order has been completely filled with the broker.
    """


cdef class OrderPartiallyFilled(OrderFillEvent):
    """
    Represents an event where an order has been partially filled with the broker.
    """
    cdef readonly Quantity leaves_quantity


cdef class PositionEvent(Event):
    """
    The base class for all position events.
    """
    cdef readonly Position position
    cdef readonly StrategyId strategy_id
    cdef readonly OrderEvent order_fill


cdef class PositionOpened(PositionEvent):
    """
    Represents an event where a position has been opened.
    """


cdef class PositionModified(PositionEvent):
    """
    Represents an event where a position has been modified.
    """


cdef class PositionClosed(PositionEvent):
    """
    Represents an event where a position has been closed.
    """


cdef class TimeEvent(Event):
    """
    Represents a time event occurring at the event timestamp.
    """
    cdef readonly Label label
