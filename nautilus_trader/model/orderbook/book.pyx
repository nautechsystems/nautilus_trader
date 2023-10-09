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

import pickle
from operator import itemgetter

import pandas as pd

from libc.stdint cimport INT64_MAX
from libc.stdint cimport INT64_MIN
from libc.stdint cimport int64_t
from libc.stdint cimport uint8_t
from libc.stdint cimport uint64_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.data cimport Data
from nautilus_trader.core.rust.core cimport CVec
from nautilus_trader.core.rust.model cimport BookOrder_t
from nautilus_trader.core.rust.model cimport Level_API
from nautilus_trader.core.rust.model cimport OrderBook_API
from nautilus_trader.core.rust.model cimport Price_t
from nautilus_trader.core.rust.model cimport Quantity_t
from nautilus_trader.core.rust.model cimport book_order_from_raw
from nautilus_trader.core.rust.model cimport level_clone
from nautilus_trader.core.rust.model cimport level_drop
from nautilus_trader.core.rust.model cimport level_exposure
from nautilus_trader.core.rust.model cimport level_orders
from nautilus_trader.core.rust.model cimport level_price
from nautilus_trader.core.rust.model cimport level_size
from nautilus_trader.core.rust.model cimport orderbook_add
from nautilus_trader.core.rust.model cimport orderbook_apply_delta
from nautilus_trader.core.rust.model cimport orderbook_asks
from nautilus_trader.core.rust.model cimport orderbook_best_ask_price
from nautilus_trader.core.rust.model cimport orderbook_best_ask_size
from nautilus_trader.core.rust.model cimport orderbook_best_bid_price
from nautilus_trader.core.rust.model cimport orderbook_best_bid_size
from nautilus_trader.core.rust.model cimport orderbook_bids
from nautilus_trader.core.rust.model cimport orderbook_book_type
from nautilus_trader.core.rust.model cimport orderbook_check_integrity
from nautilus_trader.core.rust.model cimport orderbook_clear
from nautilus_trader.core.rust.model cimport orderbook_clear_asks
from nautilus_trader.core.rust.model cimport orderbook_clear_bids
from nautilus_trader.core.rust.model cimport orderbook_count
from nautilus_trader.core.rust.model cimport orderbook_delete
from nautilus_trader.core.rust.model cimport orderbook_get_avg_px_for_quantity
from nautilus_trader.core.rust.model cimport orderbook_get_quantity_for_price
from nautilus_trader.core.rust.model cimport orderbook_has_ask
from nautilus_trader.core.rust.model cimport orderbook_has_bid
from nautilus_trader.core.rust.model cimport orderbook_instrument_id
from nautilus_trader.core.rust.model cimport orderbook_midpoint
from nautilus_trader.core.rust.model cimport orderbook_new
from nautilus_trader.core.rust.model cimport orderbook_pprint_to_cstr
from nautilus_trader.core.rust.model cimport orderbook_reset
from nautilus_trader.core.rust.model cimport orderbook_sequence
from nautilus_trader.core.rust.model cimport orderbook_simulate_fills
from nautilus_trader.core.rust.model cimport orderbook_spread
from nautilus_trader.core.rust.model cimport orderbook_ts_last
from nautilus_trader.core.rust.model cimport orderbook_update
from nautilus_trader.core.rust.model cimport orderbook_update_quote_tick
from nautilus_trader.core.rust.model cimport orderbook_update_trade_tick
from nautilus_trader.core.rust.model cimport vec_fills_drop
from nautilus_trader.core.rust.model cimport vec_levels_drop
from nautilus_trader.core.rust.model cimport vec_orders_drop
from nautilus_trader.core.string cimport cstr_to_pystr
from nautilus_trader.model.data.book cimport BookOrder
from nautilus_trader.model.data.tick cimport TradeTick
from nautilus_trader.model.enums_c cimport BookAction
from nautilus_trader.model.enums_c cimport BookType
from nautilus_trader.model.enums_c cimport OrderSide
from nautilus_trader.model.enums_c cimport OrderType
from nautilus_trader.model.enums_c cimport book_type_to_str
from nautilus_trader.model.enums_c cimport order_side_to_str
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class OrderBook(Data):
    """
    Provides an order book which can handle L1/L2/L3 granularity data.
    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        BookType book_type,
    ):
        self._mem = orderbook_new(
            instrument_id._mem,
            book_type,
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__} {book_type_to_str(self.book_type)}\n"
            f"instrument: {self.instrument_id}\n"
            f"sequence: {self.sequence}\n"
            f"ts_last: {self.ts_last}\n"
            f"count: {self.count}\n"
            f"{self.pprint()}"
        )

    def __getstate__(self):
        cdef list orders = [o for level in self.bids() + self.asks() for o in level.orders()]
        return (
            self.instrument_id.value,
            self.book_type.value,
            self.ts_last,
            self.sequence,
            pickle.dumps(orders),
        )

    def __setstate__(self, state):
        cdef InstrumentId instrument_id = InstrumentId.from_str_c(state[0])
        self._mem = orderbook_new(
            instrument_id._mem,
            state[1],
        )
        cdef int64_t ts_last = state[2]
        cdef int64_t sequence = state[3]
        cdef list orders = pickle.loads(state[4])

        cdef int64_t i
        for i in range(len(orders)):
            self.add(orders[i], ts_last, sequence)

    @property
    def instrument_id(self) -> InstrumentId:
        """
        Return the books instrument ID.

        Returns
        -------
        InstrumentId

        """
        return InstrumentId.from_mem_c(orderbook_instrument_id(&self._mem))

    @property
    def book_type(self) -> BookType:
        """
        Return the order book type.

        Returns
        -------
        BookType

        """
        return <BookType>orderbook_book_type(&self._mem)

    @property
    def sequence(self) -> int:
        """
        Return the last sequence number for the book.

        Returns
        -------
        int

        """
        return orderbook_sequence(&self._mem)

    @property
    def ts_event(self) -> int:
        """
        The UNIX timestamp (nanoseconds) when the data event occurred.

        Returns
        -------
        int

        """
        return orderbook_ts_last(&self._mem)

    @property
    def ts_init(self) -> int:
        """
        The UNIX timestamp (nanoseconds) when the object was initialized.

        Returns
        -------
        int

        """
        return orderbook_ts_last(&self._mem)

    @property
    def ts_last(self) -> int:
        """
        Return the UNIX timestamp (nanoseconds) when the order book was last updated.

        Returns
        -------
        int

        """
        return orderbook_ts_last(&self._mem)

    @property
    def count(self) -> int:
        """
        Return the books update count.

        Returns
        -------
        int

        """
        return orderbook_count(&self._mem)

    cpdef void reset(self):
        """
        Reset the order book (clear all stateful values).
        """
        orderbook_reset(&self._mem)

    cpdef void add(self, BookOrder order, uint64_t ts_event, uint64_t sequence=0):
        """
        Add the given order to the book.

        Parameters
        ----------
        order : BookOrder
            The order to add.
        sequence : uint64, default 0
            The unique sequence number for the update. If default 0 then will increment the `sequence`.

        """
        Condition.not_none(order, "order")

        orderbook_add(&self._mem, order._mem, ts_event, sequence)

    cpdef void update(self, BookOrder order, uint64_t ts_event, uint64_t sequence=0):
        """
        Update the given order in the book.

        Parameters
        ----------
        order : Order
            The order to update.
        sequence : uint64, default 0
            The unique sequence number for the update. If default 0 then will increment the `sequence`.

        """
        Condition.not_none(order, "order")

        orderbook_update(&self._mem, order._mem, ts_event, sequence)

    cpdef void delete(self, BookOrder order, uint64_t ts_event, uint64_t sequence=0):
        """
        Cancel the given order in the book.

        Parameters
        ----------
        order : Order
            The order to delete.
        sequence : uint64, default 0
            The unique sequence number for the update. If default 0 then will increment the `sequence`.

        """
        Condition.not_none(order, "order")

        orderbook_delete(&self._mem, order._mem, ts_event, sequence)

    cpdef void clear(self, uint64_t ts_event, uint64_t sequence=0):
        """
        Clear the entire order book.
        """
        orderbook_clear(&self._mem, ts_event, sequence)

    cpdef void clear_bids(self, uint64_t ts_event, uint64_t sequence=0):
        """
        Clear the bids from the order book.
        """
        orderbook_clear_bids(&self._mem, ts_event, sequence)

    cpdef void clear_asks(self, uint64_t ts_event, uint64_t sequence=0):
        """
        Clear the asks from the order book.
        """
        orderbook_clear_asks(&self._mem, ts_event, sequence)

    cpdef void apply_delta(self, OrderBookDelta delta):
        """
        Apply the order book delta.

        Parameters
        ----------
        delta : OrderBookDelta
            The delta to apply.

        Raises
        ------
        ValueError
            If `delta.book_type` is not equal to `self.type`.

        """
        Condition.not_none(delta, "delta")

        orderbook_apply_delta(&self._mem, delta._mem)

    cpdef void apply_deltas(self, OrderBookDeltas deltas):
        """
        Apply the bulk deltas to the order book.

        Parameters
        ----------
        deltas : OrderBookDeltas
            The deltas to apply.

        """
        Condition.not_none(deltas, "deltas")

        cdef OrderBookDelta delta
        for delta in deltas.deltas:
            self.apply_delta(delta)

    cpdef void apply(self, Data data):
        """
        Apply the given data to the order book.

        Parameters
        ----------
        delta : OrderBookDelta, OrderBookDeltas
            The data to apply.

        """
        if isinstance(data, OrderBookDeltas):
            self.apply_deltas(deltas=data)
        elif isinstance(data, OrderBookDelta):
            self.apply_delta(delta=data)
        else:  # pragma: no-cover (design time error)
            raise RuntimeError(f"invalid order book data type, was {type(data)}")  # pragma: no-cover (design time error)

    cpdef void check_integrity(self):
        """
        Check book integrity.

        For now will panic from Rust and print the error message to stdout.

        For all order books:
        - The bid side price should not be greater than the ask side price.

        """
        orderbook_check_integrity(&self._mem)

    cpdef list bids(self):
        """
        Return the bid levels for the order book.

        Returns
        -------
        list[Level]
            Sorted in descending order of price.

        """
        cdef CVec raw_levels_vec = orderbook_bids(&self._mem)
        cdef Level_API* raw_levels = <Level_API*>raw_levels_vec.ptr

        cdef list levels = []

        cdef:
            uint64_t i
        for i in range(raw_levels_vec.len):
            levels.append(Level.from_mem_c(raw_levels[i]))

        vec_levels_drop(raw_levels_vec)

        return levels

    cpdef list asks(self):
        """
        Return the bid levels for the order book.

        Returns
        -------
        list[Level]
            Sorted in ascending order of price.

        """
        cdef CVec raw_levels_vec = orderbook_asks(&self._mem)
        cdef Level_API* raw_levels = <Level_API*>raw_levels_vec.ptr

        cdef list levels = []

        cdef:
            uint64_t i
        for i in range(raw_levels_vec.len):
            levels.append(Level.from_mem_c(raw_levels[i]))

        vec_levels_drop(raw_levels_vec)

        return levels

    cpdef best_bid_price(self):
        """
        Return the best bid price in the book (if no bids then returns ``None``).

        Returns
        -------
        double

        """
        if not orderbook_has_bid(&self._mem):
            return None

        return Price.from_mem_c(orderbook_best_bid_price(&self._mem))

    cpdef best_ask_price(self):
        """
        Return the best ask price in the book (if no asks then returns ``None``).

        Returns
        -------
        double

        """
        if not orderbook_has_ask(&self._mem):
            return None

        return Price.from_mem_c(orderbook_best_ask_price(&self._mem))

    cpdef best_bid_size(self):
        """
        Return the best bid size in the book (if no bids then returns ``None``).

        Returns
        -------
        double

        """
        if not orderbook_has_bid(&self._mem):
            return None

        return Quantity.from_mem_c(orderbook_best_bid_size(&self._mem))

    cpdef best_ask_size(self):
        """
        Return the best ask size in the book (if no asks then returns ``None``).

        Returns
        -------
        double or ``None``

        """
        if not orderbook_has_ask(&self._mem):
            return None

        return Quantity.from_mem_c(orderbook_best_ask_size(&self._mem))

    cpdef spread(self):
        """
        Return the top of book spread (if no bids or asks then returns ``None``).

        Returns
        -------
        double or ``None``

        """
        if not orderbook_has_bid(&self._mem) or not orderbook_has_ask(&self._mem):
            return None

        return orderbook_spread(&self._mem)

    cpdef midpoint(self):
        """
        Return the mid point (if no market exists then returns ``None``).

        Returns
        -------
        double or ``None``

        """
        if not orderbook_has_bid(&self._mem) or not orderbook_has_ask(&self._mem):
            return None

        return orderbook_midpoint(&self._mem)

    cpdef double get_avg_px_for_quantity(self, Quantity quantity, OrderSide order_side):
        """
        Return the average price expected for the given `quantity` based on the current state
        of the order book.

        Parameters
        ----------
        quantity : Quantity
            The quantity for the calculation.
        order_side : OrderSide
            The order side for the calculation.

        Returns
        -------
        double

        Raises
        ------
        ValueError
            If `order_side` is equal to ``NO_ORDER_SIDE``

        Warnings
        --------
        If no average price can be calculated then will return 0.0 (zero).

        """
        Condition.not_none(quantity, "quantity")
        Condition.not_equal(order_side, OrderSide.NO_ORDER_SIDE, "order_side", "NO_ORDER_SIDE")

        return orderbook_get_avg_px_for_quantity(&self._mem, quantity._mem, order_side)

    cpdef double get_quantity_for_price(self, Price price, OrderSide order_side):
        """
        Return the current total quantity for the given `price` based on the current state
        of the order book.

        Parameters
        ----------
        price : Price
            The quantity for the calculation.
        order_side : OrderSide
            The order side for the calculation.

        Returns
        -------
        double

        Raises
        ------
        ValueError
            If `order_side` is equal to ``NO_ORDER_SIDE``

        """
        Condition.not_none(price, "price")
        Condition.not_equal(order_side, OrderSide.NO_ORDER_SIDE, "order_side", "NO_ORDER_SIDE")

        return orderbook_get_quantity_for_price(&self._mem, price._mem, order_side)

    cpdef list simulate_fills(self, Order order, uint8_t price_prec, bint is_aggressive):
        """
        Simulate filling the book with the given order.

        Parameters
        ----------
        order : Order
            The order to simulate fills for.
        price_prec : uint8_t
            The price precision for the fills.

        """
        cdef int64_t price_raw
        cdef Price price
        if is_aggressive:
            price_raw = INT64_MAX if order.side == OrderSide.BUY else INT64_MIN
        else:
            price = order.price
            price_raw = price._mem.raw

        cdef BookOrder_t submit_order = book_order_from_raw(
            order.side,
            price_raw,
            price_prec,
            order.leaves_qty._mem.raw,
            order.quantity._mem.precision,
            0,
        )

        cdef CVec raw_fills_vec = orderbook_simulate_fills(&self._mem, submit_order)
        cdef (Price_t, Quantity_t)* raw_fills = <(Price_t, Quantity_t)*>raw_fills_vec.ptr
        cdef list fills = []

        cdef:
            uint64_t i
            (Price_t, Quantity_t) raw_fill
            Price fill_price
            Quantity fill_size
        for i in range(raw_fills_vec.len):
            raw_fill = raw_fills[i]
            fill_price = Price.from_mem_c(raw_fill[0])
            fill_size = Quantity.from_mem_c(raw_fill[1])
            fills.append((fill_price, fill_size))

        vec_fills_drop(raw_fills_vec)

        return fills

    cpdef void update_quote_tick(self, QuoteTick tick):
        """
        Update the order book with the given quote tick.

        Parameters
        ----------
        tick : QuoteTick
            The quote tick to update with.

        """
        orderbook_update_quote_tick(&self._mem, &tick._mem)

    cpdef void update_trade_tick(self, TradeTick tick):
        """
        Update the order book with the given trade tick.

        Parameters
        ----------
        tick : TradeTick
            The trade tick to update with.

        """
        orderbook_update_trade_tick(&self._mem, &tick._mem)

    cpdef str pprint(self, int num_levels=3):
        """
        Return a string representation of the order book in a human-readable table format.

        Parameters
        ----------
        num_levels : int
            The number of levels to include.

        Returns
        -------
        str

        """
        return cstr_to_pystr(orderbook_pprint_to_cstr(&self._mem, num_levels))


cdef class Level:
    """
    Represents a read-only order book `Level`.

    A price level on one side of the order book with one or more individual orders.

    Parameters
    ----------
    price : Price
        The price for the level.
    orders : list[BookOrder]
        The orders for the level.

    Raises
    ------
    ValueError
        If `orders` is empty.
    """

    def __del__(self) -> None:
        if self._mem._0 != NULL:
            level_drop(self._mem)

    def __eq__(self, Level other) -> bool:
        return self.price._mem.raw == other.price._mem.raw

    def __lt__(self, Level other) -> bool:
        return self.price._mem.raw < other.price._mem.raw

    def __le__(self, Level other) -> bool:
        return self.price._mem.raw <= other.price._mem.raw

    def __gt__(self, Level other) -> bool:
        return self.price._mem.raw > other.price._mem.raw

    def __ge__(self, Level other) -> bool:
        return self.price._mem.raw >= other.price._mem.raw

    def __repr__(self) -> str:
        return f"Level(price={self.price}, orders={self.orders()})"

    @property
    def price(self) -> Price:
        """
        Return the price for the level.

        Returns
        -------
        Price

        """
        return Price.from_mem_c(level_price(&self._mem))

    @staticmethod
    cdef Level from_mem_c(Level_API mem):
        cdef Level level = Level.__new__(Level)
        level._mem = level_clone(&mem)
        return level

    cpdef list orders(self):
        """
        Return the orders for the level.

        Returns
        -------
        list[BookOrder]

        """
        cdef CVec raw_orders_vec = level_orders(&self._mem)
        cdef BookOrder_t* raw_orders = <BookOrder_t*>raw_orders_vec.ptr

        cdef list book_orders = []

        cdef:
            uint64_t i
        for i in range(raw_orders_vec.len):
            book_orders.append(BookOrder.from_mem_c(raw_orders[i]))

        vec_orders_drop(raw_orders_vec)

        return book_orders

    cpdef double size(self):
        """
        Return the size at this level.

        Returns
        -------
        double

        """
        return level_size(&self._mem)

    cpdef double exposure(self):
        """
        Return the exposure at this level (price * volume).

        Returns
        -------
        double

        """
        return level_exposure(&self._mem)
