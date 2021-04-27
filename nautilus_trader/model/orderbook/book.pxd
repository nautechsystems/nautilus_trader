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

from libc.stdint cimport int64_t

from nautilus_trader.model.c_enums.orderbook_delta cimport OrderBookDeltaType
from nautilus_trader.model.c_enums.orderbook_level cimport OrderBookLevel
from nautilus_trader.model.data cimport Data
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.orderbook.ladder cimport Ladder
from nautilus_trader.model.orderbook.level cimport Level
from nautilus_trader.model.orderbook.order cimport Order
from nautilus_trader.model.tick cimport QuoteTick
from nautilus_trader.model.tick cimport Tick
from nautilus_trader.model.tick cimport TradeTick


cdef class OrderBook:
    cdef readonly InstrumentId instrument_id
    """The order book instrument identifier.\n\n:returns: `InstrumentId`"""
    cdef readonly OrderBookLevel level
    """The order book level (L1, L2, L3).\n\n:returns: `OrderBookLevel (Enum)`"""
    cdef readonly int price_precision
    """The order book price precision.\n\n:returns: `int`"""
    cdef readonly int size_precision
    """The order book size precision.\n\n:returns: `int`"""
    cdef readonly Ladder bids
    """The order books bids.\n\n:returns: `Ladder`"""
    cdef readonly Ladder asks
    """The order books asks.\n\n:returns: `Ladder`"""
    cdef readonly int64_t last_update_timestamp_ns
    """The Unix timestamp (nanos) of the last update.\n\n:returns: `int64`"""
    cdef readonly int64_t last_update_id
    """The identifier of the last update.\n\n:returns: `int64`"""

    cpdef void add(self, Order order) except *
    cpdef void update(self, Order order) except *
    cpdef void delete(self, Order order) except *
    cpdef void apply_delta(self, OrderBookDelta delta) except *
    cpdef void apply_deltas(self, OrderBookDeltas deltas) except *
    cpdef void apply_snapshot(self, OrderBookSnapshot snapshot) except *
    cpdef void apply(self, OrderBookData data) except *
    cpdef void clear_bids(self) except *
    cpdef void clear_asks(self) except *
    cpdef void clear(self) except *
    cpdef void check_integrity(self) except *
    cdef inline void _apply_delta(self, OrderBookDelta delta) except *
    cdef inline void _add(self, Order order) except *
    cdef inline void _update(self, Order order) except *
    cdef inline void _delete(self, Order order) except *
    cdef inline void _check_integrity(self) except *

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


cdef class L3OrderBook(OrderBook):
    pass


cdef class L2OrderBook(OrderBook):
    cdef inline Order _process_order(self, Order order)
    cdef inline void _remove_if_exists(self, Order order) except *


cdef class L1OrderBook(OrderBook):
    cdef Order _top_bid
    cdef Order _top_ask
    cdef Level _top_bid_level
    cdef Level _top_ask_level

    cpdef void update_top(self, Tick tick) except *
    cdef inline void _update_quote_tick(self, QuoteTick tick)
    cdef inline void _update_trade_tick(self, TradeTick tick)
    cdef inline void _update_bid(self, double price, double size)
    cdef inline void _update_ask(self, double price, double size)
    cdef inline Order _process_order(self, Order order)


cdef class OrderBookData(Data):
    cdef readonly InstrumentId instrument_id
    """The instrument identifier for the order book.\n\n:returns: `InstrumentId`"""
    cdef readonly OrderBookLevel level
    """The order book level (L1, L2, L3).\n\n:returns: `OrderBookLevel (Enum)`"""


cdef class OrderBookSnapshot(OrderBookData):
    cdef readonly list bids
    """The snapshot bids.\n\n:returns: `list`"""
    cdef readonly list asks
    """The snapshot asks.\n\n:returns: `list`"""


cdef class OrderBookDeltas(OrderBookData):
    cdef readonly list deltas
    """The order book deltas.\n\n:returns: `list[OrderBookDelta]`"""


cdef class OrderBookDelta(OrderBookData):
    cdef readonly OrderBookDeltaType type
    """The type of change (ADD, UPDATED, DELETE).\n\n:returns: `OrderBookDeltaType (Enum)`"""
    cdef readonly Order order
    """The order to apply.\n\n:returns: `Order`"""
