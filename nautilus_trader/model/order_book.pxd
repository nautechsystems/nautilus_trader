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

from libc.stdint cimport uint64_t

from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.order_book_rs cimport OrderBook as OrderBookRs


cdef class OrderBook:
    cdef OrderBookRs _book

    cdef readonly Symbol symbol
    """The order book symbol.\n\n:returns: `Symbol`"""
    cdef readonly int level
    """The order book data level (L1, L2, L3).\n\n:returns: `int`"""
    cdef readonly int depth
    """The order book depth.\n\n:returns: `int`"""
    cdef readonly int price_precision
    """The precision for the order book prices.\n\n:returns: `int`"""
    cdef readonly int size_precision
    """The precision for the order book quantities.\n\n:returns: `int`"""

    cpdef list bids(self)
    cpdef list asks(self)
    cpdef list bids_as_decimals(self)
    cpdef list asks_as_decimals(self)
    cpdef double spread(self)
    cpdef double best_bid_price(self)
    cpdef double best_ask_price(self)
    cpdef double best_bid_qty(self)
    cpdef double best_ask_qty(self)
    cpdef double buy_price_for_qty(self, double qty) except *
    cpdef double buy_qty_for_price(self, double price) except *
    cpdef double sell_price_for_qty(self, double qty) except *
    cpdef double sell_qty_for_price(self, double price) except *
    cpdef uint64_t timestamp(self)
    cpdef uint64_t last_update_id(self)

    cpdef void apply_snapshot(
        self,
        list bids,
        list asks,
        uint64_t update_id,
        uint64_t timestamp,
    ) except *
    cpdef void apply_bid_diff(
        self,
        double price,
        double qty,
        uint64_t update_id,
        uint64_t timestamp,
    ) except *
    cpdef void apply_ask_diff(
        self,
        double price,
        double qty,
        uint64_t update_id,
        uint64_t timestamp,
    ) except *
