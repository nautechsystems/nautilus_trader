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

from nautilus_trader.core.rust.model cimport FIXED_SCALAR
from nautilus_trader.model.data.tick cimport QuoteTick
from nautilus_trader.model.data.tick cimport TradeTick
from nautilus_trader.model.enums_c cimport OrderSide
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.orderbook.book cimport L1OrderBook
from nautilus_trader.model.orderbook.book cimport L2OrderBook
from nautilus_trader.model.orderbook.book cimport L3OrderBook
from nautilus_trader.model.orderbook.data cimport BookOrder


cdef class SimulatedL1OrderBook(L1OrderBook):
    """
    Provides a simulated level 1 order book for backtesting.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the book.
    price_precision : uint8
        The price precision of the books orders.
    size_precision : uint8
        The size precision of the books orders.

    Raises
    ------
    OverflowError
        If `price_precision` is negative (< 0).
    OverflowError
        If `size_precision` is negative (< 0).
    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        uint8_t price_precision,
        uint8_t size_precision,
    ):
        super().__init__(
            instrument_id=instrument_id,
            price_precision=price_precision,
            size_precision=size_precision,
        )

        self._top_bid = None
        self._top_ask = None
        self._top_bid_level = None
        self._top_ask_level = None

    cpdef void add(self, BookOrder order, uint64_t sequence=0) except *:
        """
        NotImplemented (Use `update(order)` for SimulatedOrderBook).
        """
        raise NotImplementedError("Use `update(order)` for L1OrderBook")  # pragma: no cover

    cdef void update_quote_tick(self, QuoteTick tick) except *:
        """
        Update the order book with the given quote tick.

        Parameters
        ----------
        tick : QuoteTick
            The tick to update with.

        """
        self._update_bid(tick._mem.bid.raw / FIXED_SCALAR, tick._mem.bid_size.raw / FIXED_SCALAR)
        self._update_ask(tick._mem.ask.raw / FIXED_SCALAR, tick._mem.ask_size.raw / FIXED_SCALAR)

    cdef void update_trade_tick(self, TradeTick tick) except *:
        """
        Update the order book with the given trade tick.

        Parameters
        ----------
        tick : TradeTick
            The tick to update with.

        """
        cdef double price = tick._mem.price.raw / FIXED_SCALAR
        cdef double size = tick._mem.size.raw / FIXED_SCALAR
        self._update_bid(price, size)
        self._update_ask(price, size)

    cdef void _update_bid(self, double price, double size) except *:
        cdef BookOrder bid
        if self._top_bid is None:
            bid = BookOrder(price, size, OrderSide.BUY, "B")
            self._add(bid, sequence=0)
            self._top_bid = bid
            self._top_bid_level = self.bids.top()
        else:
            self._top_bid_level.price = price
            self._top_bid.price = price
            self._top_bid.size = size

    cdef void _update_ask(self, double price, double size) except *:
        cdef BookOrder ask
        if self._top_ask is None:
            ask = BookOrder(price, size, OrderSide.SELL, "A")
            self._add(ask, sequence=0)
            self._top_ask = ask
            self._top_ask_level = self.asks.top()
        else:
            self._top_ask_level.price = price
            self._top_ask.price = price
            self._top_ask.size = size


cdef class SimulatedL2OrderBook(L2OrderBook):
    """
    Provides a simulated level 2 order book for backtesting.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the book.
    price_precision : uint8
        The price precision of the books orders.
    size_precision : uint8
        The size precision of the books orders.

    Raises
    ------
    OverflowError
        If `price_precision` is negative (< 0).
    OverflowError
        If `size_precision` is negative (< 0).
    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        uint8_t price_precision,
        uint8_t size_precision,
    ):
        super().__init__(
            instrument_id=instrument_id,
            price_precision=price_precision,
            size_precision=size_precision,
        )

        # Placeholder class for implementation


cdef class SimulatedL3OrderBook(L3OrderBook):
    """
    Provides a simulated level 3 order book for backtesting.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the book.
    price_precision : uint8
        The price precision of the books orders.
    size_precision : uint8
        The size precision of the books orders.

    Raises
    ------
    OverflowError
        If `price_precision` is negative (< 0).
    OverflowError
        If `size_precision` is negative (< 0).
    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        uint8_t price_precision,
        uint8_t size_precision,
    ):
        super().__init__(
            instrument_id=instrument_id,
            price_precision=price_precision,
            size_precision=size_precision,
        )

        # Placeholder class for implementation
