#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="position.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

from cpython.datetime cimport datetime

from inv_trader.model.objects cimport Quantity, Symbol, Price
from inv_trader.model.events cimport OrderEvent
from inv_trader.model.identifiers cimport PositionId, OrderId, ExecutionId, ExecutionTicket
from inv_trader.c_enums.market_position cimport MarketPosition
from inv_trader.c_enums.order_side cimport OrderSide


cdef class Position:
    """
    Represents a position in a financial market.
    """
    cdef list _order_ids
    cdef list _execution_ids
    cdef list _execution_tickets
    cdef list _events

    cdef readonly Symbol symbol
    cdef readonly PositionId id
    cdef readonly ExecutionId last_execution_id
    cdef readonly ExecutionTicket last_execution_ticket
    cdef readonly OrderId from_order_id
    cdef readonly OrderId last_order_id
    cdef readonly long relative_quantity
    cdef readonly Quantity quantity
    cdef readonly Quantity peak_quantity
    cdef readonly MarketPosition market_position
    cdef readonly datetime timestamp
    cdef readonly OrderSide entry_direction
    cdef readonly datetime entry_time
    cdef readonly datetime exit_time
    cdef readonly Price average_entry_price
    cdef readonly Price average_exit_price
    cdef readonly object points_realized
    cdef readonly float return_realized
    cdef readonly bint is_flat
    cdef readonly bint is_long
    cdef readonly bint is_short
    cdef readonly bint is_entered
    cdef readonly bint is_exited
    cdef readonly OrderEvent last_event

    cdef bint equals(self, Position other)
    cdef str status_string(self)
    cpdef list get_order_ids(self)
    cpdef list get_execution_ids(self)
    cpdef list get_execution_tickets(self)
    cpdef list get_events(self)
    cpdef int event_count(self)
    cpdef void apply(self, OrderEvent event)
    cpdef object points_unrealized(self, Price current_price)
    cpdef float return_unrealized(self, Price current_price)

    cdef object _calculate_points(self, Price entry_price, Price exit_price)
    cdef float _calculate_return(self, Price entry_price, Price exit_price)
