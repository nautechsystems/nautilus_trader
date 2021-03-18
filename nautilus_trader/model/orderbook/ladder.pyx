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

from bisect import bisect

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.orderbook.level cimport Level
from nautilus_trader.model.orderbook.order cimport Order


cdef class Ladder:
    """
    Represents a ladder of orders in a book.
    """
    def __init__(self, bint reverse):
        """
        Initialize a new instance of the `Ladder` class.

        Parameters
        ----------
        reverse : bool
            If the ladder should be represented in reverse order of price.

        """
        self.reverse = reverse
        self.levels = []           # type: list[Level]
        self.order_id_levels = {}  # type: dict[str, Level]

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
            price_idx = bisect(existing_prices, level.price())
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
        return list(reversed(self.levels[-n:])) if self.reverse else self.levels[:n]

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

    cpdef Level top(self):
        """
        The exposures in the ladder.

        Returns
        -------
        Level

        """
        cdef list top = self.depth(1)
        if len(top) > 0:
            return top[0]
