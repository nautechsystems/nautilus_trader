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
from inv_trader.model.objects cimport Symbol
from inv_trader.model.events cimport OrderEvent
from inv_trader.model.identifiers cimport Label, OrderId
from inv_trader.model.enums import OrderSide, OrderType, TimeInForce, OrderStatus


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
    cdef readonly object side
    cdef readonly object type
    cdef readonly int quantity
    cdef readonly datetime timestamp
    cdef readonly object price
    cdef readonly object time_in_force
    cdef readonly datetime expire_time
    cdef readonly int filled_quantity
    cdef readonly object average_price
    cdef readonly object slippage
    cdef readonly object status
    cdef readonly list events

    cpdef void apply(self, OrderEvent order_event)
    cdef object _set_slippage(self)
    cdef object _check_overfill(self)
