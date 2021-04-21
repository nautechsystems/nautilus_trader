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
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.order_side cimport OrderSideParser
from nautilus_trader.model.c_enums.orderbook_delta cimport OrderBookDeltaType
from nautilus_trader.model.c_enums.orderbook_delta cimport OrderBookDeltaTypeParser
from nautilus_trader.model.c_enums.orderbook_level cimport OrderBookLevel
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.data cimport Data
from nautilus_trader.model.orderbook.ladder cimport Ladder
from nautilus_trader.model.orderbook.level cimport Level
from nautilus_trader.model.orderbook.order cimport Order


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
        Initialize a new instance of the `OrderBook` class.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument identifier for the book.
        level : OrderBookLevel (Enum)
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
        Condition.true(
            self.__class__.__name__ != "OrderBook",
            "Cannot instantiate OrderBook directly: use OrderBook.create()",
        )
        Condition.not_negative_int(price_precision, "price_precision")
        Condition.not_negative_int(size_precision, "size_precision")

        self.instrument_id = instrument_id
        self.level = level
        self.price_precision = price_precision
        self.size_precision = size_precision
        self.bids = Ladder(is_bid=True)
        self.asks = Ladder(is_bid=False)
        self.last_update_timestamp_ns = 0
        self.last_update_id = 0

        # TODO: Id updates

    @staticmethod
    def create(
        InstrumentId instrument_id,
        OrderBookLevel level,
        int price_precision,
        int size_precision,
    ):
        """
        Create a new order book with the given parameters.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument identifier for the book.
        level : OrderBookLevel (Enum)
            The order book level (L1, L2, L3).
        price_precision : int
            The price precision for the book.
        size_precision : int
            The size precision for the book.

        Returns
        -------
        OrderBook

        Raises
        ------
        ValueError
            If price_precision is negative (< 0).
        ValueError
            If size_precision is negative (< 0).

        """
        Condition.not_none(instrument_id, "instrument_id")

        if level == OrderBookLevel.L1:
            return L1OrderBook(
                instrument_id=instrument_id,
                price_precision=price_precision,
                size_precision=size_precision,
            )
        elif level == OrderBookLevel.L2:
            return L2OrderBook(
                instrument_id=instrument_id,
                price_precision=price_precision,
                size_precision=size_precision,
            )
        elif level == OrderBookLevel.L3:
            return L3OrderBook(
                instrument_id=instrument_id,
                price_precision=price_precision,
                size_precision=size_precision,
            )
        else:
            raise RuntimeError(f"level was invalid, was {level} (must be in range [1, 3])")

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

        """
        Condition.not_none(delta, "delta")

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

        self.last_update_timestamp_ns = deltas.timestamp_ns

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
        # Use `update` instead of `add` (when book has been cleared they're equivalent) to make work for L1 Orderbook
        for bid in snapshot.bids:
            order = Order(
                price=Price(bid[0], precision=self.price_precision),
                volume=Quantity(bid[1], precision=self.size_precision),
                side=OrderSide.BUY
            )
            self.update(order=order)
        for ask in snapshot.asks:
            order = Order(
                price=Price(ask[0], precision=self.price_precision),
                volume=Quantity(ask[1], precision=self.size_precision),
                side=OrderSide.SELL
            )
            self.update(order=order)

        self.last_update_timestamp_ns = snapshot.timestamp_ns

    cpdef void apply(self, OrderBookData data) except *:
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
        Clear the bids from the book.
        """
        self.bids = Ladder(is_bid=True)

    cpdef void clear_asks(self) except *:
        """
        Clear the asks from the book.
        """
        self.asks = Ladder(is_bid=False)

    cpdef void clear(self) except *:
        """
        Clear the entire orderbook.
        """
        self.clear_bids()
        self.clear_asks()

    cdef inline void _apply_delta(self, OrderBookDelta delta) except *:
        if delta.type == OrderBookDeltaType.ADD:
            self.add(order=delta.order)
        elif delta.type == OrderBookDeltaType.UPDATE:
            self.update(order=delta.order)
        elif delta.type == OrderBookDeltaType.DELETE:
            self.delete(order=delta.order)

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
        double

        """
        cdef Level top_bid_level = self.bids.top()
        cdef Level top_ask_level = self.asks.top()
        if top_bid_level and top_ask_level:
            return top_ask_level.price() - top_bid_level.price()
        else:
            return None

    cpdef midpoint(self):
        cdef Level top_bid_level = self.bids.top()
        cdef Level top_ask_level = self.asks.top()
        if top_bid_level and top_ask_level:
            return float((top_ask_level.price() + top_bid_level.price()) / Price("2.0"))
        else:
            return None

    cpdef str pprint(self, int num_levels=3, show='volume'):
        levels = [(lvl.price(), lvl) for lvl in self.bids.levels[-num_levels:] + self.asks.levels[:num_levels]]
        levels = list(reversed(sorted(levels, key=itemgetter(0))))
        data = [
            {
                "bids": [
                    getattr(order, show).as_double()
                    for order in level.orders
                    if level.price() in self.bids.prices()
                ]
                or None,
                "price": level.price().as_double(),
                "asks": [
                    getattr(order, show).as_double()
                    for order in level.orders
                    if level.price() in self.asks.prices()
                ]
                or None,
            }
            for _, level in levels
        ]
        return tabulate(
            data, headers="keys", numalign="center", floatfmt=".4f", tablefmt="fancy"
        )

    @property
    def timestamp_ns(self):
        return self.last_update_timestamp_ns


cdef class L3OrderBook(OrderBook):
    """
    Provides an L3 order book.

    Maps directly to functionality of the `OrderBook` base class.
    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        int price_precision,
        int size_precision,
    ):
        """
        Initialize a new instance of the `L3OrderBook` class.

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
        Initialize a new instance of the `L2OrderBook` class.

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
        # normal orderbook checks.
        self._check_integrity()

        for level in self.bids.levels + self.asks.levels:
            assert len(level.orders) == 1, f"Number of orders on {level} > 1"

    cdef inline Order _process_order(self, Order order):
        # Because a L2OrderBook only has one order per level, we replace the
        # order.id with a price level, which will let us easily process the
        # order in the proxy orderbook.
        order.id = str(order.price)
        return order

    cdef inline void _remove_if_exists(self, Order order) except *:
        # For a L2OrderBook, an order update means a whole level update. If this
        # level exists, remove it so we can insert the new level.
        if order.side == OrderSide.BUY and order.price in self.bids.prices():
            self.delete(order)
        elif order.side == OrderSide.SELL and order.price in self.asks.prices():
            self.delete(order)


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
        Initialize a new instance of the `L1OrderBook` class.

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


cdef class OrderBookData(Data):
    """
    The abstract base class for all `OrderBook` data.

    This class should not be used directly, but through its concrete subclasses.
    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the `OrderBookData` class.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument identifier for the book.
        timestamp_ns : int64
            The Unix timestamp (nanos) of the snapshot.

        """
        super().__init__(timestamp_ns)

        self.instrument_id = instrument_id


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
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the `OrderBookSnapshot` class.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument identifier for the book.
        level : OrderBookLevel (Enum)
            The order book level (L1, L2, L3).
        bids : list
            The bids for the snapshot.
        asks : list
            The asks for the snapshot.
        timestamp_ns : int64
            The Unix timestamp (nanos) of the snapshot.

        """
        super().__init__(instrument_id, timestamp_ns)

        self.level = level
        self.bids = bids
        self.asks = asks

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"'{self.instrument_id}', "
                f"bids={self.bids}, "
                f"asks={self.asks}, "
                f"timestamp_ns={self.timestamp_ns})")


cdef class OrderBookDeltas(OrderBookData):
    """
    Represents bulk changes for an `OrderBook`.
    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        OrderBookLevel level,
        list deltas not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the `OrderBookDeltas` class.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument identifier for the book.
        level : OrderBookLevel (Enum)
            The order book level (L1, L2, L3).
        deltas : list[OrderBookDelta]
            The list of order book changes.
        timestamp_ns : int64
            The Unix timestamp (nanos) of the operations.

        """
        super().__init__(instrument_id, timestamp_ns)
        self.level = level
        self.deltas = deltas

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"'{self.instrument_id}', "
                f"{self.deltas}, "
                f"timestamp_ns={self.timestamp_ns})")


cdef class OrderBookDelta(OrderBookData):
    """
    Represents a single difference on an `OrderBook`.
    """

    def __init__(
        self,
        OrderBookDeltaType delta_type,
        Order order not None,
        InstrumentId instrument_id,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the `OrderBookDelta` class.

        Parameters
        ----------
        delta_type : OrderBookDeltaType
            The type of change (ADD, UPDATED, DELETE).
        order : Order
            The order to apply.
        timestamp_ns : int64
            The Unix timestamp (nanos) of the operation.

        """
        super().__init__(
            instrument_id=instrument_id,
            timestamp_ns=timestamp_ns,
        )
        self.type = delta_type
        self.order = order

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"op_type={OrderBookDeltaTypeParser.to_str(self.type)}, "
                f"order={self.order}, "
                f"timestamp_ns={self.timestamp_ns})")
