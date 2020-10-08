# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime
from cpython.datetime cimport timedelta

from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.position_side cimport PositionSide
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.events cimport OrderFilled
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport ExecutionId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.objects cimport Decimal
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.tick cimport QuoteTick


cdef class Position:
    cdef list _events
    cdef Quantity _buy_quantity
    cdef Quantity _sell_quantity
    cdef Decimal _relative_quantity

    cdef readonly PositionId id
    cdef readonly AccountId account_id
    cdef readonly ClientOrderId from_order
    cdef readonly StrategyId strategy_id

    cdef readonly datetime timestamp
    cdef readonly Symbol symbol
    cdef readonly OrderSide entry
    cdef readonly PositionSide side
    cdef readonly Quantity quantity
    cdef readonly Quantity peak_quantity
    cdef readonly Currency base_currency
    cdef readonly Currency quote_currency
    cdef readonly datetime opened_time
    cdef readonly datetime closed_time
    cdef readonly timedelta open_duration
    cdef readonly double avg_open_price
    cdef readonly double avg_close_price
    cdef readonly double realized_points
    cdef readonly double realized_return
    cdef readonly Money realized_pnl
    cdef readonly Money unrealized_pnl
    cdef readonly Money total_pnl
    cdef readonly Money commission
    cdef readonly QuoteTick last_tick

    cpdef void apply(self, OrderFilled event) except *
    cpdef void update(self, QuoteTick tick) except *
    cpdef str to_string(self)
    cpdef str position_side_as_string(self)
    cpdef str status_string(self)
    cpdef OrderFilled last_event(self)
    cpdef ExecutionId last_execution_id(self)
    cpdef set get_cl_ord_ids(self)
    cpdef set get_order_ids(self)
    cpdef set get_execution_ids(self)
    cpdef list get_events(self)
    cpdef int event_count(self)
    cpdef bint is_open(self) except *
    cpdef bint is_closed(self) except *
    cpdef bint is_long(self) except *
    cpdef bint is_short(self) except *
    cpdef Decimal relative_quantity(self)

    cdef inline void _handle_buy_order_fill(self, OrderFilled event) except *
    cdef inline void _handle_sell_order_fill(self, OrderFilled event) except *
    cdef inline double _calculate_cost(self, double avg_price,  Quantity total_quantity)
    cdef inline double _calculate_avg_price(self, double price_open, Quantity quantity_open, OrderFilled event)
    cdef inline double _calculate_avg_open_price(self, OrderFilled event)
    cdef inline double _calculate_avg_close_price(self, OrderFilled event)
    cdef inline double _calculate_points(self, double open_price, double close_price)
    cdef inline double _calculate_return(self, double open_price, double close_price)
    cdef inline Money _calculate_pnl(self, double open_price, double close_price, Quantity filled_qty)
