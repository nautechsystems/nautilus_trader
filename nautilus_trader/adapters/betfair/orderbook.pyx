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
from tabulate import tabulate

from libc.stdint cimport uint64_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.data.tick cimport TradeTick
from nautilus_trader.model.enums_c cimport BookAction
from nautilus_trader.model.enums_c cimport BookType
from nautilus_trader.model.enums_c cimport OrderSide
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orderbook.data cimport BookOrder
from nautilus_trader.model.orderbook.data cimport OrderBookSnapshot

from nautilus_trader.adapters.betfair.common import BETFAIR_FLOAT_TO_PRICE
from nautilus_trader.adapters.betfair.common import BETFAIR_PRICE_PRECISION
from nautilus_trader.adapters.betfair.common import BETFAIR_QUANTITY_PRECISION
from nautilus_trader.model.orderbook.error import BookIntegrityError

from nautilus_trader.model.orderbook.ladder cimport Ladder
from nautilus_trader.model.orderbook.level cimport Level

from nautilus_trader.adapters.betfair.constants import BETFAIR_PRICE_PRECISION
from nautilus_trader.adapters.betfair.constants import BETFAIR_QUANTITY_PRECISION


cdef class BettingOrderBook:
    """
    The base class for all order books.

    Provides a L1/L2/L3 order book as an `L3OrderBook` which can be proxied to
    `L2OrderBook` or `L1OrderBook` classes.
    """

    def __init__(self, InstrumentId instrument_id not None):
        self.instrument_id = instrument_id
        self.type = BookType.L2_MBP
        self.price_precision = BETFAIR_PRICE_PRECISION
        self.size_precision = BETFAIR_QUANTITY_PRECISION
        self.bids = Ladder(
            reverse=True,
            price_precision=self.price_precision,
            size_precision=self.size_precision,
        )
        self.asks = Ladder(
            reverse=False,
            price_precision=self.price_precision,
            size_precision=self.size_precision,
        )
        self.sequence = 0
        self.count = 0
        self.ts_last = 0

    cpdef void add(self, BookOrder order, uint64_t sequence=0) except*:
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

        self._add(order=order, sequence=sequence)

    cpdef void update(self, BookOrder order, uint64_t sequence=0) except*:
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

        self._update(order=order, sequence=sequence)

    cpdef void delete(self, BookOrder order, uint64_t sequence=0) except*:
        """
        Delete the given order in the book.

        Parameters
        ----------
        order : Order
            The order to delete.
        sequence : uint64, default 0
            The unique sequence number for the update. If default 0 then will increment the `sequence`.

        """
        Condition.not_none(order, "order")

        self._delete(order=order, sequence=sequence)

    cpdef void apply_delta(self, OrderBookDelta delta) except*:
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
        Condition.equal(delta.book_type, self.type, "delta.book_type", "self.type")

        self._apply_delta(delta)

    cpdef void apply_deltas(self, OrderBookDeltas deltas) except*:
        """
        Apply the bulk deltas to the order book.

        Parameters
        ----------
        deltas : OrderBookDeltas
            The deltas to apply.

        Raises
        ------
        ValueError
            If `snapshot.book_type` is not equal to `self.type`.

        """
        Condition.not_none(deltas, "deltas")
        Condition.equal(deltas.book_type, self.type, "deltas.book_type", "self.type")

        cdef OrderBookDelta delta
        for delta in deltas.deltas:
            self._apply_delta(delta)

    cpdef void apply_snapshot(self, OrderBookSnapshot snapshot) except*:
        """
        Apply the bulk snapshot to the order book.

        Parameters
        ----------
        snapshot : OrderBookSnapshot
            The snapshot to apply.

        Raises
        ------
        ValueError
            If `snapshot.book_type` is not equal to `self.type`.

        """
        Condition.not_none(snapshot, "snapshot")
        Condition.equal(snapshot.book_type, self.type, "snapshot.book_type", "self.type")

        self.clear()
        # Use `update` instead of `add` (when book has been cleared they're
        # equivalent) to make work for L1_TBBO Orderbook.
        cdef BookOrder order
        for bid in snapshot.bids:
            order = BookOrder(
                price=bid[0],
                size=bid[1],
                side=OrderSide.BUY
            )
            self.update(order=order, sequence=snapshot.sequence)
        for ask in snapshot.asks:
            order = BookOrder(
                price=ask[0],
                size=ask[1],
                side=OrderSide.SELL
            )
            self.update(order=order, sequence=snapshot.sequence)

        self.ts_last = snapshot.ts_init

    cpdef void apply(self, OrderBookData data) except*:
        """
        Apply the data to the order book.

        Parameters
        ----------
        data : OrderBookData
            The data to apply.

        Raises
        ------
        ValueError
            If `data.level` is not equal to `self.type`.

        """
        Condition(data.book_type, self.type, "data.book_type", "self.type")

        if isinstance(data, OrderBookSnapshot):
            self.apply_snapshot(snapshot=data)
        elif isinstance(data, OrderBookDeltas):
            self.apply_deltas(deltas=data)
        elif isinstance(data, OrderBookDelta):
            self._apply_delta(delta=data)

    cpdef void check_integrity(self) except*:
        """
        Check order book integrity.

        For all order books:
        - The bid side price should not be greater than the ask side price.

        Raises
        ------
        BookIntegrityError
            If any check fails.

        """
        self._check_integrity()

    cpdef void clear_bids(self) except*:
        """
        Clear the bids from the order book.
        """
        self.bids = Ladder(
            reverse=True,
            price_precision=self.price_precision,
            size_precision=self.size_precision,
        )

    cpdef void clear_asks(self) except*:
        """
        Clear the asks from the order book.
        """
        self.asks = Ladder(
            reverse=False,
            price_precision=self.price_precision,
            size_precision=self.size_precision,
        )

    cpdef void clear(self) except*:
        """
        Clear the entire order book.
        """
        self.clear_bids()
        self.clear_asks()

    cdef void _add(self, BookOrder order, uint64_t sequence) except*:
        if order.side == OrderSide.BUY:
            self.bids.add(order=order)
        elif order.side == OrderSide.SELL:
            self.asks.add(order=order)
        self._apply_sequence(sequence)

    cdef void _update(self, BookOrder order, uint64_t sequence) except*:
        if order.side == OrderSide.BUY:
            self.bids.update(order=order)
        elif order.side == OrderSide.SELL:
            self.asks.update(order=order)
        self._apply_sequence(sequence)

    cdef void _delete(self, BookOrder order, uint64_t sequence) except*:
        if order.side == OrderSide.BUY:
            self.bids.delete(order=order)
        elif order.side == OrderSide.SELL:
            self.asks.delete(order=order)
        self._apply_sequence(sequence)

    cdef void _apply_delta(self, OrderBookDelta delta) except*:
        if delta.action == BookAction.ADD:
            self.add(order=delta.order, sequence=delta.sequence)
        elif delta.action == BookAction.UPDATE:
            self.update(order=delta.order, sequence=delta.sequence)
        elif delta.action == BookAction.DELETE:
            self.delete(order=delta.order, sequence=delta.sequence)

        self.ts_last = delta.ts_init

    cdef void _apply_sequence(self, uint64_t sequence) except*:
        if sequence == 0:
            self.sequence += 1
        else:
            self.sequence = sequence

        self.count += 1

    cdef void _check_integrity(self) except *:
        """ Betting order book is reversed """
        cdef Level top_bid_level = self.bids.top()
        cdef Level top_ask_level = self.asks.top()
        if top_bid_level is None or top_ask_level is None:
            return

        cdef double best_bid = top_bid_level.price
        cdef double best_ask = top_ask_level.price
        if best_bid is None or best_ask is None:
            return
        if best_bid >= best_ask:
            raise BookIntegrityError(f"Orders in cross [{best_bid} @ {best_ask}]")

    cdef void update_quote_tick(self, QuoteTick tick) except*:
        raise NotImplementedError()

    cdef void update_trade_tick(self, TradeTick tick) except*:
        raise NotImplementedError()

    cpdef int trade_side(self, TradeTick trade):
        """
        Return which side of the book a trade occurred given a trade tick.

        Parameters
        ----------
        trade : TradeTick
            The trade tick.

        Returns
        -------
        OrderSide

        """
        if self.best_bid_price() and trade.price <= self.best_bid_price():
            return OrderSide.BUY
        elif self.best_ask_price() and trade.price >= self.best_ask_price():
            return OrderSide.SELL
        return 0  # Invalid trade tick

    cpdef Level best_bid_level(self):
        """
        Return the best bid level.

        Returns
        -------
        Level

        """
        return self.bids.top()

    cpdef Level best_ask_level(self):
        """
        Return the best ask level.

        Returns
        -------
        Level

        """
        return self.asks.top()

    cpdef best_bid_price(self):
        """
        Return the best bid price in the book (if no bids then returns ``None``).

        Returns
        -------
        double

        """
        cdef Level top_bid_level = self.bids.top()
        if top_bid_level:
            return top_bid_level.price
        else:
            return None

    cpdef best_ask_price(self):
        """
        Return the best ask price in the book (if no asks then returns ``None``).

        Returns
        -------
        double

        """
        cdef Level top_ask_level = self.asks.top()
        if top_ask_level:
            return top_ask_level.price
        else:
            return None

    cpdef best_bid_qty(self):
        """
        Return the best bid quantity in the book (if no bids then returns ``None``).

        Returns
        -------
        double

        """
        cdef Level top_bid_level = self.bids.top()
        if top_bid_level:
            return top_bid_level.volume()
        else:
            return None

    cpdef best_ask_qty(self):
        """
        Return the best ask quantity in the book (if no asks then returns ``None``).

        Returns
        -------
        double or ``None``

        """
        cdef Level top_ask_level = self.asks.top()
        if top_ask_level:
            return top_ask_level.volume()
        else:
            return None

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}\n"
            f"instrument: {self.instrument_id}\n"
            f"timestamp: {pd.Timestamp(self.ts_last)}\n\n"
            f"{self.pprint()}"
        )

    cpdef spread(self):
        """
        Return the top of book spread (if no bids or asks then returns ``None``).

        Returns
        -------
        double or ``None``

        """
        cdef Level top_bid_level = self.bids.top()
        cdef Level top_ask_level = self.asks.top()
        if top_bid_level and top_ask_level:
            return top_ask_level.price - top_bid_level.price
        else:
            return None

    cpdef midpoint(self):
        """
        Return the mid point (if no market exists the returns ``None``).

        Returns
        -------
        double or ``None``

        """
        cdef Level top_bid_level = self.bids.top()
        cdef Level top_ask_level = self.asks.top()
        if top_bid_level and top_ask_level:
            return (top_ask_level.price + top_bid_level.price) / 2.0
        else:
            return None

    cpdef str pprint(self, int num_levels=3, show="size"):
        """
        Print the order book in a clear format.

        Parameters
        ----------
        num_levels : int
            The number of levels to print.
        show : str
            The data to show.

        Returns
        -------
        str

        """
        cdef list levels = [
            (lvl.price, lvl) for lvl in self.bids.depth(num_levels) + self.asks.depth(num_levels)
        ]
        levels = list(reversed(sorted(levels, key=itemgetter(0))))
        cdef list data = [
            {
                "bids": [
                            getattr(order, show)
                            for order in level.orders
                            if level.price in self.bids.prices()
                        ]
                        or None,
                "price": level.price,
                "asks": [
                            getattr(order, show)
                            for order in level.orders
                            if level.price in self.asks.prices()
                        ]
                        or None,
            }
            for _, level in levels
        ]

        return tabulate(
            data,
            headers="keys",
            numalign="center",
            floatfmt=f".{self.price_precision}f",
            tablefmt="fancy",
        )

    cdef double get_price_for_volume_c(self, bint is_buy, double volume):
        cdef:
            Level level
            list levels = self.asks.levels if is_buy else self.bids.levels
            double cumulative_volume = 0.0
            double target_price = 0.0

        for level in levels:
            cumulative_volume += level.volume()
            if cumulative_volume >= volume:
                target_price = level.price
                break
        return target_price

    cdef double get_price_for_quote_volume_c(self, bint is_buy, double quote_volume):
        cdef:
            Level level
            list levels = self.asks.levels if is_buy else self.bids.levels
            double cumulative_volume = 0.0
            double target_price = 0.0

        for level in levels:
            cumulative_volume += level.volume() * level.price
            if cumulative_volume >= quote_volume:
                target_price = level.price
                break
        return target_price

    cdef double get_volume_for_price_c(self, bint is_buy, double price):
        cdef:
            Ladder book = self.bids if is_buy else self.asks
            Level top_of_book = book.top()
            double cumulative_volume = 0.0
            Level level

        if is_buy and top_of_book.price > price:
            # Buy price cannot be below best ask price
            return 0.0
        elif not is_buy and top_of_book.price < price:
            # Sell price cannot be above best bid price
            return 0.0

        if is_buy:
            for level in self.asks.levels:
                if level.price > price:
                    break
                cumulative_volume += level.volume()
        else:
            for level in self.bids.levels:
                if level.price < price:
                    break
                cumulative_volume += level.volume()
        return cumulative_volume

    cdef double get_quote_volume_for_price_c(self, bint is_buy, double price):
        cdef:
            Ladder book = self.bids if is_buy else self.asks
            Level top_of_book = book.top()
            double cumulative_quote_volume = 0.0
            Level level

        if is_buy and top_of_book.price > price:
            # Buy price cannot be below best ask price
            return 0.0
        elif not is_buy and top_of_book.price < price:
            # Sell price cannot be above best bid price
            return 0.0

        if is_buy:
            for level in self.asks.levels:
                if level.price > price:
                    break
                cumulative_quote_volume += level.volume() * level.price
        else:
            for level in self.bids.levels:
                if level.price < price:
                    break
                cumulative_quote_volume += level.volume() * level.price
        return cumulative_quote_volume

    cdef double get_vwap_for_volume_c(self, bint is_buy, double volume):
        cdef:
            Level level
            list levels = self.asks.levels if is_buy else self.bids.levels
            double total_cost = 0.0
            double cumulative_volume = 0.0
            double target_vwap = 0.0

        for level in levels:
            cumulative_volume += level.volume()
            total_cost += level.price * level.volume()
            if cumulative_volume >= volume:
                # Subtract exceed volume
                total_cost -= level.price * level.volume()
                cumulative_volume -= level.volume()
                remaining_volume = volume - cumulative_volume
                total_cost += remaining_volume * level.price
                cumulative_volume += remaining_volume
                target_vwap = total_cost / cumulative_volume
                break
        return target_vwap

    cpdef double get_price_for_volume(self, bint is_buy, double volume):
        return self.get_price_for_volume_c(is_buy, volume)
    cpdef double get_price_for_quote_volume(self, bint is_buy, double quote_volume):
        return self.get_price_for_quote_volume_c(is_buy, quote_volume)
    cpdef double get_volume_for_price(self, bint is_buy, double price):
        return self.get_volume_for_price_c(is_buy, price)
    cpdef double get_quote_volume_for_price(self, bint is_buy, double price):
        return self.get_quote_volume_for_price_c(is_buy, price)
    cpdef double get_vwap_for_volume(self, bint is_buy, double volume):
        return self.get_vwap_for_volume_c(is_buy, volume)


cpdef Price betfair_float_to_price_c(double value) except *:
    try:
        return BETFAIR_FLOAT_TO_PRICE[value]
    except KeyError:
        return Price(value, BETFAIR_PRICE_PRECISION)


cpdef Quantity betfair_float_to_quantity_c(double value) except *:
    cdef Quantity quantity = Quantity(value, BETFAIR_QUANTITY_PRECISION)
    return quantity
