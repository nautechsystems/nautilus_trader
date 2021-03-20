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

from tabulate import tabulate

from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.order_side cimport OrderSideParser
from nautilus_trader.model.orderbook.ladder cimport Ladder
from nautilus_trader.model.orderbook.level cimport Level
from nautilus_trader.model.orderbook.order cimport Order


cdef class OrderBook:
    """
    Provides a L1/L2/L3 order book.

    The base class for all order books.

    An L3 order book can be proxied to L2 or L1 `OrderBook` classes.
    """

    def __init__(self, InstrumentId instrument_id=None):
        """
        Initialize a new instance of the `OrderBook` class.

        """
        self.instrument_id = instrument_id
        self.bids = Ladder(reverse=True)
        self.asks = Ladder(reverse=False)

    cpdef void add(self, Order order) except *:
        """
        Add the given order to the book.

        Parameters
        ----------
        order : Order
            The order to add.

        """
        self._add(order=order)

    cpdef void update(self, Order order) except *:
        """
        Update the given order in the book.

        Parameters
        ----------
        order : Order
            The order to update.

        """
        self._update(order=order)

    cpdef void delete(self, Order order) except *:
        """
        Delete the given order in the book.

        Parameters
        ----------
        order : Order
            The order to delete.

        """
        self._delete(order=order)

    cpdef void apply_snapshot(
        self,
        list bids,
        list asks,
    ) except *:
        """
        Update this orderbook from a new snapshot.

        Parameters
        ----------
        bids : list
            The snapshot bids.
        asks : list
            The snapshot asks.

        """
        self.clear()
        cdef list bid
        for bid in bids:
            self.add(order=Order(price=bid[0], volume=bid[1], side=OrderSide.BUY))
        cdef list ask
        for ask in asks:
            self.add(order=Order(price=ask[0], volume=ask[1], side=OrderSide.SELL))

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
        Clear the bids from the book.
        """
        self.bids = Ladder(reverse=True)

    cpdef void clear_asks(self) except *:
        """
        Clear the asks from the book.
        """
        self.asks = Ladder(reverse=False)

    cpdef void clear(self) except *:
        """
        Clear the entire orderbook.
        """
        self.clear_bids()
        self.clear_asks()

    cdef inline void _add(self, Order order) except *:
        """
        Add the given order to the book.

        Parameters
        ----------
        order : Order
            The order to add.

        """
        if order.side == OrderSide.BUY:
            self.bids.add(order=order)
        elif order.side == OrderSide.SELL:
            self.asks.add(order=order)

    cdef inline void _update(self, Order order) except *:
        """
        Update the given order in the book.

        Parameters
        ----------
        order : Order
            The order to update.

        """
        if order.side == OrderSide.BUY:
            self.bids.update(order=order)
        elif order.side == OrderSide.SELL:
            self.asks.update(order=order)

    cdef inline void _delete(self, Order order) except *:
        """
        Delete the given order in the book.

        Parameters
        ----------
        order : Order
            The order to delete.

        """
        if order.side == OrderSide.BUY:
            self.bids.delete(order=order)
        elif order.side == OrderSide.SELL:
            self.asks.delete(order=order)

    cdef inline void _check_integrity(self) except *:
        """
        Return a value indicating whether the order book integrity test passes.

        Returns
        -------
        bool
            True if check passes, else False.

        """
        cdef Level top_bid_level = self.bids.top()
        cdef Level top_ask_level = self.asks.top()
        if top_bid_level is None or top_ask_level is None:
            return

        cdef double best_bid = top_bid_level.price()
        cdef double best_ask = top_ask_level.price()
        if best_bid == 0. or best_ask == 0.:
            return
        assert best_bid < best_ask, f"Orders in cross [{best_bid} @ {best_ask}]"

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
            return top_bid_level.price()
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
            return top_ask_level.price()
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

    def __repr__(self):
        return self.pprint()

    cpdef spread(self):
        """
        Return the top of book spread (if no bids or asks then returns None).

        Returns
        -------
        double

        """
        cdef Level top_bid_level = self.bids.top()
        cdef Level top_ask_level = self.asks.top()
        if top_bid_level and top_ask_level:
            return top_ask_level.price() - top_bid_level.price()
        else:
            return None

    cpdef str pprint(self, int num_levels=3):
        levels = reversed(
            [
                lvl
                for lvl in self.bids.levels[-num_levels:]
                           + self.asks.levels[:num_levels]
            ]
        )
        data = [
            {
                "bids": [
                            order.id
                            for order in level.orders
                            if level.price() in self.bids.prices()
                        ]
                        or None,
                "price": level.price(),
                "asks": [
                            order.id
                            for order in level.orders
                            if level.price() in self.asks.prices()
                        ]
                        or None,
            }
            for level in levels
        ]
        return tabulate(
            data, headers="keys", numalign="center", floatfmt=".2f", tablefmt="fancy"
        )


cdef class L3OrderBook(OrderBook):
    """
    Provides an L3 order book.

    Maps directly to functionality of the `OrderBook` base class.
    """
    pass


cdef class L2OrderBook(OrderBook):
    """
    Provides an L2 order book.

    An L2 order book `Levels` are only made up of a single order.
    """

    cpdef void add(self, Order order) except *:
        """
        Add the given order to the book.

        Parameters
        ----------
        order : Order
            The order to add.

        """
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
        # For L2 Orderbook, ensure only one order per level in addition to
        # normal orderbook checks.
        self._check_integrity()

        for level in self.bids.levels + self.asks.levels:
            assert len(level.orders) == 1, f"Number of orders on {level} > 1"

    cdef inline Order _process_order(self, Order order):
        # Because L2 Orderbook only has one order per level, we replace the
        # order.id with a price level, which will let us easily process the
        # order in the proxy orderbook.
        order.id = str(order.price)
        return order

    cdef inline void _remove_if_exists(self, Order order) except *:
        # For a L2 orderbook, an order update means a whole level update. If
        # this level exists, remove it so we can insert the new level.

        if order.side == OrderSide.BUY and order.price in self.bids.prices():
            self.delete(order)
        elif order.side == OrderSide.SELL and order.price in self.asks.prices():
            self.delete(order)


cdef class L1OrderBook(OrderBook):
    """
    Provides an L1 order book.

    An L1 order book has a single (top) `Level`.
    """

    cpdef void add(self, Order order) except *:
        """
        NotImplemented (Use `update(order)` for L1Orderbook).
        """
        raise NotImplementedError("Use `update(order)` for L1Orderbook")

    cpdef void update(self, Order order) except *:
        """
        Update the given order in the book.

        Parameters
        ----------
        order : Order
            The order to update.

        """
        # Because of the way we typically get updates from a L1 order book (bid
        # and ask updates at the same time), its quite probable that the last
        # bid is now the ask price we are trying to insert (or vice versa). We
        # just need to add some extra protection against this if we are calling
        # `check_integrity` on each individual update .
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

    cpdef void delete(self, Order order) except *:
        """
        Delete the given order in the book.

        Parameters
        ----------
        order : Order
            The order to delete.

        """
        self._delete(order=self._process_order(order=order))

    cpdef void check_integrity(self) except *:
        """
        Return a value indicating whether the order book integrity test passes.

        Returns
        -------
        bool
            True if check passes, else False.

        """
        # For an L1OrderBook, ensure only one level per side in addition to
        # normal orderbook checks.
        self._check_integrity()
        assert len(self.bids.levels) <= 1, "Number of bid levels > 1"
        assert len(self.asks.levels) <= 1, "Number of ask levels > 1"

    cdef inline Order _process_order(self, Order order):
        # Because a L1OrderBook only has one level per side, we replace the
        # order.id with the name of the side, which will let us easily process
        # the order.
        order.id = OrderSideParser.to_str(order.side)
        return order
