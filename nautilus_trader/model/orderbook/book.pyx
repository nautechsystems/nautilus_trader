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

from operator import itemgetter

import pandas as pd

from nautilus_trader.model.orderbook.error import BookIntegrityError

from libc.stdint cimport INT64_MAX
from libc.stdint cimport INT64_MIN
from libc.stdint cimport int64_t
from libc.stdint cimport uint8_t
from libc.stdint cimport uint64_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.data cimport Data
from nautilus_trader.core.rust.core cimport CVec
from nautilus_trader.core.rust.model cimport BookOrder_t
from nautilus_trader.core.rust.model cimport Price_t
from nautilus_trader.core.rust.model cimport Quantity_t
from nautilus_trader.core.rust.model cimport book_order_from_raw
from nautilus_trader.core.rust.model cimport instrument_id_clone
from nautilus_trader.core.rust.model cimport orderbook_add
from nautilus_trader.core.rust.model cimport orderbook_apply_delta
from nautilus_trader.core.rust.model cimport orderbook_best_ask_price
from nautilus_trader.core.rust.model cimport orderbook_best_ask_size
from nautilus_trader.core.rust.model cimport orderbook_best_bid_price
from nautilus_trader.core.rust.model cimport orderbook_best_bid_size
from nautilus_trader.core.rust.model cimport orderbook_book_type
from nautilus_trader.core.rust.model cimport orderbook_check_integrity
from nautilus_trader.core.rust.model cimport orderbook_clear
from nautilus_trader.core.rust.model cimport orderbook_clear_asks
from nautilus_trader.core.rust.model cimport orderbook_clear_bids
from nautilus_trader.core.rust.model cimport orderbook_count
from nautilus_trader.core.rust.model cimport orderbook_delete
from nautilus_trader.core.rust.model cimport orderbook_delta_clone
from nautilus_trader.core.rust.model cimport orderbook_drop
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
            instrument_id_clone(&instrument_id._mem),
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
            f"count: {self.count}\n"
            f"{self.pprint()}"
        )

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

        # We have to clone the delta because of the heap allocation `instrument_id`
        orderbook_apply_delta(&self._mem, orderbook_delta_clone(&delta._mem))

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

        Raises
        ------
        BookIntegrityError
            If any check fails.

        """
        orderbook_check_integrity(&self._mem)

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
        cdef tuple[Price_t, Quantity_t]* raw_fills = <tuple[Price_t, Quantity_t]*>raw_fills_vec.ptr
        cdef list fills = []

        cdef:
            uint64_t i
            tuple[Price_t, Quantity_t] raw_fill
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
        Print the order book in a clear format.

        Parameters
        ----------
        num_levels : int
            The number of levels to print.

        Returns
        -------
        str

        """
        return cstr_to_pystr(orderbook_pprint_to_cstr(&self._mem, num_levels))
