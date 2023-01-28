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

from libc.stdint cimport uint8_t
from libc.stdint cimport uint64_t

from nautilus_trader.model.data.tick cimport QuoteTick
from nautilus_trader.model.data.tick cimport TradeTick
from nautilus_trader.model.enums_c cimport BookType
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.orderbook.data cimport BookOrder
from nautilus_trader.model.orderbook.data cimport OrderBookData
from nautilus_trader.model.orderbook.data cimport OrderBookDelta
from nautilus_trader.model.orderbook.data cimport OrderBookDeltas
from nautilus_trader.model.orderbook.data cimport OrderBookSnapshot
from nautilus_trader.model.orderbook.ladder cimport Ladder
from nautilus_trader.model.orderbook.level cimport Level


cdef class OrderBook:
    cdef readonly InstrumentId instrument_id
    """The order book instrument ID.\n\n:returns: `InstrumentId`"""
    cdef readonly BookType type
    """The order book type {``L1_TBBO``, ``L2_MBP``, ``L3_MBO``}.\n\n:returns: `BookType`"""
    cdef readonly uint8_t price_precision
    """The order book price precision.\n\n:returns: `uint8`"""
    cdef readonly uint8_t size_precision
    """The order book size precision.\n\n:returns: `uint8`"""
    cdef readonly Ladder bids
    """The order books bids.\n\n:returns: `Ladder`"""
    cdef readonly Ladder asks
    """The order books asks.\n\n:returns: `Ladder`"""
    cdef readonly uint64_t sequence
    """The last sequence number for the book.\n\n:returns: `uint64_t`"""
    cdef readonly uint64_t count
    """The update count for the book.\n\n:returns: `uint64_t`"""
    cdef readonly uint64_t ts_last
    """The UNIX timestamp (nanoseconds) when the order book was last updated.\n\n:returns: `uint64_t`"""

    cpdef void add(self, BookOrder order, uint64_t sequence=*) except *
    cpdef void update(self, BookOrder order, uint64_t sequence=*) except *
    cpdef void delete(self, BookOrder order, uint64_t sequence=*) except *
    cpdef void apply_delta(self, OrderBookDelta delta) except *
    cpdef void apply_deltas(self, OrderBookDeltas deltas) except *
    cpdef void apply_snapshot(self, OrderBookSnapshot snapshot) except *
    cpdef void apply(self, OrderBookData data) except *
    cpdef void clear_bids(self) except *
    cpdef void clear_asks(self) except *
    cpdef void clear(self) except *
    cpdef void check_integrity(self) except *
    cdef void _add(self, BookOrder order, uint64_t sequence) except *
    cdef void _update(self, BookOrder order, uint64_t sequence) except *
    cdef void _delete(self, BookOrder order, uint64_t sequence) except *
    cdef void _apply_delta(self, OrderBookDelta delta) except *
    cdef void _apply_sequence(self, uint64_t sequence) except *
    cdef void _check_integrity(self) except *

    cdef void update_quote_tick(self, QuoteTick tick) except *
    cdef void update_trade_tick(self, TradeTick tick) except *
    cpdef Level best_bid_level(self)
    cpdef Level best_ask_level(self)
    cpdef best_bid_price(self)
    cpdef best_ask_price(self)
    cpdef best_bid_qty(self)
    cpdef best_ask_qty(self)
    cpdef spread(self)
    cpdef midpoint(self)
    cpdef str pprint(self, int num_levels=*, show=*)
    cpdef int trade_side(self, TradeTick trade)

    cdef double get_price_for_volume_c(self, bint is_buy, double volume)
    cdef double get_price_for_quote_volume_c(self, bint is_buy, double quote_volume)
    cdef double get_volume_for_price_c(self, bint is_buy, double price)
    cdef double get_quote_volume_for_price_c(self, bint is_buy, double price)
    cdef double get_vwap_for_volume_c(self, bint is_buy, double volume)

    cpdef double get_price_for_volume(self, bint is_buy, double volume)
    cpdef double get_price_for_quote_volume(self, bint is_buy, double quote_volume)
    cpdef double get_volume_for_price(self, bint is_buy, double price)
    cpdef double get_quote_volume_for_price(self, bint is_buy, double price)
    cpdef double get_vwap_for_volume(self, bint is_buy, double volume)


cdef class L3OrderBook(OrderBook):
    pass


cdef class L2OrderBook(OrderBook):
    cdef void _process_order(self, BookOrder order) except *
    cdef void _remove_if_exists(self, BookOrder order, uint64_t sequence) except *


cdef class L1OrderBook(OrderBook):
    cdef BookOrder _top_bid
    cdef BookOrder _top_ask
    cdef Level _top_bid_level
    cdef Level _top_ask_level

    cdef BookOrder _process_order(self, BookOrder order)
