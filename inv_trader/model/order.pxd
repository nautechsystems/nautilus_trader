#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="order.oxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

from cpython.datetime cimport datetime

from inv_trader.common.clock cimport Clock
from inv_trader.model.objects cimport Symbol, Price
from inv_trader.model.events cimport OrderEvent
from inv_trader.model.identifiers cimport Label, OrderId, ExecutionId, ExecutionTicket
from inv_trader.model.identifiers cimport OrderIdGenerator
from inv_trader.enums.order_side cimport OrderSide
from inv_trader.enums.order_type cimport OrderType
from inv_trader.enums.order_status cimport OrderStatus
from inv_trader.enums.time_in_force cimport TimeInForce


cdef class Order:
    """
    Represents an order in a financial market.
    """
    cdef list _order_ids_broker
    cdef list _execution_ids
    cdef list _execution_tickets
    cdef list _events

    cdef readonly Symbol symbol
    cdef readonly OrderId id
    cdef readonly OrderId id_current
    cdef readonly OrderId broker_id
    cdef readonly ExecutionId execution_id
    cdef readonly ExecutionTicket execution_ticket
    cdef readonly Label label
    cdef readonly OrderSide side
    cdef readonly OrderType type
    cdef readonly int quantity
    cdef readonly datetime timestamp
    cdef readonly Price price
    cdef readonly TimeInForce time_in_force
    cdef readonly datetime expire_time
    cdef readonly int filled_quantity
    cdef readonly Price average_price
    cdef readonly object slippage
    cdef readonly OrderStatus status
    cdef readonly int event_count
    cdef readonly OrderEvent last_event
    cdef readonly bint is_complete

    cdef bint equals(self, Order other)
    cpdef list get_order_ids_broker(self)
    cpdef list get_execution_ids(self)
    cpdef list get_execution_tickets(self)
    cpdef list get_events(self)
    cpdef void apply(self, OrderEvent order_event)
    cdef void _set_slippage(self)
    cdef void _set_fill_status(self)


cdef class OrderFactory:
    """
    A factory class which provides different order types.
    """
    cdef Clock _clock
    cdef OrderIdGenerator _id_generator

    cpdef Order market(
            self,
            Symbol symbol,
            Label label,
            OrderSide order_side,
            int quantity)

    cpdef Order limit(
            self,
            Symbol symbol,
            Label label,
            OrderSide order_side,
            int quantity,
            Price price,
            TimeInForce time_in_force=*,
            datetime expire_time=*)

    cpdef Order stop_market(
            self,
            Symbol symbol,
            Label label,
            OrderSide order_side,
            int quantity,
            Price price,
            TimeInForce time_in_force=*,
            datetime expire_time=*)

    cpdef Order stop_limit(
            self,
            Symbol symbol,
            Label label,
            OrderSide order_side,
            int quantity,
            Price price,
            TimeInForce time_in_force=*,
            datetime expire_time=*)

    cpdef Order market_if_touched(
            self,
            Symbol symbol,
            Label label,
            OrderSide order_side,
            int quantity,
            Price price,
            TimeInForce time_in_force=*,
            datetime expire_time=*)

    cpdef Order fill_or_kill(
            self,
            Symbol symbol,
            Label label,
            OrderSide order_side,
            int quantity)

    cpdef Order immediate_or_cancel(
            self,
            Symbol symbol,
            Label label,
            OrderSide order_side,
            int quantity)
