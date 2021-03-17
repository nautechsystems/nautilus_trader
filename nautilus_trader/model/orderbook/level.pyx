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
import logging
from nautilus_trader.model.orderbook.order cimport Order


cdef class Level:
    """ A Orderbook level; A price level on one side of the Orderbook with one or more individual Orders"""

    def __init__(self, list orders=None):
        self.orders = []
        for order in orders or []:
            self.add(order)

    cpdef void add(self, Order order) except *:
        """
        Add an order to this level.
        :param order: New order
        :return:
        """
        self._check_price(order=order)
        self.orders.append(order)

    cpdef void update(self, Order order) except *:
        """
        Update an order on this level.
        :param order: New order
        :return:
        """
        self._check_price(order=order)
        if order.volume == 0:
            self.delete(order=order)
        else:
            existing = self.orders[self.orders.index(order)]
            if existing is None:
                logging.warning(f"Tried to update unknown order: {order}")
                return
            existing.update_volume(volume=order.volume)

    cpdef void delete(self, Order order) except *:
        """
        Delete an Order from this level
        :param order: Quantity of volume to delete
        :return:
        """
        self.orders.remove(order)

    cdef bint _check_price(self, Order order) except *:
        if not self.orders:
            return True
        err = "Order passed to `update` has wrong price! Should be handled in Ladder"
        assert order.price == self.orders[0].price, err

    @property
    def volume(self):
        return sum([order.volume for order in self.orders])

    @property
    def price(self):
        return self.orders[0].price

    def __eq__(self, other) -> bool:
        return self.price == other.price

    def __lt__(self, other) -> bool:
        return self.price < other.price

    def __le__(self, other) -> bool:
        return self.price <= other.price

    def __gt__(self, other) -> bool:
        return self.price > other.price

    def __ge__(self, other) -> bool:
        return self.price >= other.price

    def __repr__(self):
        return f"Level(price={self.price}, orders={self.orders[:5]})"
