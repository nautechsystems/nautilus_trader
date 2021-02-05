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

from decimal import Decimal

from libc.stdint cimport uint64_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model cimport order_book_rs
from nautilus_trader.model.order_book_rs cimport OrderBookEntry


cdef class OrderBook:
    """
    Represents an order book.
    """

    def __init__(
        self,
        Symbol symbol not None,
        int level,
        int depth,
        int price_precision,
        int size_precision,
        list bids not None,
        list asks not None,
        long update_id,
        long timestamp,
    ):
        """
        Initialize a new instance of the `OrderBook` class.

        Parameters
        ----------
        symbol : Symbol
            The order book symbol.
        level : int
            The order book data level (L1, L2, L3).
        bids : double[:, :]
            The initial bids for the order book.
        asks : double[:, :]
            The initial asks for the order book.
        price_precision : int
            The precision for the order book prices.
        size_precision : int
            The precision for the order book quantities.
        timestamp : long
            The initial order book update timestamp (Unix time).

        Raises
        ------
        ValueError
            If level is not in range 1-3.

        """
        Condition.in_range_int(level, 1, 3, "level")
        Condition.not_negative(price_precision, "price_precision")
        Condition.not_negative(size_precision, "size_precision")

        self.symbol = symbol
        self.level = level
        self.depth = depth
        self.price_precision = price_precision
        self.size_precision = size_precision

        self.apply_snapshot(bids, asks, update_id, timestamp)

    def __cinit__(
        self,
        Symbol symbol not None,
        int level,
        int depth,
        int price_precision,
        int size_precision,
        list bids not None,
        list asks not None,
        uint64_t update_id,
        uint64_t timestamp,
    ):
        self._book = order_book_rs.new(timestamp)

    def __str__(self) -> str:
        return (f"{self.symbol}, "
                f"level={self.level}, "
                f"depth={self.depth}, "
                f"last_update_id={self._book.last_update_id}, "
                f"timestamp={self._book.timestamp}")

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self})"

    cpdef list bids(self):
        """
        Return the current bid entries as doubles.

        Returns
        -------
        list[double, double]

        """
        cdef OrderBookEntry entry
        return [[row.price, row.qty] for row in self._book._bid_book if row.qty != 0]

    cpdef list asks(self):
        """
        Return the current bid entries as doubles.

        Returns
        -------
        list[double, double]

        """
        cdef OrderBookEntry entry
        return [[row.price, row.qty] for row in self._book._ask_book if row.qty != 0]  # noqa

    cpdef list bids_as_decimals(self):
        """
        Return the bids with prices and quantities as decimals.

        The Decimal type is the built-in `decimal.Decimal`.

        Returns
        -------
        list[[Decimal, Decimal]]
        """
        cdef OrderBookEntry entry
        return [
            [Decimal(f"{row.price:.{self.price_precision}f}"), Decimal(f"{row.qty:.{self.size_precision}f}")]
            for row in self._book._bid_book if row.qty != 0  # noqa (access to protected member ok here)
        ]

    cpdef list asks_as_decimals(self):
        """
        Return the asks with prices and quantities as decimals.

        The Decimal type is the built-in `decimal.Decimal`.

        Returns
        -------
        list[[Decimal, Decimal]]
        """
        cdef OrderBookEntry entry
        return [
            [Decimal(f"{row.price:.{self.price_precision}f}"), Decimal(f"{row.qty:.{self.size_precision}f}")]
            for row in self._book._ask_book if row.qty != 0  # noqa (access to protected member ok here)
        ]

    cpdef double spread(self):
        """
        Return the top of book spread.

        Returns
        -------
        double

        """
        return order_book_rs.spread(&self._book)

    cpdef double best_bid_price(self):
        """
        Return the current best bid price.

        Returns
        -------
        double

        """
        return self._book.best_bid_price

    cpdef double best_ask_price(self):
        """
        Return the current best ask price.

        Returns
        -------
        double

        """
        return self._book.best_ask_price

    cpdef double best_bid_qty(self):
        """
        Return the current size at the best bid.

        Returns
        -------
        double

        """
        return self._book.best_bid_qty

    cpdef double best_ask_qty(self):
        """
        Return the current size at the best ask.

        Returns
        -------
        double

        """
        return self._book.best_ask_qty

    cpdef double buy_price_for_qty(self, double qty) except *:
        """
        Return the predicted price the for given buy quantity.

        Parameters
        ----------
        qty : double
            The buy quantity.

        Returns
        -------
        double

        """
        return order_book_rs.buy_price_for_qty(&self._book, qty)

    cpdef double buy_qty_for_price(self, double price) except *:
        """
        Return the predicted quantity the for given buy price.

        Parameters
        ----------
        price : double
            The buy price.

        Returns
        -------
        double

        """
        return order_book_rs.buy_qty_for_price(&self._book, price)

    cpdef double sell_price_for_qty(self, double qty) except *:
        """
        Return the predicted price the for given sell quantity.

        Parameters
        ----------
        qty : double
            The sell quantity.

        Returns
        -------
        double

        """
        return order_book_rs.sell_price_for_qty(&self._book, qty)

    cpdef double sell_qty_for_price(self, double price) except *:
        """
        Return the predicted quantity the for given sell price.

        Parameters
        ----------
        price : double
            The sell price.

        Returns
        -------
        double

        """
        return order_book_rs.sell_qty_for_price(&self._book, price)

    cpdef uint64_t timestamp(self):
        """
        Return the last updated timestamp.

        Returns
        -------
        unsigned long

        """
        return self._book.timestamp

    cpdef uint64_t last_update_id(self):
        """
        Return the last update identifier.

        Returns
        -------
        unsigned long

        """
        return self._book.last_update_id

    cpdef void apply_snapshot(
        self,
        list bids,
        list asks,
        uint64_t update_id,
        uint64_t timestamp,
    ) except *:
        """
        Apply the snapshot with the given parameters.

        Parameters
        ----------
        bids : list[double, double]
            The bid side entries.
        asks : list[double, double]
            The ask side entries.
        update_id : unsigned long
            The identifier of this update.
        timestamp : unsigned long
            The timestamp of this update.

        """
        order_book_rs.reset(&self._book)
        [order_book_rs.apply_bid_diff(&self._book, order_book_rs.new_entry(row[0], row[1], update_id), timestamp) for row in bids]
        [order_book_rs.apply_ask_diff(&self._book, order_book_rs.new_entry(row[0], row[1], update_id), timestamp) for row in asks]

    cpdef void apply_bid_diff(
        self,
        double price,
        double qty,
        uint64_t update_id,
        uint64_t timestamp,
    ) except *:
        """
        Apply the given bid side difference.

        Parameters
        ----------
        price : double
            The price for the entry.
        qty : double
            The quantity for the entry.
        update_id : unsigned long
            The identifier of this update.
        timestamp : unsigned long
            The timestamp of this update.

        """
        cdef OrderBookEntry entry = order_book_rs.new_entry(price, qty, update_id)
        order_book_rs.apply_bid_diff(&self._book, entry, timestamp)

    cpdef void apply_ask_diff(
        self,
        double price,
        double qty,
        uint64_t update_id,
        uint64_t timestamp,
    ) except *:
        """
        Apply the given ask side difference.

        Parameters
        ----------
        price : double
            The price for the entry.
        qty : double
            The quantity for the entry.
        update_id : unsigned long
            The identifier of this update.
        timestamp : unsigned long
            The timestamp of this update.

        """
        cdef OrderBookEntry entry = order_book_rs.new_entry(price, qty, update_id)
        order_book_rs.apply_ask_diff(&self._book, entry, timestamp)
