# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

from libc.stdint cimport uint8_t
from libc.stdint cimport uint64_t

from nautilus_trader.core.data cimport Data
from nautilus_trader.core.rust.model cimport Level_API
from nautilus_trader.core.rust.model cimport OrderBook_API
from nautilus_trader.core.rust.model cimport OrderSide
from nautilus_trader.model.data cimport BookOrder
from nautilus_trader.model.data cimport OrderBookDelta
from nautilus_trader.model.data cimport OrderBookDeltas
from nautilus_trader.model.data cimport OrderBookDepth10
from nautilus_trader.model.data cimport QuoteTick
from nautilus_trader.model.data cimport TradeTick
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport Order


cdef class OrderBook(Data):
    cdef OrderBook_API _mem

    cpdef void reset(self)
    cpdef void add(self, BookOrder order, uint64_t ts_event, uint64_t sequence=*)
    cpdef void update(self, BookOrder order, uint64_t ts_event, uint64_t sequence=*)
    cpdef void delete(self, BookOrder order, uint64_t ts_event, uint64_t sequence=*)
    cpdef void clear(self, uint64_t ts_event, uint64_t sequence=*)
    cpdef void clear_bids(self, uint64_t ts_event, uint64_t sequence=*)
    cpdef void clear_asks(self, uint64_t ts_event, uint64_t sequence=*)
    cpdef void apply_delta(self, OrderBookDelta delta)
    cpdef void apply_deltas(self, OrderBookDeltas deltas)
    cpdef void apply_depth(self, OrderBookDepth10 depth)
    cpdef void apply(self, Data data)
    cpdef void check_integrity(self)

    cpdef list bids(self)
    cpdef list asks(self)
    cpdef best_bid_price(self)
    cpdef best_ask_price(self)
    cpdef best_bid_size(self)
    cpdef best_ask_size(self)
    cpdef spread(self)
    cpdef midpoint(self)
    cpdef double get_avg_px_for_quantity(self, Quantity quantity, OrderSide order_side)
    cpdef double get_quantity_for_price(self, Price price, OrderSide order_side)
    cpdef list simulate_fills(self, Order order, uint8_t price_prec, bint is_aggressive)
    cpdef void update_quote_tick(self, QuoteTick tick)
    cpdef void update_trade_tick(self, TradeTick tick)
    cpdef str pprint(self, int num_levels=*)


cdef class Level:
    cdef Level_API _mem

    cpdef list orders(self)
    cpdef double size(self)
    cpdef double exposure(self)

    @staticmethod
    cdef Level from_mem_c(Level_API mem)
