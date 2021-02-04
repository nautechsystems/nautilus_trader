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


cdef extern from "lib/nautilus-order-book/src/nautilus_order_book.h":
    ctypedef struct OrderBookEntry:
        double price
        double qty
        uint64_t update_id

    OrderBookEntry new_entry(double price, double qty, uint64_t update_id)

    ctypedef struct OrderBook:
        double best_bid_price
        double best_ask_price
        double best_bid_qty
        double best_ask_qty
        uint64_t timestamp
        uint64_t last_update_id

    void apply_bid_diff(OrderBook *self, OrderBookEntry entry, uint64_t timestamp)
    void apply_ask_diff(OrderBook *self, OrderBookEntry entry, uint64_t timestamp)
    OrderBook new(uint64_t timestamp)
    double spread(OrderBook *self)
    double buy_price_for_qty(OrderBook *self, double qty)
    double sell_price_for_qty(OrderBook *self, double qty)
