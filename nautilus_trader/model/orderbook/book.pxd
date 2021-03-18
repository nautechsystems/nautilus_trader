# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.model.orderbook.ladder cimport Ladder
from nautilus_trader.model.orderbook.level cimport Level
from nautilus_trader.model.orderbook.order cimport Order

# TODO - Some violations of DRY principal here - I can't think of a way around this with cython given the slow
#  subclassing. The best I can come up with is this shared OrderBookProxy object which is going to mean a bunch of
#  duplicated accessor code for the L1/L2/L3 Orderbook classes. Possible some code generation might be worthwhile here?

cdef class OrderBookProxy:
    cdef readonly Ladder bids
    """The order books bids.\n\n:returns: `Ladder`"""
    cdef readonly Ladder asks
    """The order books asks.\n\n:returns: `Ladder`"""

    cpdef void add(self, Order order) except *
    cpdef void update(self, Order order) except *
    cpdef void delete(self, Order order) except *
    cpdef void clear_bids(self) except *
    cpdef void clear_asks(self) except *
    cpdef void clear(self) except *
    cpdef Level best_bid(self)
    cpdef Level best_ask(self)
    cpdef bint check_integrity(self, bint deep=*) except *


cdef class OrderBook:
    cdef OrderBookProxy _orderbook

    cpdef void add(self, Order order) except *
    cpdef void update(self, Order order) except *
    cpdef void delete(self, Order order) except *
    cpdef bint check_integrity(self, bint deep=*) except *
    cpdef Ladder bids(self)
    cpdef Ladder asks(self)
    cpdef Level best_bid(self)
    cpdef Level best_ask(self)
    cpdef double spread(self) except *
    cpdef double best_bid_price(self) except *
    cpdef double best_ask_price(self) except *
    cpdef double best_bid_qty(self) except *
    cpdef double best_ask_qty(self) except *


cdef class L3OrderBook(OrderBook):
    pass


cdef class L2OrderBook(OrderBook):
    cdef inline Order _process_order(self, Order order)
    cdef inline void _remove_if_exists(self, Order order) except *


cdef class L1OrderBook(OrderBook):
    cdef inline Order _process_order(self, Order order)
