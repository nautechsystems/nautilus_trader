# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.core.rust.model cimport Price_t
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.orders.base cimport Order


cdef class MatchingCore:
    cdef Instrument _instrument
    cdef int64_t _bid_raw
    cdef int64_t _ask_raw
    cdef int64_t _last_raw
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

    cdef void set_bid(self, Price_t bid) except *
    cdef void set_ask(self, Price_t ask) except *
    cdef void set_last(self, Price_t last) except *

    cpdef void reset(self) except *
    cpdef void add_order(self, Order order) except *
    cdef void _add_order(self, Order order) except *
    cpdef void delete_order(self, Order order) except *
    cpdef void iterate(self, uint64_t timestamp_ns) except *

# -- MATCHING -------------------------------------------------------------------------------------

    cpdef void match_order(self, Order order) except *
    cpdef void match_limit_order(self, Order order) except *
    cpdef void match_stop_market_order(self, Order order) except *
    cpdef void match_stop_limit_order(self, Order order) except *
    cpdef bint is_limit_matched(self, OrderSide side, Price price) except *
    cpdef bint is_stop_triggered(self, OrderSide side, Price price) except *
