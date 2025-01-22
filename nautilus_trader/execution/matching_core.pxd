# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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
from libc.stdint cimport uint8_t
from libc.stdint cimport uint64_t

from nautilus_trader.core.rust.model cimport LiquiditySide
from nautilus_trader.core.rust.model cimport OrderSide
from nautilus_trader.core.rust.model cimport PriceRaw
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.orders.base cimport Order


cdef class MatchingCore:
    cdef InstrumentId _instrument_id
    cdef Price _price_increment
    cdef uint8_t _price_precision
    cdef readonly PriceRaw bid_raw
    cdef readonly PriceRaw ask_raw
    cdef readonly PriceRaw last_raw
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
    cpdef bint order_exists(self, ClientOrderId client_order_id)
    cpdef list get_orders(self)
    cpdef list get_orders_bid(self)
    cpdef list get_orders_ask(self)

# -- COMMANDS -------------------------------------------------------------------------------------

    cdef void set_bid_raw(self, PriceRaw bid_raw)
    cdef void set_ask_raw(self, PriceRaw ask_raw)
    cdef void set_last_raw(self, PriceRaw last_raw)

    cpdef void reset(self)
    cpdef void add_order(self, Order order)
    cdef void _add_order(self, Order order)
    cdef void sort_bid_orders(self)
    cdef void sort_ask_orders(self)
    cpdef void delete_order(self, Order order)
    cpdef void iterate(self, uint64_t timestamp_ns)

# -- MATCHING -------------------------------------------------------------------------------------

    cpdef void match_order(self, Order order, bint initial=*)
    cpdef void match_limit_order(self, Order order)
    cpdef void match_stop_market_order(self, Order order)
    cpdef void match_stop_limit_order(self, Order order, bint initial)
    cpdef void match_market_if_touched_order(self, Order order)
    cpdef void match_limit_if_touched_order(self, Order order, bint initial)
    cpdef bint is_limit_matched(self, OrderSide side, Price price)
    cpdef bint is_stop_triggered(self, OrderSide side, Price trigger_price)
    cpdef bint is_touch_triggered(self, OrderSide side, Price trigger_price)
    cdef LiquiditySide _determine_order_liquidity(self, bint initial, OrderSide side, Price price, Price trigger_price)


cdef int64_t order_sort_key(Order order)
