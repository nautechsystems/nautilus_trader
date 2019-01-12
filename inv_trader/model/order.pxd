#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="order.oxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False

from cpython.datetime cimport datetime

from inv_trader.core.decimal cimport Decimal
from inv_trader.common.clock cimport Clock
from inv_trader.model.objects cimport Symbol
from inv_trader.model.events cimport OrderEvent
from inv_trader.model.identifiers cimport Label, OrderId
from inv_trader.enums.order_side cimport OrderSide
from inv_trader.enums.order_type cimport OrderType
from inv_trader.enums.order_status cimport OrderStatus
from inv_trader.enums.time_in_force cimport TimeInForce


cdef class Order:
    """
    Represents an order in a financial market.
    """
    cdef list _order_ids
    cdef list _order_ids_broker
    cdef list _execution_ids
    cdef list _execution_tickets

    cdef readonly Symbol symbol
    cdef readonly OrderId id
    cdef readonly Label label
    cdef readonly OrderSide side
    cdef readonly OrderType type
    cdef readonly int quantity
    cdef readonly datetime timestamp
    cdef readonly Decimal price
    cdef readonly TimeInForce time_in_force
    cdef readonly datetime expire_time
    cdef readonly int filled_quantity
    cdef readonly Decimal average_price
    cdef readonly Decimal slippage
    cdef readonly OrderStatus status
    cdef readonly list events

    cpdef void apply(self, OrderEvent order_event)
    cdef Decimal _set_slippage(self)
    cdef Decimal _check_overfill(self)


cdef class OrderIdGenerator:
    """
    Provides a generator for unique order identifiers.
    """
    cdef Clock _clock
    cdef str _separator
    cdef str _order_id_tag
    cdef dict _order_symbol_counts
    cdef list _order_ids

    cpdef OrderId generate(self, Symbol order_symbol)


cdef class OrderFactory:
    """
    A factory class which provides different order types.
    """
    cdef Clock _clock

    cpdef Order market(
            self,
            Symbol symbol,
            OrderId order_id,
            Label label,
            OrderSide order_side,
            int quantity)

    cpdef Order limit(
            self,
            Symbol symbol,
            OrderId order_id,
            Label label,
            OrderSide order_side,
            int quantity,
            Decimal price,
            TimeInForce time_in_force=*,
            datetime expire_time=*)

    cpdef Order stop_market(
            self,
            Symbol symbol,
            OrderId order_id,
            Label label,
            OrderSide order_side,
            int quantity,
            Decimal price,
            TimeInForce time_in_force=*,
            datetime expire_time=*)

    cpdef Order stop_limit(
            self,
            Symbol symbol,
            OrderId order_id,
            Label label,
            OrderSide order_side,
            int quantity,
            Decimal price,
            TimeInForce time_in_force=*,
            datetime expire_time=*)

    cpdef Order market_if_touched(
            self,
            Symbol symbol,
            OrderId order_id,
            Label label,
            OrderSide order_side,
            int quantity,
            Decimal price,
            TimeInForce time_in_force=*,
            datetime expire_time=*)

    cpdef Order fill_or_kill(
            self,
            Symbol symbol,
            OrderId order_id,
            Label label,
            OrderSide order_side,
            int quantity)

    cpdef Order immediate_or_cancel(
            self,
            Symbol symbol,
            OrderId order_id,
            Label label,
            OrderSide order_side,
            int quantity)
