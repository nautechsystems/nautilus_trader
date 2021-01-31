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

from nautilus_trader.core.correctness cimport Condition


cdef class OrderBook:
    """
    Represents an order book.
    """

    def __init__(
        self,
        Symbol symbol not None,
        int level,
        int price_precision,
        int size_precision,
        double[:, :] bids not None,
        double[:, :] asks not None,
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
            The initial order book update timestamp (UNIX time).

        Raises
        ------
        ValueError
            If level is not in range 1-3.

        """
        Condition.in_range_int(level, 1, 3, "level")
        Condition.not_negative(price_precision, "price_precision")
        Condition.not_negative(size_precision, "size_precision")

        self._bids = bids
        self._asks = asks

        self.symbol = symbol
        self.level = level
        self.price_precision = price_precision
        self.size_precision = size_precision
        self.timestamp = timestamp

    def __str__(self) -> str:
        return (f"{self.symbol}, "
                f"bids_len={len(self._bids)}, "
                f"asks_len={len(self._asks)}, "
                f"timestamp={self.timestamp}")

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self})"

    cpdef void update(
        self,
        double[:, :] bids,
        double[:, :] asks,
        long timestamp,
    ) except *:
        """
        Update the order book with the given bids, asks and timestamp.

        Parameters
        ----------
        bids : double[:, :]
            The updated bids.
        asks : double[:, :]
            The updated asks.
        timestamp : long
            The update timestamp (UNIX time)

        """
        self._bids = bids
        self._asks = asks
        self.timestamp = timestamp

    cdef double[:, :] bids_c(self):
        """
        Return the order book bids.

        Returns
        -------
        double[:, :]

        """
        return self._bids

    cdef double[:, :] asks_c(self):
        """
        Return the order book asks.

        Returns
        -------
        double[:, :]

        """
        return self._asks

    cpdef list bids(self):
        """
        Return the order book bids.

        Returns
        -------
        double[:, :]

        """
        cdef double[:] entry
        return [[entry[0], entry[1]] for entry in self.bids_c()]

    cpdef list asks(self):
        """
        Return the order book asks.

        Returns
        -------
        list[list[double]]

        """
        cdef double[:] entry
        return [[entry[0], entry[1]] for entry in self.asks_c()]

    cpdef list bids_as_decimals(self):
        """
        Return the bids with prices and quantities as decimals.

        Decimal type is the built-in `decimal.Decimal`.

        Returns
        -------
        list[list[Decimal, Decimal]]

        """
        cdef double[:] entry
        return [[Decimal(f"{entry[0]:.{self.price_precision}f}"), Decimal(f"{entry[1]:.{self.size_precision}f}")] for entry in self._bids]

    cpdef list asks_as_decimals(self):
        """
        Return the asks with prices and quantities as decimals.

        Decimal type is the built-in `decimal.Decimal`.

        Returns
        -------
        list[list[Decimal, Decimal]]

        """
        cdef double[:] entry
        return [[Decimal(f"{entry[0]:.{self.price_precision}f}"), Decimal(f"{entry[1]:.{self.size_precision}f}")] for entry in self._asks]
