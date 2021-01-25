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

from cpython.datetime cimport datetime
from decimal import Decimal


cdef class OrderBook:
    """
    Represents an order book snapshot.
    """

    def __init__(
        self,
        Symbol symbol not None,
        int level,
        list bids not None,
        list asks not None,
        datetime timestamp,
    ):
        """
        Initialize a new instance of the `OrderBook` class.

        Parameters
        ----------
        symbol : Symbol
            The order book symbol.
        level : int
            The order book data level (L2, L3).
        bids : list[(Decimal, Decimal)]
            The bids for the order book snapshot.
        asks : list[(Decimal, Decimal)]
            The asks in the order book snapshot.
        timestamp : datetime
            The order book snapshot timestamp (UTC).

        """
        self.symbol = symbol
        self.level = level
        self.bids = bids
        self.asks = asks
        self.timestamp = timestamp

    def __str__(self) -> str:
        return (f"{self.symbol},"
                f"bids={self.bids},"
                f"asks={self.asks}")

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self})"

    @staticmethod
    cdef OrderBook from_floats(
        Symbol symbol,
        int level,
        list bids,
        list asks,
        int price_precision,
        int size_precision,
        datetime timestamp,
    ):
        """
        Create an order book from the given parameters where bid/ask price,
        quantities are expressed as floating point values.

        Parameters
        ----------
        symbol : Symbol
            The order book symbol.
        level : int
            The order book data level (L2, L3).
        bids : list[[float, float]]
            The bid values for the order book.
        asks : list[[float, float]]
            The ask values for the order book.
        price_precision : int
            The precision for the order book prices.
        size_precision : int
            The precision for the order book quantities.
        timestamp : datetime
            The order book snapshot timestamp (UTC).

        Returns
        -------
        OrderBook

        """
        return OrderBook(
            symbol,
            level,
            [(Decimal(f"{entry[0]:.{price_precision}f}"), Decimal(f"{entry[1]:.{size_precision}f}")) for entry in bids],
            [(Decimal(f"{entry[0]:.{price_precision}f}"), Decimal(f"{entry[1]:.{size_precision}f}")) for entry in asks],
            timestamp,
        )

    @staticmethod
    def from_floats_py(
        Symbol symbol,
        int level,
        list bids,
        list asks,
        int price_precision,
        int size_precision,
        datetime timestamp,
    ):
        """
        Create an order book from the given parameters where bid/ask price,
        quantities are expressed as floating point values.

        Parameters
        ----------
        symbol : Symbol
            The order book symbol.
        level : int
            The order book data level (L2, L3).
        bids : list[[float, float]]
            The bid values for the order book.
        asks : list[[float, float]]
            The ask values for the order book.
        price_precision : int
            The precision for the order book prices.
        size_precision : int
            The precision for the order book quantities.
        timestamp : datetime
            The order book snapshot timestamp (UTC).

        Returns
        -------
        OrderBook

        """
        return OrderBook.from_floats(
            symbol,
            level,
            bids,
            asks,
            price_precision,
            size_precision,
            timestamp,
        )
