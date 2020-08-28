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

from nautilus_trader.model.c_enums.currency cimport Currency
from nautilus_trader.model.c_enums.market_position cimport MarketPosition
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.events cimport OrderFillEvent
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ExecutionId
from nautilus_trader.model.identifiers cimport OrderId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport PositionIdBroker
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.tick cimport Tick


cdef class Position:
    cdef set _order_ids
    cdef set _execution_ids
    cdef list _events
    cdef dict _fill_prices
    cdef dict _buy_quantities
    cdef dict _sell_quantities
    cdef Quantity _buy_quantity
    cdef Quantity _sell_quantity
    cdef double _relative_quantity
    cdef int _precision

    cdef readonly PositionId id
    cdef readonly PositionIdBroker id_broker
    cdef readonly AccountId account_id
    cdef readonly ExecutionId last_execution_id

    cdef readonly OrderId from_order_id
    cdef readonly OrderId last_order_id
    cdef readonly datetime timestamp
    cdef readonly Symbol symbol
    cdef readonly Currency quote_currency
    cdef readonly OrderSide entry_direction
    cdef readonly datetime opened_time
    cdef readonly datetime closed_time
    cdef readonly timedelta open_duration
    cdef readonly double average_open_price
    cdef readonly double average_close_price
    cdef readonly double realized_points
    cdef readonly double realized_return
    cdef readonly Money realized_pnl
    cdef readonly Money realized_pnl_last
    cdef readonly OrderFillEvent last_event
    cdef readonly int event_count
    cdef readonly Quantity quantity
    cdef readonly Quantity peak_quantity
    cdef readonly MarketPosition market_position
    cdef readonly bint is_open
    cdef readonly bint is_closed
    cdef readonly bint is_long
    cdef readonly bint is_short

    cpdef bint equals(self, Position other)
    cpdef str to_string(self)
    cpdef str market_position_as_string(self)
    cpdef str status_string(self)
    cpdef list get_order_ids(self)
    cpdef list get_execution_ids(self)
    cpdef list get_events(self)
    cpdef void apply(self, OrderFillEvent event) except *
    cpdef double relative_quantity(self)
    cpdef double unrealized_points(self, Tick last)
    cpdef double total_points(self, Tick last)
    cpdef double unrealized_return(self, Tick last)
    cpdef double total_return(self, Tick last)
    cpdef Money unrealized_pnl(self, Tick last)
    cpdef Money total_pnl(self, Tick last)

    cdef void _update(self, OrderFillEvent event) except *
    cdef void _handle_buy_order_fill(self, OrderFillEvent event) except *
    cdef void _handle_sell_order_fill(self, OrderFillEvent event) except *
    cdef double _calculate_average_price(self, dict fills, Quantity total_quantity)
    cdef double _calculate_points(self, double open_price, double close_price)
    cdef double _calculate_return(self, double open_price, double close_price)
    cdef Money _calculate_pnl(self, double open_price, double close_price, Quantity filled_quantity)
