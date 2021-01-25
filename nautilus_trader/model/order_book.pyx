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
        bids : list[tuple(Price, Quantity)]
            The bids in the order book snapshot.
        asks : list[tuple(Price, Quantity)]
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
