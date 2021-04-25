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

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.functions cimport bisect_double_right
from nautilus_trader.model.c_enums.depth_type cimport DepthType
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orderbook.level cimport Level
from nautilus_trader.model.orderbook.order cimport Order


cdef class Ladder:
    """
    Represents a ladder of orders in a book.
    """
    def __init__(
        self,
        bint reverse,
        int price_precision,
        int size_precision,
    ):
        """
        Initialize a new instance of the `Ladder` class.

        Parameters
        ----------
        reverse : bool
            If the ladder should be represented in reverse order of price (bids).
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
        Condition.not_negative_int(price_precision, "price_precision")
        Condition.not_negative_int(size_precision, "size_precision")

        self._order_id_level_index = {}  # type: dict[str, Level]

        self.levels = []  # type: list[Level]  # TODO: Make levels private??
        self.reverse = reverse
        self.price_precision = price_precision
        self.size_precision = size_precision

    def __repr__(self) -> str:
        return f"{Ladder.__name__}({self.levels})"

    cpdef void add(self, Order order) except *:
        """
        Add the given order to the ladder.

        Parameters
        ----------
        order : Order
            The order to add.

        """
        Condition.not_none(order, "order")

        cdef list existing_prices = self.prices()

        # Level exists, add new order
        cdef int price_idx
        cdef Level level
        if order.price in existing_prices:
            price_idx = existing_prices.index(order.price)
            level = self.levels[price_idx]
            level.add(order=order)
        # New price, create Level
        else:
            level = Level(price=order.price)
            level.add(order)
            price_idx = bisect_double_right(existing_prices, level.price)
            self.levels.insert(price_idx, level)

        self._order_id_level_index[order.id] = level

    cpdef void update(self, Order order) except *:
        """
        Update the given order in the ladder.

        Parameters
        ----------
        order : Order
            The order to add.

        """
        Condition.not_none(order, "order")

        if order.id not in self._order_id_level_index:
            self.add(order=order)
            return

        # Find the existing order
        cdef Level level = self._order_id_level_index[order.id]
        if order.price == level.price:
            # This update contains a volume update
            level.update(order=order)
            if not level.orders:
                self.levels.remove(level)  # <-- TODO: This is new
        else:
            # New price for this order, delete and insert
            self.delete(order=order)
            self.add(order=order)

    cpdef void delete(self, Order order) except *:
        """
        Delete the given order in the ladder.

        Parameters
        ----------
        order : Order

        Raises
        ------
        KeyError
            If order.id is not contained in the order_id_level_index.

        """
        Condition.not_none(order, "order")

        cdef Level level = self._order_id_level_index.get(order.id)
        if level is None:
            return
            # TODO: raise KeyError("Cannot delete order: not found at level.")
        cdef int price_idx = self.prices().index(level.price)
        level.delete(order=order)
        self._order_id_level_index.pop(order.id)
        if not level.orders:
            del self.levels[price_idx]

    cpdef list depth(self, int n=1):
        """
        Return the levels in the ladder to the given depth.

        Parameters
        ----------
        n : int
            The maximum level to query.

        Returns
        -------
        list[Level]

        """
        if not self.levels:
            return []
        n = n or len(self.levels)
        return list(reversed(self.levels[-n:])) if self.reverse else self.levels[:n]

    cpdef list prices(self):
        """
        The prices in the ladder.

        Returns
        -------
        list[double]

        """
        return [level.price for level in self.levels]

    cpdef list volumes(self):
        """
        The volumes in the ladder.

        Returns
        -------
        list[double]

        """
        return [level.volume() for level in self.levels]

    cpdef list exposures(self):
        """
        The exposures in the ladder.

        Returns
        -------
        list[double]

        """
        return [level.exposure() for level in self.levels]

    cpdef Level top(self):
        """
        The top `Level` in the ladder.

        Returns
        -------
        Level or None

        """
        cdef list top = self.depth(1)
        if top:
            return top[0]
        else:
            return None

    cpdef tuple simulate_order_fill(self, Order order, DepthType depth_type=DepthType.VOLUME):
        """
        Return a simulation of where this order would be filled in the ladder.

        Parameters
        ----------
        order : Order
            The order to simulate.
        depth_type : DepthType (Enum)
            The depth type to simulate.

        Returns
        -------
        (Price, Quantity)

        """
        Condition.not_none(order, "order")

        cdef int level_idx = 0
        cdef int order_idx = 0
        cdef Order book_order
        cdef double cumulative_numerator = 0.0
        cdef double cumulative_denominator = 0.0
        cdef double current = 0.0
        cdef double target = order.volume if depth_type == DepthType.VOLUME else order.price * order.volume
        cdef bint completed = False

        for level_idx in reversed(range(len(self.levels))) if self.reverse else range(len(self.levels)):
            if self.reverse and self.levels[level_idx].price < order.price:
                break
            elif not self.reverse and self.levels[level_idx].price > order.price:
                break
            for order_idx in range(len(self.levels[level_idx].orders)):
                book_order = self.levels[level_idx].orders[order_idx]
                current = book_order.volume if depth_type == DepthType.VOLUME else book_order.exposure()
                if (cumulative_denominator + current) >= target:
                    # This order has filled us, calc and return
                    remainder = target - cumulative_denominator
                    cumulative_numerator += book_order.price * remainder
                    cumulative_denominator += remainder
                    completed = True
                else:
                    # Add this order and continue
                    cumulative_numerator += book_order.price * current
                    cumulative_denominator += current
        if cumulative_denominator:
            return (
                Price(cumulative_numerator / cumulative_denominator, precision=self.price_precision),
                Quantity(cumulative_denominator, precision=self.size_precision),
            )
        else:
            return (
                Price(0, precision=self.price_precision),
                Quantity(0, precision=self.size_precision),
            )

    cpdef tuple depth_at_price(self, double price, DepthType depth_type=DepthType.VOLUME):
        """
        Find the total volume or exposure and average price that an order
        inserted at `price` would be filled for.

        Parameters
        ----------
        price : double
            The price for the calculation.
        depth_type : DepthType (Enum)
            The depth type.

        Returns
        -------
        (Price, Quantity)

        """
        cdef int level_idx = 0
        cdef int order_idx = 0
        cdef Order order
        cdef double cumulative_numerator = 0.0
        cdef double cumulative_denominator = 0.0
        cdef double current = 0.0

        for level_idx in reversed(range(len(self.levels))) if self.reverse else range(len(self.levels)):
            if self.reverse and self.levels[level_idx].price < price or not self.reverse and self.levels[level_idx].price > price:
                break
            for order_idx in range(len(self.levels[level_idx].orders)):
                order = self.levels[level_idx].orders[order_idx]
                current = order.volume if depth_type == DepthType.VOLUME else order.exposure()
                cumulative_numerator += order.price * current
                cumulative_denominator += current
        if cumulative_denominator:
            return (
                Price(cumulative_numerator / cumulative_denominator, precision=self.price_precision),
                Quantity(cumulative_denominator, precision=self.size_precision),
            )
        else:
            return (
                Price(0, precision=self.price_precision),
                Quantity(0, precision=self.size_precision),
            )

    cpdef tuple volume_fill_price(self, double volume):
        """
        Returns the average price that a certain volume order would be filled at.

        Parameters
        ----------
        volume : double
            The volume to be filled.

        Returns
        -------
        (Price, Quantity)

        """
        return self._depth_for_value(target=volume, depth_type=DepthType.VOLUME)

    cpdef tuple exposure_fill_price(self, double exposure):
        """
        Returns the average price that a certain exposure order would be filled at.

        Parameters
        ----------
        exposure : double
            The exposure amount.

        Returns
        -------
        (Price, Quantity)

        """
        return self._depth_for_value(target=exposure, depth_type=DepthType.EXPOSURE)

    cdef tuple _depth_for_value(self, double target, DepthType depth_type=DepthType.VOLUME):
        # Find the levels in this ladder required to fill a certain volume or exposure
        cdef int level_idx = 0
        cdef int order_idx = 0
        cdef Order order
        cdef double cumulative_numerator = 0.0
        cdef double cumulative_denominator = 0.0
        cdef double current = 0.0
        cdef double remainder = 0.0
        cdef bint completed = False

        for level_idx in reversed(range(len(self.levels))) if self.reverse else range(len(self.levels)):
            if completed:
                break
            for order_idx in range(len(self.levels[level_idx].orders)):
                order = self.levels[level_idx].orders[order_idx]
                current = order.volume if depth_type == DepthType.VOLUME else order.exposure()
                if (cumulative_denominator + current) >= target:
                    # This order has filled us, calc and return
                    remainder = target - cumulative_denominator
                    cumulative_numerator += order.price * remainder
                    cumulative_denominator += remainder
                    completed = True
                else:
                    # Add this order and continue
                    cumulative_numerator += order.price * current
                    cumulative_denominator += current
        if cumulative_denominator:
            return (
                Price(cumulative_numerator / cumulative_denominator, precision=self.price_precision),
                Quantity(cumulative_denominator, precision=self.size_precision),
            )
        else:
            return (
                Price(0, precision=self.price_precision),
                Quantity(0, precision=self.size_precision),
            )
