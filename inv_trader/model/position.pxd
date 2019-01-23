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
    cdef long _relative_quantity
    cdef long _peak_quantity
    cdef set _from_order_ids
    cdef list _execution_ids
    cdef list _execution_tickets
    cdef list _events

    cdef readonly Symbol symbol
    cdef readonly PositionId id
    cdef readonly ExecutionId last_execution_id
    cdef readonly ExecutionTicket last_execution_ticket
    cdef readonly OrderId from_order_id
    cdef readonly long quantity
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
    cdef readonly Decimal realized_pnl

    cdef bint equals(self, Position other)
    cpdef list get_from_order_ids(self)
    cpdef list get_execution_ids(self)
    cpdef list get_execution_tickets(self)
    cpdef list get_events(self)
    cpdef void apply(self, OrderEvent event)
