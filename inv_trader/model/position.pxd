#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="position.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False

from cpython.datetime cimport datetime
from inv_trader.model.objects cimport Symbol
from inv_trader.model.events cimport OrderEvent
from inv_trader.model.identifiers cimport PositionId
from inv_trader.enums.order_side cimport OrderSide


cdef class Position:
    """
    Represents a position in a financial market.
    """
    cdef int _relative_quantity
    cdef int _peak_quantity


    cdef readonly Symbol symbol
    cdef readonly PositionId id
    cdef readonly datetime timestamp
    cdef readonly datetime entry_time
    cdef readonly datetime exit_time
    cdef readonly object average_entry_price
    cdef readonly object average_exit_price
    cdef readonly list execution_ids
    cdef readonly list execution_tickets
    cdef readonly list events

    cpdef void apply(self, OrderEvent event)
    cdef void _update_position(
            self,
            OrderSide order_side,
            int quantity,
            average_price,
            datetime event_time)
