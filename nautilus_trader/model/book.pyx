# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from libc.stdint cimport int64_t
from libc.stdint cimport uint8_t
from libc.stdint cimport uint64_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.data cimport Data
from nautilus_trader.core.rust.core cimport CVec
from nautilus_trader.core.rust.model cimport PRICE_RAW_MAX
from nautilus_trader.core.rust.model cimport PRICE_RAW_MIN
from nautilus_trader.core.rust.model cimport BookAction
from nautilus_trader.core.rust.model cimport BookLevel_API
from nautilus_trader.core.rust.model cimport BookOrder_t
from nautilus_trader.core.rust.model cimport BookType
from nautilus_trader.core.rust.model cimport OrderBook_API
from nautilus_trader.core.rust.model cimport OrderSide
from nautilus_trader.core.rust.model cimport OrderType
from nautilus_trader.core.rust.model cimport Price_t
from nautilus_trader.core.rust.model cimport Quantity_t
from nautilus_trader.core.rust.model cimport book_order_new
from nautilus_trader.core.rust.model cimport level_clone
from nautilus_trader.core.rust.model cimport level_drop
from nautilus_trader.core.rust.model cimport level_exposure
from nautilus_trader.core.rust.model cimport level_orders
from nautilus_trader.core.rust.model cimport level_price
from nautilus_trader.core.rust.model cimport level_side
from nautilus_trader.core.rust.model cimport level_size
from nautilus_trader.core.rust.model cimport orderbook_add
from nautilus_trader.core.rust.model cimport orderbook_apply_delta
from nautilus_trader.core.rust.model cimport orderbook_apply_deltas
from nautilus_trader.core.rust.model cimport orderbook_apply_depth
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
from nautilus_trader.core.rust.model cimport orderbook_delete
from nautilus_trader.core.rust.model cimport orderbook_drop
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
from nautilus_trader.core.rust.model cimport orderbook_update_count
from nautilus_trader.core.rust.model cimport orderbook_update_quote_tick
from nautilus_trader.core.rust.model cimport orderbook_update_trade_tick
from nautilus_trader.core.rust.model cimport vec_drop_book_levels
from nautilus_trader.core.rust.model cimport vec_drop_book_orders
from nautilus_trader.core.rust.model cimport vec_drop_fills
from nautilus_trader.core.string cimport cstr_to_pystr
from nautilus_trader.model.data cimport BookOrder
from nautilus_trader.model.data cimport OrderBookDelta
from nautilus_trader.model.data cimport OrderBookDeltas
from nautilus_trader.model.data cimport OrderBookDepth10
from nautilus_trader.model.data cimport TradeTick
from nautilus_trader.model.functions cimport book_type_to_str
from nautilus_trader.model.functions cimport order_side_to_str
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class OrderBook(Data):
    """
    Provides an order book which can handle L1/L2/L3 granularity data.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the order book.
    book_type : BookType {``L1_MBP``, ``L2_MBP``, ``L3_MBO``}
        The order book type.

    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        BookType book_type,
    ) -> None:
        self._book_type = book_type
        self._mem = orderbook_new(
            instrument_id._mem,
            book_type,
        )

    def __del__(self) -> None:
        if self._mem._0 != NULL:
            orderbook_drop(self._mem)

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__} {book_type_to_str(self.book_type)}\n"
            f"instrument: {self.instrument_id}\n"
            f"sequence: {self.sequence}\n"
            f"ts_last: {self.ts_last}\n"
            f"update_count: {self.update_count}\n"
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
        self._book_type = state[1]
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
        return self._book_type

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
        UNIX timestamp (nanoseconds) when the data event occurred.

        Returns
        -------
        int

        """
        return orderbook_ts_last(&self._mem)

    @property
    def ts_init(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the object was initialized.

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
    def update_count(self) -> int:
        """
        Return the books update count.

        Returns
        -------
        int

        """
        return orderbook_update_count(&self._mem)

    cpdef void reset(self):
        """
        Reset the order book (clear all stateful values).
        """
        orderbook_reset(&self._mem)

    cpdef void add(self, BookOrder order, uint64_t ts_event, uint8_t flags=0, uint64_t sequence=0):
        """
        Add the given order to the book.

        Parameters
        ----------
        order : BookOrder
            The order to add.
        ts_event : uint64_t
            UNIX timestamp (nanoseconds) when the book event occurred.
        flags : uint8_t, default 0
            The record flags bit field, indicating event end and data information.
        sequence : uint64_t, default 0
            The unique sequence number for the update. If default 0 then will increment the `sequence`.

        Raises
        ------
        RuntimeError
            If the book type is L1_MBP.

        """
        Condition.not_none(order, "order")

        if self._book_type == BookType.L1_MBP:
            raise RuntimeError("Invalid book operation: cannot add order for L1_MBP book")

        orderbook_add(&self._mem, order._mem, flags, sequence, ts_event)

    cpdef void update(self, BookOrder order, uint64_t ts_event, uint8_t flags=0, uint64_t sequence=0):
        """
        Update the given order in the book.

        Parameters
        ----------
        order : Order
            The order to update.
        ts_event : uint64_t
            UNIX timestamp (nanoseconds) when the book event occurred.
        flags : uint8_t, default 0
            The record flags bit field, indicating event end and data information.
        sequence : uint64_t, default 0
            The unique sequence number for the update. If default 0 then will increment the `sequence`.

        """
        Condition.not_none(order, "order")

        orderbook_update(&self._mem, order._mem, flags, sequence, ts_event)

    cpdef void delete(self, BookOrder order, uint64_t ts_event, uint8_t flags=0, uint64_t sequence=0):
        """
        Cancel the given order in the book.

        Parameters
        ----------
        order : Order
            The order to delete.
        ts_event : uint64_t
            UNIX timestamp (nanoseconds) when the book event occurred.
        flags : uint8_t, default 0
            The record flags bit field, indicating event end and data information.
        sequence : uint64_t, default 0
            The unique sequence number for the update. If default 0 then will increment the `sequence`.

        """
        Condition.not_none(order, "order")

        orderbook_delete(&self._mem, order._mem, flags, sequence, ts_event)

    cpdef void clear(self, uint64_t ts_event, uint64_t sequence=0):
        """
        Clear the entire order book.
        """
        orderbook_clear(&self._mem, sequence, ts_event)

    cpdef void clear_bids(self, uint64_t ts_event, uint64_t sequence=0):
        """
        Clear the bids from the order book.
        """
        orderbook_clear_bids(&self._mem, sequence, ts_event)

    cpdef void clear_asks(self, uint64_t ts_event, uint64_t sequence=0):
        """
        Clear the asks from the order book.
        """
        orderbook_clear_asks(&self._mem, sequence, ts_event)

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

        orderbook_apply_delta(&self._mem, &delta._mem)

    cpdef void apply_deltas(self, OrderBookDeltas deltas):
        """
        Apply the bulk deltas to the order book.

        Parameters
        ----------
        deltas : OrderBookDeltas
            The deltas to apply.

        """
        Condition.not_none(deltas, "deltas")

        orderbook_apply_deltas(&self._mem, &deltas._mem)

    cpdef void apply_depth(self, OrderBookDepth10 depth):
        """
        Apply the depth update to the order book.

        Parameters
        ----------
        depth : OrderBookDepth10
            The depth update to apply.

        """
        Condition.not_none(depth, "depth")

        orderbook_apply_depth(&self._mem, &depth._mem)

    cpdef void apply(self, Data data):
        """
        Apply the given data to the order book.

        Parameters
        ----------
        delta : OrderBookDelta, OrderBookDeltas
            The data to apply.

        """
        if isinstance(data, OrderBookDelta):
            self.apply_delta(data)
        elif isinstance(data, OrderBookDeltas):
            self.apply_deltas(data)
        elif isinstance(data, OrderBookDepth10):
            self.apply_depth(data)
        else:  # pragma: no-cover (design time error)
            raise RuntimeError(f"invalid order book data type, was {type(data)}")  # pragma: no-cover (design time error)

    cpdef void check_integrity(self):
        """
        Check book integrity.

        For all order books:
        - The bid side price should not be greater than the ask side price.

        Raises
        ------
        RuntimeError
            If book integrity check fails.

        """
        if not orderbook_check_integrity(&self._mem):
            raise RuntimeError(f"Integrity error: orders in cross [{self.best_bid_price()} {self.best_ask_price()}]")

    cpdef list bids(self):
        """
        Return the bid levels for the order book.

        Returns
        -------
        list[BookLevel]
            Sorted in descending order of price.

        """
        cdef CVec raw_levels_vec = orderbook_bids(&self._mem)
        cdef BookLevel_API* raw_levels = <BookLevel_API*>raw_levels_vec.ptr

        cdef list levels = []

        cdef:
            uint64_t i
        for i in range(raw_levels_vec.len):
            levels.append(BookLevel.from_mem_c(raw_levels[i]))

        vec_drop_book_levels(raw_levels_vec)

        return levels

    cpdef list asks(self):
        """
        Return the bid levels for the order book.

        Returns
        -------
        list[BookLevel]
            Sorted in ascending order of price.

        """
        cdef CVec raw_levels_vec = orderbook_asks(&self._mem)
        cdef BookLevel_API* raw_levels = <BookLevel_API*>raw_levels_vec.ptr

        cdef list levels = []

        cdef:
            uint64_t i
        for i in range(raw_levels_vec.len):
            levels.append(BookLevel.from_mem_c(raw_levels[i]))

        vec_drop_book_levels(raw_levels_vec)

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
        Return the top-of-book spread (if no bids or asks then returns ``None``).

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

    cpdef list simulate_fills(self, Order order, uint8_t price_prec, uint8_t size_prec, bint is_aggressive):
        """
        Simulate filling the book with the given order.

        Parameters
        ----------
        order : Order
            The order to simulate fills for.
        price_prec : uint8_t
            The price precision for the fills.
        size_prec : uint8_t
            The size precision for the fills (based on the instrument definition).
        is_aggressive : bool
            If the order is an aggressive liquidity taking order.

        """
        Condition.not_none(order, "order")

        if order.leaves_qty._mem.precision != size_prec:
            raise RuntimeError(
                f"Invalid size precision for order leaves quantity {order.leaves_qty._mem.precision} "
                f"when instrument size precision is {size_prec}. "
                f"Check order quantity precision matches the {order.instrument.id} instrument"
            )

        cdef Price order_price
        cdef Price_t price
        price.precision = price_prec
        if is_aggressive:
            price.raw = PRICE_RAW_MAX if order.side == OrderSide.BUY else PRICE_RAW_MIN
        else:
            order_price = order.price
            price.raw = order_price._mem.raw

        cdef BookOrder_t submit_order = book_order_new(
            order.side,
            price,
            order.leaves_qty._mem,
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
            if fill_price.precision != price_prec:
                raise RuntimeError(f"{fill_price.precision=} did not match instrument {price_prec=}")
            if fill_size.precision != size_prec:
                raise RuntimeError(f"{fill_size.precision=} did not match instrument {size_prec=}")

        vec_drop_fills(raw_fills_vec)

        return fills

    cpdef void update_quote_tick(self, QuoteTick tick):
        """
        Update the order book with the given quote tick.

        This operation is only valid for ``L1_MBP`` books maintaining a top level.

        Parameters
        ----------
        tick : QuoteTick
            The quote tick to update with.

        Raises
        ------
        RuntimeError
            If `book_type` is not ``L1_MBP``.

        """
        if self._book_type != BookType.L1_MBP:
            raise RuntimeError(
                "Invalid book operation: "
                f"cannot update with quote for {book_type_to_str(self.book_type)} book",
            )

        orderbook_update_quote_tick(&self._mem, &tick._mem)

    cpdef void update_trade_tick(self, TradeTick tick):
        """
        Update the order book with the given trade tick.

        Parameters
        ----------
        tick : TradeTick
            The trade tick to update with.

        Raises
        ------
        RuntimeError
            If `book_type` is not ``L1_MBP``.

        """
        if self._book_type != BookType.L1_MBP:
            raise RuntimeError(
                "Invalid book operation: "
                f"cannot update with trade for {book_type_to_str(self.book_type)} book",
            )

        orderbook_update_trade_tick(&self._mem, &tick._mem)

    cpdef QuoteTick to_quote_tick(self):
        """
        Return a `QuoteTick` created from the top of book levels.

        Returns ``None`` when the top-of-book bid or ask is missing or invalid
        (zero size).

        Returns
        -------
        QuoteTick or ``None``

        """
        # Get the best bid and ask prices and sizes
        cdef Price bid_price = self.best_bid_price()
        cdef Price ask_price = self.best_ask_price()

        if bid_price is None or ask_price is None:
            return None

        cdef Quantity bid_size = self.best_bid_size()
        cdef Quantity ask_size = self.best_ask_size()

        if bid_size is None or ask_size is None:
            return None

        # Check for zero sizes
        if bid_size._mem.raw == 0 or ask_size._mem.raw == 0:
            return None

        return QuoteTick(
            instrument_id=self.instrument_id,
            bid_price=bid_price,
            ask_price=ask_price,
            bid_size=bid_size,
            ask_size=ask_size,
            ts_event=self.ts_last,
            ts_init=self.ts_last,
        )

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


cdef class BookLevel:
    """
    Represents an order book price level.

    A price level on one side of the order book with one or more individual orders.

    This class is read-only and cannot be initialized from Python.

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

    def __eq__(self, BookLevel other) -> bool:
        return self.price._mem.raw == other.price._mem.raw

    def __lt__(self, BookLevel other) -> bool:
        return self.price._mem.raw < other.price._mem.raw

    def __le__(self, BookLevel other) -> bool:
        return self.price._mem.raw <= other.price._mem.raw

    def __gt__(self, BookLevel other) -> bool:
        return self.price._mem.raw > other.price._mem.raw

    def __ge__(self, BookLevel other) -> bool:
        return self.price._mem.raw >= other.price._mem.raw

    def __repr__(self) -> str:
        return f"BookLevel(price={self.price}, orders={self.orders()})"

    @property
    def side(self) -> OrderSide:
        """
        Return the side for the level.

        Returns
        -------
        OrderSide

        """
        return <OrderSide>level_side(&self._mem)


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
    cdef BookLevel from_mem_c(BookLevel_API mem):
        cdef BookLevel level = BookLevel.__new__(BookLevel)
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

        vec_drop_book_orders(raw_orders_vec)

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


def py_should_handle_own_book_order(Order order) -> bool:
    return should_handle_own_book_order(order)
