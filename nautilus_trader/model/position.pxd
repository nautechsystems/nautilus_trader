# -------------------------------------------------------------------------------------------------
# <copyright file="position.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime, timedelta

from nautilus_trader.model.c_enums.currency cimport Currency
from nautilus_trader.model.objects cimport Quantity, Tick, Money
from nautilus_trader.model.events cimport OrderFillEvent
from nautilus_trader.model.identifiers cimport (
    Symbol,
    PositionId,
    OrderId,
    AccountId,
    ExecutionId,
    PositionIdBroker)
from nautilus_trader.model.c_enums.market_position cimport MarketPosition
from nautilus_trader.model.c_enums.order_side cimport OrderSide


cdef class Position:
    cdef set _order_ids
    cdef set _execution_ids
    cdef list _events
    cdef dict _fill_prices
    cdef dict _buy_quantities
    cdef dict _sell_quantities
    cdef long _buy_quantity
    cdef long _sell_quantity

    cdef readonly PositionId id
    cdef readonly PositionIdBroker id_broker
    cdef readonly AccountId account_id
    cdef readonly ExecutionId last_execution_id

    cdef readonly OrderId from_order_id
    cdef readonly OrderId last_order_id
    cdef readonly datetime timestamp
    cdef readonly Symbol symbol
    cdef readonly Currency base_currency
    cdef readonly OrderSide entry_direction
    cdef readonly datetime opened_time
    cdef readonly datetime closed_time
    cdef readonly timedelta open_duration
    cdef readonly object average_open_price
    cdef readonly object average_close_price
    cdef readonly object realized_points
    cdef readonly float realized_return
    cdef readonly Money realized_pnl
    cdef readonly Money realized_pnl_last
    cdef readonly OrderFillEvent last_event
    cdef readonly int event_count

    cdef readonly long relative_quantity
    cdef readonly Quantity quantity
    cdef readonly Quantity peak_quantity
    cdef readonly MarketPosition market_position
    cdef readonly bint is_open
    cdef readonly bint is_closed
    cdef readonly bint is_long
    cdef readonly bint is_short

    cdef bint equals(self, Position other)
    cpdef str status_string(self)
    cpdef list get_order_ids(self)
    cpdef list get_execution_ids(self)
    cpdef list get_events(self)
    cpdef void apply(self, OrderFillEvent event) except *
    cpdef object unrealized_points(self, Tick last)
    cpdef float unrealized_return(self, Tick last) except *
    cpdef Money unrealized_pnl(self, Tick last)
    cpdef object total_points(self, Tick last)
    cpdef float total_return(self, Tick last) except *
    cpdef Money total_pnl(self, Tick last )

    cdef void _update(self, OrderFillEvent event) except *
    cdef void _handle_buy_order_fill(self, OrderFillEvent event)
    cdef void _handle_sell_order_fill(self, OrderFillEvent event)
    cdef object _calculate_average_price(self, dict fills, long total_quantity)
    cdef object _calculate_points(self, open_price, close_price)
    cdef float _calculate_return(self, open_price, close_price)
    cdef Money _calculate_pnl(self, open_price, close_price, long filled_quantity)
