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

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.orderbook.data cimport BookOrder


cdef class Level:
    """
    Represents an `OrderBook` level.

    A price level on one side of the `OrderBook` with one or more individual orders.

    Parameters
    ----------
    price : double
        The price for the level.
    """

    def __init__(self, double price):
        self.price = price
        self.orders = []

    def __eq__(self, Level other) -> bool:
        return self.price == other.price

    def __lt__(self, Level other) -> bool:
        return self.price < other.price

    def __le__(self, Level other) -> bool:
        return self.price <= other.price

    def __gt__(self, Level other) -> bool:
        return self.price > other.price

    def __ge__(self, Level other) -> bool:
        return self.price >= other.price

    def __repr__(self) -> str:
        return f"Level(price={self.price}, orders={self.orders[:5]})"

    cpdef void bulk_add(self, list orders) except *:
        """
        Add the list of bulk orders to this level.

        Parameters
        ----------
        orders : list[BookOrder]
            The orders to add.

        """
        cdef BookOrder order
        for order in orders:
            self.add(order)

    cpdef void add(self, BookOrder order) except *:
        """
        Add the given order to this level.

        Parameters
        ----------
        order : BookOrder
            The order to add.

        Raises
        ------
        ValueError
            If `order.price` is not equal to the levels price.

        """
        Condition.not_none(order, "order")
        Condition.equal(order.price, self.price, "order.price", "self.price")

        self.orders.append(order)

    cpdef void update(self, BookOrder order) except *:
        """
        Update the given order on this level.

        Parameters
        ----------
        order : BookOrder
            The order to update.

        Raises
        ------
        KeyError
            If `order` is not found at this level.

        """
        Condition.not_none(order, "order")
        Condition.equal(order.price, self.price, "order.price", "self.price")

        cdef BookOrder existing
        if order.size == 0:
            self.delete(order=order)
        else:
            existing = self.orders[self.orders.index(order)]
            if existing is None:
                raise KeyError("Cannot update order: order not found")
            existing.update_size(size=order.size)

    cpdef void delete(self, BookOrder order) except *:
        """
        Delete the given order from this level.

        Parameters
        ----------
        order : BookOrder
            The order to delete.

        """
        Condition.not_none(order, "order")

        self.orders.remove(order)

    cpdef double volume(self) except *:
        """
        Return the volume at this level.

        Returns
        -------
        double

        """
        return sum([order.size for order in self.orders])

    cpdef double exposure(self):
        """
        Return the exposure at this level (price * volume).

        Returns
        -------
        double

        """
        return self.price * self.volume()
