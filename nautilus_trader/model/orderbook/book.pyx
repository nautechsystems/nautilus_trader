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

from nautilus_trader.model.orderbook.error import BookIntegrityError

from libc.stdint cimport uint8_t
from libc.stdint cimport uint64_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.data.tick cimport TradeTick
from nautilus_trader.model.enums_c cimport BookAction
from nautilus_trader.model.enums_c cimport BookType
from nautilus_trader.model.enums_c cimport OrderSide
from nautilus_trader.model.enums_c cimport order_side_to_str
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.orderbook.data cimport BookOrder
from nautilus_trader.model.orderbook.data cimport OrderBookSnapshot
from nautilus_trader.model.orderbook.ladder cimport Ladder
from nautilus_trader.model.orderbook.level cimport Level
from nautilus_trader.model.orderbook.simulated cimport SimulatedL1OrderBook
from nautilus_trader.model.orderbook.simulated cimport SimulatedL2OrderBook
from nautilus_trader.model.orderbook.simulated cimport SimulatedL3OrderBook


cdef class OrderBook:
    """
    The base class for all order books.

    Provides a L1/L2/L3 order book as an `L3OrderBook` which can be proxied to
    `L2OrderBook` or `L1OrderBook` classes.
    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        BookType book_type,
        uint8_t price_precision,
        uint8_t size_precision,
    ):
        if self.__class__.__name__ == OrderBook.__name__:  # pragma: no cover
            raise RuntimeError("cannot instantiate OrderBook directly: use OrderBook.create()")

        self.instrument_id = instrument_id
        self.type = book_type
        self.price_precision = price_precision
        self.size_precision = size_precision
        self.bids = Ladder(
            reverse=True,
            price_precision=price_precision,
            size_precision=size_precision,
        )
        self.asks = Ladder(
            reverse=False,
            price_precision=price_precision,
            size_precision=size_precision,
        )
        self.sequence = 0
        self.count = 0
        self.ts_last = 0

    @staticmethod
    def create(
        Instrument instrument,
        BookType book_type,
        bint simulated=False,
    ):
        """
        Create a new order book with the given parameters.

        Parameters
        ----------
        instrument : Instrument
            The instrument for the book.
        book_type : BookType {``L1_TBBO``, ``L2_MBP``, ``L3_MBO``}
            The order book level.
        simulated : bool
            If the order book should be simulated (for backtesting only).

        Returns
        -------
        OrderBook

        """
        Condition.not_none(instrument, "instrument")
        Condition.in_range_int(book_type, 1, 3, "book_type")

        if book_type == BookType.L1_TBBO:
            if simulated:
                return SimulatedL1OrderBook(
                    instrument_id=instrument.id,
                    price_precision=instrument.price_precision,
                    size_precision=instrument.size_precision,
                )
            else:
                return L1OrderBook(
                    instrument_id=instrument.id,
                    price_precision=instrument.price_precision,
                    size_precision=instrument.size_precision,
                )
        elif book_type == BookType.L2_MBP:
            if simulated:
                return SimulatedL2OrderBook(
                    instrument_id=instrument.id,
                    price_precision=instrument.price_precision,
                    size_precision=instrument.size_precision,
                )
            else:
                return L2OrderBook(
                    instrument_id=instrument.id,
                    price_precision=instrument.price_precision,
                    size_precision=instrument.size_precision,
                )
        elif book_type == BookType.L3_MBO:
            if simulated:
                return SimulatedL3OrderBook(
                    instrument_id=instrument.id,
                    price_precision=instrument.price_precision,
                    size_precision=instrument.size_precision,
                )
            else:
                return L3OrderBook(
                    instrument_id=instrument.id,
                    price_precision=instrument.price_precision,
                    size_precision=instrument.size_precision,
                )

    cpdef void add(self, BookOrder order, uint64_t sequence=0) except *:
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

    cpdef void update(self, BookOrder order, uint64_t sequence=0) except *:
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

    cpdef void delete(self, BookOrder order, uint64_t sequence=0) except *:
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

    cpdef void apply_delta(self, OrderBookDelta delta) except *:
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

    cpdef void apply_deltas(self, OrderBookDeltas deltas) except *:
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

    cpdef void apply_snapshot(self, OrderBookSnapshot snapshot) except *:
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

    cpdef void apply(self, OrderBookData data) except *:
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

    cpdef void check_integrity(self) except *:
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

    cpdef void clear_bids(self) except *:
        """
        Clear the bids from the order book.
        """
        self.bids = Ladder(
            reverse=True,
            price_precision=self.price_precision,
            size_precision=self.size_precision,
        )

    cpdef void clear_asks(self) except *:
        """
        Clear the asks from the order book.
        """
        self.asks = Ladder(
            reverse=False,
            price_precision=self.price_precision,
            size_precision=self.size_precision,
        )

    cpdef void clear(self) except *:
        """
        Clear the entire order book.
        """
        self.clear_bids()
        self.clear_asks()

    cdef void _add(self, BookOrder order, uint64_t sequence) except *:
        if order.side == OrderSide.BUY:
            self.bids.add(order=order)
        elif order.side == OrderSide.SELL:
            self.asks.add(order=order)
        self._apply_sequence(sequence)

    cdef void _update(self, BookOrder order, uint64_t sequence) except *:
        if order.side == OrderSide.BUY:
            self.bids.update(order=order)
        elif order.side == OrderSide.SELL:
            self.asks.update(order=order)
        self._apply_sequence(sequence)

    cdef void _delete(self, BookOrder order, uint64_t sequence) except *:
        if order.side == OrderSide.BUY:
            self.bids.delete(order=order)
        elif order.side == OrderSide.SELL:
            self.asks.delete(order=order)
        self._apply_sequence(sequence)

    cdef void _apply_delta(self, OrderBookDelta delta) except *:
        if delta.action == BookAction.ADD:
            self.add(order=delta.order, sequence=delta.sequence)
        elif delta.action == BookAction.UPDATE:
            self.update(order=delta.order, sequence=delta.sequence)
        elif delta.action == BookAction.DELETE:
            self.delete(order=delta.order, sequence=delta.sequence)

        self.ts_last = delta.ts_init

    cdef void _apply_sequence(self, uint64_t sequence) except *:
        if sequence == 0:
            self.sequence += 1
        else:
            self.sequence = sequence

        self.count += 1

    cdef void _check_integrity(self) except *:
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

    cdef void update_quote_tick(self, QuoteTick tick) except *:
        raise NotImplementedError()

    cdef void update_trade_tick(self, TradeTick tick) except *:
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


cdef class L3OrderBook(OrderBook):

    """
    Provides an L3 MBO (market by order) order book.

    A level 3 order books `Levels` can be made up of multiple orders.
    This class maps directly to the functionality of the base class.

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
            book_type=BookType.L3_MBO,
            price_precision=price_precision,
            size_precision=size_precision,
        )


cdef class L2OrderBook(OrderBook):
    """
    Provides a L2 MBP (market by price) order book.

    A level 2 order books `Levels` are only made up of a single order.

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
            book_type=BookType.L2_MBP,
            price_precision=price_precision,
            size_precision=size_precision,
        )

    cpdef void add(self, BookOrder order, uint64_t sequence=0) except *:
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

        self._process_order(order=order)
        self._add(order=order, sequence=sequence)

    cpdef void update(self, BookOrder order, uint64_t sequence=0) except *:
        """
        Update the given order in the book.

        Parameters
        ----------
        order : BookOrder
            The order to update.
        sequence : uint64, default 0
            The unique sequence number for the update. If default 0 then will increment the `sequence`.

        """
        Condition.not_none(order, "order")

        self._process_order(order=order)
        self._remove_if_exists(order, sequence=sequence)
        self._update(order=order, sequence=sequence)

    cpdef void delete(self, BookOrder order, uint64_t sequence=0) except *:
        """
        Delete the given order in the book.

        Parameters
        ----------
        order : BookOrder
            The order to delete.
        sequence : uint64, default 0
            The unique sequence number for the update. If default 0 then will increment the `sequence`.

        """
        Condition.not_none(order, "order")

        self._process_order(order=order)
        self._delete(order=order, sequence=sequence)

    cpdef void check_integrity(self) except *:
        """
        Check order book integrity.

        For a L2_MBP order book:
        - There should be at most one order per level.
        - The bid side price should not be greater than or equal to the ask side price.

        Raises
        ------
        BookIntegrityError
            If any check fails.

        """
        self._check_integrity()

        cdef Level level
        for level in self.bids.levels + self.asks.levels:
            num_orders = len(level.orders)
            if num_orders != 1:
                raise BookIntegrityError(f"Number of orders on {level} != 1, was {num_orders}")

    cdef void _process_order(self, BookOrder order) except *:
        # Because a L2OrderBook only has one order per level, we replace the
        # order.order_id with a price level, which will let us easily process the
        # order in the base class.
        order.order_id = f"{order.price:.{self.price_precision}f}"

    cdef void _remove_if_exists(self, BookOrder order, uint64_t sequence) except *:
        # For a L2OrderBook, an order update means a whole level update. If this
        # level exists, remove it so that we can insert the new level.
        if order.side == OrderSide.BUY and order.price in self.bids.prices():
            self._delete(order, sequence=sequence)
        elif order.side == OrderSide.SELL and order.price in self.asks.prices():
            self._delete(order, sequence=sequence)


cdef class L1OrderBook(OrderBook):
    """
    Provides a L1 TBBO (top of book best bid/offer) order book.

    A level 1 order book has a single (top) `Level`.

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
            book_type=BookType.L1_TBBO,
            price_precision=price_precision,
            size_precision=size_precision,
        )

    cpdef void add(self, BookOrder order, uint64_t sequence=0) except *:
        """
        NotImplemented (Use `update(order)` for L1OrderBook).
        """
        raise NotImplementedError("Use `update(order)` for L1OrderBook")  # pragma: no cover

    cpdef void update(self, BookOrder order, uint64_t sequence=0) except *:
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

        # Because of the way we typically get updates from a L1 order book (bid
        # and ask updates at the same time), its quite probable that the last
        # bid is now the ask price we are trying to insert (or vice versa). We
        # just need to add some extra protection against this if we aren't calling
        # `check_integrity()` on each individual update.
        if (
            order.side == OrderSide.BUY
            and self.best_ask_level()
            and order.price >= self.best_ask_price()
        ):
            self.clear_asks()
        elif (
            order.side == OrderSide.SELL
            and self.best_bid_level()
            and order.price <= self.best_bid_price()
        ):
            self.clear_bids()
        self._update(order=self._process_order(order=order), sequence=sequence)

    cpdef void delete(self, BookOrder order, uint64_t sequence=0) except *:
        """
        Delete the given order in the book.

        Parameters
        ----------
        order : BookOrder
            The order to delete.
        sequence : uint64, default 0
            The unique sequence number for the update. If default 0 then will increment the `sequence`.

        """
        Condition.not_none(order, "order")

        self._delete(order=self._process_order(order=order), sequence=sequence)

    cpdef void check_integrity(self) except *:
        """
        Check order book integrity.

        For a L1 TBBO order book:
        - There should be at most one level per side.
        - The bid side price should not be greater than or equal to the ask side price.

        Raises
        ------
        BookIntegrityError
            If any check fails.

        """
        self._check_integrity()

        cdef int bid_levels = len(self.bids.levels)
        cdef int ask_levels = len(self.asks.levels)

        if bid_levels > 1:
            raise BookIntegrityError(f"Number of bid levels > 1, was {bid_levels}")
        if ask_levels > 1:
            raise BookIntegrityError(f"Number of ask levels > 1, was {ask_levels}")

    cdef BookOrder _process_order(self, BookOrder order):
        # Because an `L1OrderBook` only has one level per side, we replace the
        # `order.order_id` with the name of the side, which will let us easily process
        # the order.
        order.order_id = order_side_to_str(order.side)
        return order
