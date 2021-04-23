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

import logging

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.functions cimport bisect_double_right
from nautilus_trader.model.c_enums.depth_type cimport DepthType
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.order.base cimport PassiveOrder
from nautilus_trader.model.orderbook.level cimport Level
from nautilus_trader.model.orderbook.order cimport Order


logger = logging.getLogger(__name__)


cdef class Ladder:
    """
    Represents a ladder of orders in a book.
    """
    def __init__(
        self,
        bint is_bid,
        int price_precision,
        int size_precision,
    ):
        """
        Initialize a new instance of the `Ladder` class.

        Parameters
        ----------
        is_bid : bool
            If the ladder should be represented in reverse order of price.
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

        self.is_bid = is_bid
        self.price_precision = price_precision
        self.size_precision = size_precision
        self.levels = []           # type: list[Level]
        self.order_id_levels = {}  # type: dict[str, Level]

    cpdef bint reverse(self) except *:
        return self.is_bid

    def __repr__(self):
        return f"Ladder({self.levels})"

    cpdef void add(self, Order order) except *:
        """
        Add the given order to the ladder.

        Parameters
        ----------
        order : Order
            The order to add.

        """
        Condition.not_none(order, "order")

        # Level exists, add new order
        cdef int price_idx
        cdef Level level
        existing_prices = self.prices()
        if order.price in existing_prices:
            price_idx = existing_prices.index(order.price)
            level = self.levels[price_idx]
            level.add(order=order)
        # New price, create Level
        else:
            level = Level(orders=[order])
            price_idx = bisect_double_right(existing_prices, level.price())
            self.levels.insert(price_idx, level)
        self.order_id_levels[order.id] = level

    cpdef void update(self, Order order) except *:
        """
        Update the given order in the ladder.

        Parameters
        ----------
        order : Order
            The order to add.

        """
        Condition.not_none(order, "order")

        if order.id not in self.order_id_levels:
            self.add(order=order)
            return

        # Find the existing order
        cdef Level level = self.order_id_levels[order.id]
        if order.price == level.price():
            # This update contains a volume update
            level.update(order=order)
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

        """
        Condition.not_none(order, "order")
        if order.id not in self.order_id_levels:
            # TODO - we could emit a better error here about book integrity?
            logger.warning(f"Couldn't find order_id {order.id} in levels, SKIPPING!")
            return
        cdef Level level = self.order_id_levels[order.id]
        cdef int price_idx = self.prices().index(level.price())
        level.delete(order=order)
        del self.order_id_levels[order.id]
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
        return list(reversed(self.levels[-n:])) if self.reverse() else self.levels[:n]

    cpdef list prices(self):
        """
        The prices in the ladder.

        Returns
        -------
        list[double]

        """
        return [level.price() for level in self.levels]

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
        The top Level in the ladder.

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
        """ Simulate where this order would be filled in the ladder """
        cdef int level_idx = 0
        cdef int order_idx = 0
        cdef Order book_order
        cdef double cumulative_numerator = 0.0
        cdef double cumulative_denominator = 0.0
        cdef double current = 0.0
        cdef double target = order.volume if depth_type == DepthType.VOLUME else order.price * order.volume
        cdef bint completed = False

        for level_idx in reversed(range(len(self.levels))) if self.is_bid else range(len(self.levels)):
            if self.is_bid and self.levels[level_idx].price() < order.price:
                break
            elif not self.is_bid and self.levels[level_idx].price() > order.price:
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
        Find the total volume or exposure and average price that an  order inserted at `price` would be filled for.

        Parameters
        ----------
        price : double
            The price for the calculation.
        depth_type : DepthType (Enum)
            The depth type.

        """
        cdef int level_idx = 0
        cdef int order_idx = 0
        cdef Order order
        cdef double cumulative_numerator = 0.0
        cdef double cumulative_denominator = 0.0
        cdef double current = 0.0

        for level_idx in reversed(range(len(self.levels))) if self.is_bid else range(len(self.levels)):
            if self.is_bid and self.levels[level_idx].price() < price or not self.is_bid and self.levels[level_idx].price() > price:
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
        Price or None

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
        Price or None

        """
        return self._depth_for_value(target=exposure, depth_type=DepthType.EXPOSURE)

    cdef tuple _depth_for_value(self, double target, DepthType depth_type=DepthType.VOLUME):
        """
        Find the levels in this ladder required to fill a certain volume or exposure.
        """
        cdef int level_idx = 0
        cdef int order_idx = 0
        cdef Order order
        cdef double cumulative_numerator = 0.0
        cdef double cumulative_denominator = 0.0
        cdef double current = 0.0
        cdef double remainder = 0.0
        cdef bint completed = False

        for level_idx in reversed(range(len(self.levels))) if self.is_bid else range(len(self.levels)):
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
