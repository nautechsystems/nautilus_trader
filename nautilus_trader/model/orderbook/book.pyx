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

from operator import itemgetter

import pandas as pd
from tabulate import tabulate

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.c_enums.aggressor_side cimport AggressorSide
from nautilus_trader.model.c_enums.delta_type cimport DeltaType
from nautilus_trader.model.c_enums.delta_type cimport DeltaTypeParser
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.order_side cimport OrderSideParser
from nautilus_trader.model.c_enums.orderbook_level cimport OrderBookLevel
from nautilus_trader.model.c_enums.orderbook_level cimport OrderBookLevelParser
from nautilus_trader.model.data cimport Data
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.orderbook.ladder cimport Ladder
from nautilus_trader.model.orderbook.level cimport Level
from nautilus_trader.model.orderbook.order cimport Order
from nautilus_trader.model.tick cimport QuoteTick
from nautilus_trader.model.tick cimport Tick
from nautilus_trader.model.tick cimport TradeTick


cdef class OrderBook:
    """
    The base class for all order books.

    Provides a L1/L2/L3 order book as a `L3OrderBook` can be proxied to
    `L2OrderBook` or `L1OrderBook` classes.
    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        OrderBookLevel level,
        int price_precision,
        int size_precision,
    ):
        """
        Initialize a new instance of the ``OrderBook`` class.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument identifier for the book.
        level : OrderBookLevel
            The order book level (L1, L2, L3).
        price_precision : int
            The price precision for the book.
        size_precision : int
            The size precision for the book.

        Raises
        ------
        ValueError
            If initializing type is not a subclass of `OrderBook`.
        ValueError
            If price_precision is negative (< 0).
        ValueError
            If size_precision is negative (< 0).

        """
        if self.__class__.__name__ == OrderBook.__name__:
            raise RuntimeError("Cannot instantiate OrderBook directly: use OrderBook.create()")
        Condition.not_negative_int(price_precision, "price_precision")
        Condition.not_negative_int(size_precision, "size_precision")

        self.instrument_id = instrument_id
        self.level = level
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
        self.last_update_timestamp_ns = 0

    @staticmethod
    def create(
        Instrument instrument,
        OrderBookLevel level,
    ):
        """
        Create a new order book with the given parameters.

        Parameters
        ----------
        instrument : Instrument
            The instrument for the book.
        level : OrderBookLevel
            The order book level (L1, L2, L3).

        Returns
        -------
        OrderBook

        """
        Condition.not_none(instrument, "instrument")
        Condition.in_range_int(level, 1, 3, "level")

        if level == OrderBookLevel.L1:
            return L1OrderBook(
                instrument_id=instrument.id,
                price_precision=instrument.price_precision,
                size_precision=instrument.size_precision,
            )
        elif level == OrderBookLevel.L2:
            return L2OrderBook(
                instrument_id=instrument.id,
                price_precision=instrument.price_precision,
                size_precision=instrument.size_precision,
            )
        elif level == OrderBookLevel.L3:
            return L3OrderBook(
                instrument_id=instrument.id,
                price_precision=instrument.price_precision,
                size_precision=instrument.size_precision,
            )

    cpdef void add(self, Order order) except *:
        """
        Add the given order to the book.

        Parameters
        ----------
        order : Order
            The order to add.

        """
        Condition.not_none(order, "order")

        self._add(order=order)

    cpdef void update(self, Order order) except *:
        """
        Update the given order in the book.

        Parameters
        ----------
        order : Order
            The order to update.

        """
        Condition.not_none(order, "order")

        self._update(order=order)

    cpdef void delete(self, Order order) except *:
        """
        Delete the given order in the book.

        Parameters
        ----------
        order : Order
            The order to delete.

        """
        Condition.not_none(order, "order")

        self._delete(order=order)

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
            If delta.level is not equal to self.level.

        """
        Condition.not_none(delta, "delta")
        Condition.equal(delta.level, self.level, "delta.level", "self.level")

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
            If snapshot.level is not equal to self.level.

        """
        Condition.not_none(deltas, "deltas")
        Condition.equal(deltas.level, self.level, "deltas.level", "self.level")

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
            If snapshot.level is not equal to self.level.

        """
        Condition.not_none(snapshot, "snapshot")
        Condition.equal(snapshot.level, self.level, "snapshot.level", "self.level")

        self.clear()
        # Use `update` instead of `add` (when book has been cleared they're
        # equivalent) to make work for L1 Orderbook.
        cdef Order order
        for bid in snapshot.bids:
            order = Order(
                price=bid[0],
                volume=bid[1],
                side=OrderSide.BUY
            )
            self.update(order=order)
        for ask in snapshot.asks:
            order = Order(
                price=ask[0],
                volume=ask[1],
                side=OrderSide.SELL
            )
            self.update(order=order)

        self.last_update_timestamp_ns = snapshot.ts_recv_ns

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
            If data.level is not equal to self.level.

        """
        Condition(data.level, self.level, "data.level", "self.level")

        if isinstance(data, OrderBookSnapshot):
            self.apply_snapshot(snapshot=data)
        elif isinstance(data, OrderBookDeltas):
            self.apply_deltas(deltas=data)
        elif isinstance(data, OrderBookDelta):
            self._apply_delta(delta=data)

    cpdef void check_integrity(self) except *:
        """
        Return a value indicating whether the order book integrity test passes.

        Returns
        -------
        bool
            True if check passes, else False.

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

    cdef void _apply_delta(self, OrderBookDelta delta) except *:
        if delta.type == DeltaType.ADD:
            self.add(order=delta.order)
        elif delta.type == DeltaType.UPDATE:
            self.update(order=delta.order)
        elif delta.type == DeltaType.DELETE:
            self.delete(order=delta.order)

        self.last_update_timestamp_ns = delta.ts_recv_ns

    cdef void _add(self, Order order) except *:
        if order.side == OrderSide.BUY:
            self.bids.add(order=order)
        elif order.side == OrderSide.SELL:
            self.asks.add(order=order)

    cdef void _update(self, Order order) except *:
        if order.side == OrderSide.BUY:
            self.bids.update(order=order)
        elif order.side == OrderSide.SELL:
            self.asks.update(order=order)

    cdef void _delete(self, Order order) except *:
        if order.side == OrderSide.BUY:
            self.bids.delete(order=order)
        elif order.side == OrderSide.SELL:
            self.asks.delete(order=order)

    cdef void _check_integrity(self) except *:
        cdef Level top_bid_level = self.bids.top()
        cdef Level top_ask_level = self.asks.top()
        if top_bid_level is None or top_ask_level is None:
            return

        best_bid = top_bid_level.price
        best_ask = top_ask_level.price
        if best_bid is None or best_ask is None:
            return
        assert best_bid < best_ask, f"Orders in cross [{best_bid} @ {best_ask}]"

    @property
    def timestamp_ns(self):
        """
        The UNIX timestamp (nanos) of the last update.

        Returns
        -------
        int64

        """
        return self.last_update_timestamp_ns

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
        Return the best bid price in the book (if no bids then returns None).

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
        Return the best ask price in the book (if no asks then returns None).

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
        Return the best bid quantity in the book (if no bids then returns None).

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
        Return the best ask quantity in the book (if no asks then returns None).

        Returns
        -------
        double or None

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
            f"timestamp: {pd.Timestamp(self.last_update_timestamp_ns)}\n\n"
            f"{self.pprint()}"
        )

    cpdef spread(self):
        """
        Return the top of book spread (if no bids or asks then returns None).

        Returns
        -------
        double or None

        """
        cdef Level top_bid_level = self.bids.top()
        cdef Level top_ask_level = self.asks.top()
        if top_bid_level and top_ask_level:
            return top_ask_level.price - top_bid_level.price
        else:
            return None

    cpdef midpoint(self):
        """
        Return the mid point (if no market exists the returns None).

        Returns
        -------
        double or None

        """
        cdef Level top_bid_level = self.bids.top()
        cdef Level top_ask_level = self.asks.top()
        if top_bid_level and top_ask_level:
            return (top_ask_level.price + top_bid_level.price) / 2.0
        else:
            return None

    cpdef str pprint(self, int num_levels=3, show="volume"):
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
            list levels = self.asks.levels \
                if is_buy else self.bids.levels
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
            list levels = self.asks.levels \
                if is_buy else self.bids.levels
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
            list levels = self.asks.levels \
                if is_buy else self.bids.levels
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
    Provides an L3 order book.

    A level 3 order books `Levels` can be made up of multiple orders.
    This class maps directly to functionality of the `OrderBook` base class.
    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        int price_precision,
        int size_precision,
    ):
        """
        Initialize a new instance of the ``L3OrderBook`` class.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument identifier for the book.
        price_precision : int
            The price precision for the book.
        size_precision : int
            The size precision for the book.

        Raises
        ------
        ValueError
            If price_precision is negative (< 0).
        ValueError
            If size_precision is negative (< 0).

        """
        super().__init__(
            instrument_id=instrument_id,
            level=OrderBookLevel.L3,
            price_precision=price_precision,
            size_precision=size_precision,
        )


cdef class L2OrderBook(OrderBook):
    """
    Provides an L2 order book.

    A level 2 order books `Levels` are only made up of a single order.
    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        int price_precision,
        int size_precision,
    ):
        """
        Initialize a new instance of the ``L2OrderBook`` class.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument identifier for the book.
        price_precision : int
            The price precision for the book.
        size_precision : int
            The size precision for the book.

        """
        super().__init__(
            instrument_id=instrument_id,
            level=OrderBookLevel.L2,
            price_precision=price_precision,
            size_precision=size_precision,
        )

    cpdef void add(self, Order order) except *:
        """
        Add the given order to the book.

        Parameters
        ----------
        order : Order
            The order to add.

        """
        Condition.not_none(order, "order")

        self._process_order(order=order)
        self._add(order=order)

    cpdef void update(self, Order order) except *:
        """
        Update the given order in the book.

        Parameters
        ----------
        order : Order
            The order to update.

        """
        Condition.not_none(order, "order")

        self._process_order(order=order)
        self._remove_if_exists(order)
        self._update(order=order)

    cpdef void delete(self, Order order) except *:
        """
        Delete the given order in the book.

        Parameters
        ----------
        order : Order
            The order to delete.

        """
        Condition.not_none(order, "order")

        self._process_order(order=order)
        self._delete(order=order)

    cpdef void check_integrity(self) except *:
        """
        Return a value indicating whether the order book integrity test passes.

        Returns
        -------
        bool
            True if check passes, else False.

        """
        # For a L2OrderBook, ensure only one order per level in addition to
        # normal order book checks.
        self._check_integrity()

        cdef Level level
        for level in self.bids.levels + self.asks.levels:
            assert len(level.orders) == 1, f"Number of orders on {level} > 1"

    cdef void _process_order(self, Order order):
        # Because a L2OrderBook only has one order per level, we replace the
        # order.id with a price level, which will let us easily process the
        # order in the base class.
        order.id = f"{order.price:.{self.price_precision}f}"

    cdef void _remove_if_exists(self, Order order) except *:
        # For a L2OrderBook, an order update means a whole level update. If this
        # level exists, remove it so we can insert the new level.
        if order.side == OrderSide.BUY and order.price in self.bids.prices():
            self._delete(order)
        elif order.side == OrderSide.SELL and order.price in self.asks.prices():
            self._delete(order)


cdef class L1OrderBook(OrderBook):
    """
    Provides an L1 order book.

    A level 1 order book has a single (top) `Level`.
    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        int price_precision,
        int size_precision,
    ):
        """
        Initialize a new instance of the ``L1OrderBook`` class.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument identifier for the book.
        price_precision : int
            The price precision for the book.
        size_precision : int
            The size precision for the book.

        """
        super().__init__(
            instrument_id=instrument_id,
            level=OrderBookLevel.L1,
            price_precision=price_precision,
            size_precision=size_precision,
        )

        self._top_bid = None
        self._top_ask = None
        self._top_bid_level = None
        self._top_ask_level = None

    cpdef void add(self, Order order) except *:
        """
        NotImplemented (Use `update(order)` for L1OrderBook).
        """
        raise NotImplementedError("Use `update(order)` for L1OrderBook")

    cpdef void update(self, Order order) except *:
        """
        Update the given order in the book.

        Parameters
        ----------
        order : Order
            The order to update.

        """
        Condition.not_none(order, "order")

        # Because of the way we typically get updates from a L1 order book (bid
        # and ask updates at the same time), its quite probable that the last
        # bid is now the ask price we are trying to insert (or vice versa). We
        # just need to add some extra protection against this if we are calling
        # `check_integrity` on each individual update.
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
        self._update(order=self._process_order(order=order))

    cpdef void update_top(self, Tick tick) except *:
        """
        Update the order book with the given tick.

        Parameters
        ----------
        tick : Tick
            The tick to update with.

        """
        if isinstance(tick, QuoteTick):
            self._update_quote_tick(tick)
        elif isinstance(tick, TradeTick):
            self._update_trade_tick(tick)

    cdef void _update_quote_tick(self, QuoteTick tick):
        self._update_bid(tick.bid, tick.bid_size)
        self._update_ask(tick.ask, tick.ask_size)

    cdef void _update_trade_tick(self, TradeTick tick):
        if tick.aggressor_side == AggressorSide.SELL:  # TAKER hit the bid
            self._update_bid(tick.price, tick.size)
            if self._top_ask and self._top_bid.price >= self._top_ask.price:
                self._top_ask.price == self._top_bid.price
                self._top_ask_level.price == self._top_bid.price
        elif tick.aggressor_side == AggressorSide.BUY:  # TAKER lifted the offer
            self._update_ask(tick.price, tick.size)
            if self._top_bid and self._top_ask.price <= self._top_bid.price:
                self._top_bid.price == self._top_ask.price
                self._top_bid_level.price == self._top_ask.price

    cdef void _update_bid(self, double price, double size):
        if self._top_bid is None:
            bid = self._process_order(Order(price, size, OrderSide.BUY))
            self._add(bid)
            self._top_bid = bid
            self._top_bid_level = self.bids.top()
        else:
            self._top_bid_level.price = price
            self._top_bid.update_price(price)
            self._top_bid.update_volume(size)

    cdef void _update_ask(self, double price, double size):
        if self._top_ask is None:
            ask = self._process_order(Order(price, size, OrderSide.SELL))
            self._add(ask)
            self._top_ask = ask
            self._top_ask_level = self.asks.top()
        else:
            self._top_ask_level.price = price
            self._top_ask.update_price(price)
            self._top_ask.update_volume(size)

    cpdef void delete(self, Order order) except *:
        """
        Delete the given order in the book.

        Parameters
        ----------
        order : Order
            The order to delete.

        """
        Condition.not_none(order, "order")

        self._delete(order=self._process_order(order=order))

    cpdef void check_integrity(self) except *:
        """
        Return a value indicating whether the order book integrity test passes.

        Returns
        -------
        bool
            True if check passes, else False.

        """
        # For a L1OrderBook, ensure only one level per side in addition to
        # normal order book checks.
        self._check_integrity()
        assert len(self.bids.levels) <= 1, "Number of bid levels > 1"
        assert len(self.asks.levels) <= 1, "Number of ask levels > 1"

    cdef Order _process_order(self, Order order):
        # Because a L1OrderBook only has one level per side, we replace the
        # order.id with the name of the side, which will let us easily process
        # the order.
        order.id = OrderSideParser.to_str(order.side)
        return order


cdef class OrderBookData(Data):
    """
    The abstract base class for all `OrderBook` data.

    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        OrderBookLevel level,
        int64_t ts_event_ns,
        int64_t ts_recv_ns,
    ):
        """
        Initialize a new instance of the ``OrderBookData`` class.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument identifier for the book.
        level : OrderBookLevel
            The order book level (L1, L2, L3).
        ts_event_ns : int64
            The UNIX timestamp (nanos) when data event occurred.
        ts_recv_ns : int64
            The UNIX timestamp (nanos) when received by the Nautilus system.

        """
        super().__init__(ts_event_ns, ts_recv_ns)

        self.instrument_id = instrument_id
        self.level = level


cdef class OrderBookSnapshot(OrderBookData):
    """
    Represents a snapshot in time for an `OrderBook`.
    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        OrderBookLevel level,
        list bids not None,
        list asks not None,
        int64_t ts_event_ns,
        int64_t ts_recv_ns,
    ):
        """
        Initialize a new instance of the ``OrderBookSnapshot`` class.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument identifier for the book.
        level : OrderBookLevel
            The order book level (L1, L2, L3).
        bids : list
            The bids for the snapshot.
        asks : list
            The asks for the snapshot.
        ts_event_ns : int64
            The UNIX timestamp (nanos) when data event occurred.
        ts_recv_ns : int64
            The UNIX timestamp (nanos) when received by the Nautilus system.

        """
        super().__init__(instrument_id, level, ts_event_ns, ts_recv_ns)

        self.bids = bids
        self.asks = asks

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"'{self.instrument_id}', "
                f"level={OrderBookLevelParser.to_str(self.level)}, "
                f"bids={self.bids}, "
                f"asks={self.asks}, "
                f"ts_recv_ns={self.ts_recv_ns})")


cdef class OrderBookDeltas(OrderBookData):
    """
    Represents bulk changes for an `OrderBook`.
    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        OrderBookLevel level,
        list deltas not None,
        int64_t ts_event_ns,
        int64_t ts_recv_ns,
    ):
        """
        Initialize a new instance of the ``OrderBookDeltas`` class.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument identifier for the book.
        level : OrderBookLevel
            The order book level (L1, L2, L3).
        deltas : list[OrderBookDelta]
            The list of order book changes.
        ts_event_ns : int64
            The UNIX timestamp (nanos) when data event occurred.
        ts_recv_ns : int64
            The UNIX timestamp (nanos) when received by the Nautilus system.

        """
        super().__init__(instrument_id, level, ts_event_ns, ts_recv_ns)

        self.deltas = deltas

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"'{self.instrument_id}', "
                f"level={OrderBookLevelParser.to_str(self.level)}, "
                f"{self.deltas}, "
                f"ts_recv_ns={self.ts_recv_ns})")


cdef class OrderBookDelta(OrderBookData):
    """
    Represents a single difference on an `OrderBook`.
    """

    def __init__(
        self,
        InstrumentId instrument_id,
        OrderBookLevel level,
        DeltaType delta_type,
        Order order not None,
        int64_t ts_event_ns,
        int64_t ts_recv_ns,
    ):
        """
        Initialize a new instance of the ``OrderBookDelta`` class.

        Parameters
        ----------
        delta_type : DeltaType
            The type of change (ADD, UPDATED, DELETE, CLEAR).
        order : Order
            The order to apply.
        ts_event_ns : int64
            The UNIX timestamp (nanos) when data event occurred.
        ts_recv_ns : int64
            The UNIX timestamp (nanos) when received by the Nautilus system.

        """
        super().__init__(instrument_id, level, ts_event_ns, ts_recv_ns)

        self.type = delta_type
        self.order = order

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"'{self.instrument_id}', "
                f"level={OrderBookLevelParser.to_str(self.level)}, "
                f"delta_type={DeltaTypeParser.to_str(self.type)}, "
                f"order={self.order}, "
                f"ts_recv_ns={self.ts_recv_ns})")
