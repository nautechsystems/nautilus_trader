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
from nautilus_trader.model.orderbook.order cimport Order

# TODO - Some violations of DRY principal here - I can't think of a way around this with cython given the slow
#  subclassing. The best I can come up with is this shared OrderBookProxy object which is going to mean a bunch of
#  duplicated accessor code for the L1/L2/L3 Orderbook classes. Possible some code generation might be worthwhile here?

cdef class OrderBookProxy:
    cdef readonly Ladder bids
    cdef readonly Ladder asks

    cpdef void add(self, Order order)
    cpdef void update(self, Order order)
    cpdef void delete(self, Order order)
    cpdef void clear(self)
    cpdef bint _check_integrity(self, bint deep= *)


cdef class L3OrderBook:
    cdef readonly OrderBookProxy _order_book
    cpdef void add(self, Order order)
    cpdef void update(self, Order order)
    cpdef void delete(self, Order order)
    cpdef void delete(self, Order order)
    cpdef void delete(self, Order order)
    cpdef bint _check_integrity(self, bint deep= *)


cdef class L2OrderBook:
    cpdef void add(self, Order order)
    cpdef void update(self, Order order)
    cpdef void delete(self, Order order)


cdef class L1OrderBook:
    cpdef void add(self, Order order)
    cpdef void update(self, Order order)
    cpdef void delete(self, Order order)
