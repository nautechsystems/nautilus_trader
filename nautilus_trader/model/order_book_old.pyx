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
        InstrumentId instrument_id not None,
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
        instrument_id : InstrumentId
            The order book instrument identifier.
        level : int
            The order book data level (L1, L2, L3).
        depth : int
            The depth of the order book.
        bids : list[[double, double]]
            The initial bids for the order book.
        asks : list[[double, double]]
            The initial asks for the order book.
        price_precision : int
            The precision for the order book prices.
        size_precision : int
            The precision for the order book quantities.
        timestamp : long
            The initial update identifier.
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

        self._bids = bids
        self._asks = asks

        self.instrument_id = instrument_id
        self.symbol = instrument_id.symbol
        self.venue = instrument_id.venue
        self.level = level
        self.depth = depth
        self.price_precision = price_precision
        self.size_precision = size_precision
        self.update_id = update_id
        self.timestamp = timestamp

    def __str__(self) -> str:
        return (f"{self.instrument_id}, "
                f"level={self.level}, "
                f"depth={self.depth}, "
                f"last_update_id={self.update_id}, "
                f"timestamp={self.timestamp}")

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self})"

    cpdef list bids(self):
        """
        Return the order book bids.
        Returns
        -------
        double[:, :]
        """
        return self._bids

    cpdef list asks(self):
        """
        Return the order book asks.
        Returns
        -------
        double[:, :]
        """
        return self._asks

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

    cpdef double spread(self):
        """
        Return the top of book spread.
        Returns
        -------
        double
        """
        return self._asks[0][0] - self._bids[0][0]

    cpdef double best_bid_price(self):
        """
        Return the current best bid price.
        Returns
        -------
        double
        """
        return self._bids[0][0]

    cpdef double best_ask_price(self):
        """
        Return the current best ask price.
        Returns
        -------
        double
        """
        return self._asks[0][0]

    cpdef double best_bid_qty(self):
        """
        Return the current size at the best bid.
        Returns
        -------
        double
        """
        return self._bids[0][1]

    cpdef double best_ask_qty(self):
        """
        Return the current size at the best ask.
        Returns
        -------
        double
        """
        return self._asks[0][1]

    cpdef void apply_snapshot(
        self,
        list bids,
        list asks,
        long update_id,
        long timestamp,
    ) except *:
        """
        Apply the snapshot with the given parameters.
        Parameters
        ----------
        bids : list[[double, double]]
            The bid side entries.
        asks : list[[double, double]]
            The ask side entries.
        update_id : unsigned long
            The identifier of this update.
        timestamp : unsigned long
            The timestamp of this update.
        """
        self._bids = bids
        self._asks = asks
        self.update_id = update_id
        self.timestamp = timestamp
