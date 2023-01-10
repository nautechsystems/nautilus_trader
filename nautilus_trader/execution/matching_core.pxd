# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from libc.stdint cimport int64_t
from libc.stdint cimport uint64_t

from nautilus_trader.model.enums_c cimport LiquiditySide
from nautilus_trader.model.enums_c cimport OrderSide
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.orders.base cimport Order


cdef class MatchingCore:
    cdef Instrument _instrument
    cdef readonly int64_t bid_raw
    cdef readonly int64_t ask_raw
    cdef readonly int64_t last_raw
    cdef readonly bint is_bid_initialized
    cdef readonly bint is_ask_initialized
    cdef readonly bint is_last_initialized

    cdef object _trigger_stop_order
    cdef object _fill_market_order
    cdef object _fill_limit_order

    cdef dict _orders
    cdef list _orders_bid
    cdef list _orders_ask

# -- QUERIES --------------------------------------------------------------------------------------

    cpdef Order get_order(self, ClientOrderId client_order_id)
    cpdef bint order_exists(self, ClientOrderId client_order_id) except *
    cpdef list get_orders(self)
    cpdef list get_orders_bid(self)
    cpdef list get_orders_ask(self)

# -- COMMANDS -------------------------------------------------------------------------------------

    cdef void set_bid_raw(self, int64_t bid_raw) except *
    cdef void set_ask_raw(self, int64_t ask_raw) except *
    cdef void set_last_raw(self, int64_t last_raw) except *

    cpdef void reset(self) except *
    cpdef void add_order(self, Order order) except *
    cdef void _add_order(self, Order order) except *
    cdef void sort_bid_orders(self) except *
    cdef void sort_ask_orders(self) except *
    cpdef void delete_order(self, Order order) except *
    cpdef void iterate(self, uint64_t timestamp_ns) except *

# -- MATCHING -------------------------------------------------------------------------------------

    cpdef void match_order(self, Order order, bint initial=*) except *
    cpdef void match_limit_order(self, Order order) except *
    cpdef void match_stop_market_order(self, Order order) except *
    cpdef void match_stop_limit_order(self, Order order, bint initial) except *
    cpdef void match_market_if_touched_order(self, Order order) except *
    cpdef void match_limit_if_touched_order(self, Order order, bint initial) except *
    cpdef bint is_limit_matched(self, OrderSide side, Price price) except *
    cpdef bint is_stop_triggered(self, OrderSide side, Price trigger_price) except *
    cpdef bint is_touch_triggered(self, OrderSide side, Price trigger_price) except *
    cdef LiquiditySide _determine_order_liquidity(self, bint initial, OrderSide side, Price price, Price trigger_price) except *


cdef int64_t order_sort_key(Order order) except *
