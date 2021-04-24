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

# TODO - Instead of a Level.orders being a list (python-land) could use structured arrays?
# https://docs.scipy.org/doc/numpy-1.13.0/user/basics.rec.html


from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.orderbook.order cimport Order


cdef class Level:
    """
    Represents an `OrderBook` level.

    A price level on one side of the `OrderBook` with one or more individual orders.
    """

    def __init__(self, list orders=None):
        """
        Initialize a new instance of the `Level` class.

        Parameters
        ----------
        orders : list[Order]
            The initial orders for the level.

        """
        self.orders = []
        for order in orders or []:
            self.add(order)

    def __eq__(self, Level other) -> bool:
        return self.price() == other.price()

    def __lt__(self, Level other) -> bool:
        return self.price() < other.price()

    def __le__(self, Level other) -> bool:
        return self.price() <= other.price()

    def __gt__(self, Level other) -> bool:
        return self.price() > other.price()

    def __ge__(self, Level other) -> bool:
        return self.price() >= other.price()

    def __repr__(self) -> str:
        return f"Level(price={self.price()}, orders={self.orders[:5]})"

    cpdef void add(self, Order order) except *:
        """
        Add the given order to this level.

        Parameters
        ----------
        order : Order
            The order to add.

        """
        Condition.not_none(order, "order")

        self._check_price(order=order)
        self.orders.append(order)

    cpdef void update(self, Order order) except *:
        """
        Update the given order on this level.

        Parameters
        ----------
        order : Order
            The order to update.

        Raises
        ------
        KeyError
            If the order is not found at this level.

        """
        Condition.not_none(order, "order")
        if self.orders:
            Condition.equal(order.price, self.orders[0].price, "order.price", "self.orders[0].price")

        if order.volume == 0:
            self.delete(order=order)
        else:
            existing = self.orders[self.orders.index(order)]
            if existing is None:
                raise KeyError("Cannot update order: order not found.")
            existing.update_volume(volume=order.volume)

    cpdef void delete(self, Order order) except *:
        """
        Delete the given order from this level.

        Parameters
        ----------
        order : Order
            The order to delete.

        """
        Condition.not_none(order, "order")

        self.orders.remove(order)

    cpdef price(self):
        """
        Return the price for this level.

        Returns
        -------
        double or None

        """
        if len(self.orders) > 0:
            return self.orders[0].price
        else:
            return None

    cpdef double volume(self) except *:
        """
        Return the volume at this level.

        Returns
        -------
        double

        """
        return sum([order.volume for order in self.orders])

    cpdef double exposure(self):
        """
        Return the exposure at this level (price * volume).

        Returns
        -------
        double

        """
        return self.price() * self.volume()

    cdef inline bint _check_price(self, Order order) except *:
        if not self.orders:
            return True
        return order.price == self.orders[0].price
