# -------------------------------------------------------------------------------------------------
# <copyright file="position.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime

from nautilus_trader.model.objects cimport Quantity, Price
from nautilus_trader.model.events cimport OrderFillEvent
from nautilus_trader.model.identifiers cimport (
    Symbol,
    PositionId,
    OrderId,
    AccountId,
    ExecutionId,
    ExecutionTicket)
from nautilus_trader.model.c_enums.market_position cimport MarketPosition
from nautilus_trader.model.c_enums.order_side cimport OrderSide


cdef class Position:
    """
    Represents a position in a financial market.
    """
    cdef set _order_ids
    cdef set _execution_ids
    cdef set _execution_tickets
    cdef list _events

    cdef readonly Symbol symbol
    cdef readonly PositionId id
    cdef readonly AccountId account_id
    cdef readonly ExecutionId last_execution_id
    cdef readonly ExecutionTicket last_execution_ticket
    cdef readonly OrderId from_order_id
    cdef readonly OrderId last_order_id
    cdef readonly datetime timestamp
    cdef readonly OrderSide entry_direction
    cdef readonly datetime entry_time
    cdef readonly datetime exit_time
    cdef readonly Price average_entry_price
    cdef readonly Price average_exit_price
    cdef readonly object points_realized
    cdef readonly float return_realized
    cdef readonly OrderFillEvent last_event

    cdef readonly long relative_quantity
    cdef readonly Quantity quantity
    cdef readonly Quantity peak_quantity
    cdef readonly MarketPosition market_position
    cdef readonly bint is_open
    cdef readonly bint is_closed
    cdef readonly bint is_flat
    cdef readonly bint is_long
    cdef readonly bint is_short

    cdef bint equals(self, Position other)
    cdef str status_string(self)
    cpdef list get_order_ids(self)
    cpdef list get_execution_ids(self)
    cpdef list get_execution_tickets(self)
    cpdef list get_events(self)
    cpdef int event_count(self)
    cpdef void apply(self, OrderFillEvent event)
    cpdef object points_unrealized(self, Price current_price)
    cpdef float return_unrealized(self, Price current_price)

    @staticmethod
    cdef int _calculate_relative_quantity(OrderFillEvent event)
    cdef void _fill_logic(self, OrderFillEvent event)
    cdef void _increment_returns(self, OrderFillEvent event)
    cdef object _calculate_points(self, Price entry_price, Price exit_price)
    cdef float _calculate_return(self, Price entry_price, Price exit_price)
