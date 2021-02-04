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

from nautilus_trader.model.order_book_rs cimport OrderBook as OrderBookRs
from nautilus_trader.model.order_book_rs cimport OrderBookEntry
from nautilus_trader.model cimport order_book_rs


cdef class OrderBook:
    cdef OrderBookRs _book

    def __cinit__(self, uint64_t timestamp):
        self._book = order_book_rs.new(timestamp)

    cpdef double spread(self):
        return order_book_rs.spread(&self._book)

    cpdef double best_bid_price(self):
        return self._book.best_bid_price

    cpdef double best_ask_price(self):
        return self._book.best_ask_price

    cpdef double best_bid_qty(self):
        return self._book.best_bid_qty

    cpdef double best_ask_qty(self):
        return self._book.best_ask_qty

    cpdef uint64_t timestamp(self):
        return self._book.timestamp

    cpdef uint64_t last_update_id(self):
        return self._book.last_update_id

    cpdef void apply_snapshot(
        self,
        double[:, :] bids,
        double[:, :] asks,
        uint64_t update_id,
        uint64_t timestamp,
    ) except *:
        #cdef OrderBookEntry[10] bid_entries = [order_book_rs.new_entry(row[0], row[1], update_id) for row in bids]
        #cdef OrderBookEntry[10] ask_entries = [order_book_rs.new_entry(row[0], row[1], update_id) for row in asks]

        [order_book_rs.apply_bid_diff(&self._book, order_book_rs.new_entry(row[0], row[1], update_id), timestamp) for row in bids]
        [order_book_rs.apply_ask_diff(&self._book, order_book_rs.new_entry(row[0], row[1], update_id), timestamp) for row in asks]

    cpdef void apply_bid_diff(
        self,
        double price,
        double qty,
        uint64_t update_id,
        uint64_t timestamp,
    ) except *:
        cdef OrderBookEntry entry = order_book_rs.new_entry(price, qty, update_id)
        order_book_rs.apply_bid_diff(&self._book, entry, timestamp)

    cpdef void apply_ask_diff(
        self,
        double price,
        double qty,
        uint64_t update_id,
        uint64_t timestamp,
    ) except *:
        cdef OrderBookEntry entry = order_book_rs.new_entry(price, qty, update_id)
        order_book_rs.apply_ask_diff(&self._book, entry, timestamp)

    cpdef double buy_price_for_qty(self, double qty) except *:
        return order_book_rs.buy_price_for_qty(&self._book, qty)

    cpdef double sell_price_for_qty(self, double qty) except *:
        return order_book_rs.sell_price_for_qty(&self._book, qty)
