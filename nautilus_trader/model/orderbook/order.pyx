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
from nautilus_trader.core.uuid import uuid4

from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.order_side cimport OrderSideParser


cdef class Order:
    """
    Represents an order in a book.
    """

    def __init__(
        self,
        double price,
        double size,
        OrderSide side,
        str id=None,  # noqa (shadows built-in name)
    ):
        """
        Initialize a new instance of the ``Order`` class.

        Parameters
        ----------
        price : double
            The order price.
        size : double
            The order size.
        side : OrderSide
            The order side.
        id : str
            The order ID.

        """
        self.price = price
        self.size = size
        self.side = side
        self.id = id or str(uuid4())

    def __eq__(self, Order other) -> bool:
        return self.id == other.id

    def __hash__(self) -> int:
        return hash(frozenset(Order.to_dict_c(self)))

    def __repr__(self) -> str:
        return f"{Order.__name__}({self.price}, {self.size}, {OrderSideParser.to_str(self.side)}, {self.id})"

    cpdef void update_price(self, double price) except *:
        """
        Update the orders price.

        Parameters
        ----------
        price : double
            The updated price.

        """
        self.price = price

    cpdef void update_size(self, double size) except *:
        """
        Update the orders size.

        Parameters
        ----------
        size : double
            The updated size.

        """
        self.size = size

    cpdef void update_id(self, str value) except *:
        """
        Update the orders ID.

        Parameters
        ----------
        value : str
            The updated ID.

        """
        self.id = value

    cpdef double exposure(self):
        """
        Return the total exposure for this order (price * size).

        Returns
        -------
        double

        """
        return self.price * self.size

    cpdef double signed_size(self):
        """
        Return the signed size of the order (negative for SELL).

        Returns
        -------
        double

        """
        if self.side == OrderSide.BUY:
            return self.size * 1.0
        else:
            return self.size * -1.0

    @staticmethod
    cdef Order from_dict_c(dict values):
        Condition.not_none(values, "values")
        return Order(
            price=values["price"],
            size=values["size"],
            side=OrderSideParser.from_str(values["side"]),
            id=values["id"],
        )

    @staticmethod
    cdef dict to_dict_c(Order obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "Order",
            "price": obj.price,
            "size": obj.size,
            "side": OrderSideParser.to_str(obj.side),
            "id": obj.id,
        }

    @staticmethod
    def from_dict(dict values):
        """
        Return an order from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        Order

        """
        return Order.from_dict_c(values)

    @staticmethod
    def to_dict(Order obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return Order.to_dict_c(obj)
