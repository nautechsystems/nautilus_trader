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
from nautilus_trader.model cimport order_book_rs


cdef class OrderBook:
    cdef OrderBookRs _book

    def __cinit__(self, uint64_t timestamp):
        self._book = order_book_rs.new(timestamp)

    # cpdef void apply_snapshot(
    #     self,
    #     double[:, :] bids,
    #     double[:, :] asks,
    #     uint64_t timestamp,
    #     uint64_t update_id,
    # ) except *:
    #     order_book_rs.apply_snapshot(
    #         &self._book,
    #         <double [25][2]>bids,
    #         <double [25][2]>asks,
    #         timestamp,
    #         update_id,
    #     )

    cpdef double spread(self):
        return order_book_rs.spread(&self._book)

    cpdef double best_bid_price(self):
        return self._book.best_bid_price

    cpdef double best_ask_price(self):
        return self._book.best_ask_price

    cpdef double best_bid_qty(self):
        return self._book.best_bid_qty

    cpdef double best_ask_qty(self):
        return self._book.best_bid_qty

    cpdef uint64_t timestamp(self):
        return self._book.timestamp

    cpdef uint64_t last_update_id(self):
        return self._book.last_update_id
