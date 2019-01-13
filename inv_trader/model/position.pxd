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

from inv_trader.core.decimal cimport Decimal
from inv_trader.model.objects cimport Symbol
from inv_trader.model.events cimport OrderEvent
from inv_trader.model.identifiers cimport PositionId, OrderId, ExecutionId, ExecutionTicket
from inv_trader.enums.market_position cimport MarketPosition


cdef class Position:
    """
    Represents a position in a financial market.
    """
    cdef int _relative_quantity
    cdef int _peak_quantity
    cdef list _order_ids
    cdef list _execution_ids
    cdef list _execution_tickets
    cdef list _events

    cdef readonly Symbol symbol
    cdef readonly PositionId id
    cdef readonly ExecutionId execution_id
    cdef readonly ExecutionTicket execution_ticket
    cdef readonly OrderId from_order_id
    cdef readonly int quantity
    cdef readonly MarketPosition market_position
    cdef readonly datetime timestamp
    cdef readonly datetime entry_time
    cdef readonly datetime exit_time
    cdef readonly Decimal average_entry_price
    cdef readonly Decimal average_exit_price
    cdef readonly bint is_entered
    cdef readonly bint is_exited
    cdef readonly int event_count
    cdef readonly OrderEvent last_event

    cdef bint equals(self, Position other)
    cpdef void apply(self, OrderEvent event)
