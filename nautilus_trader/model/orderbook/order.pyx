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
        double volume,
        OrderSide side,
        str id=None,
    ):
        """
        Initialize a new instance of the `Order` class.

        Parameters
        ----------
        price : double
            The order price.
        volume : double
            The order volume.
        side : OrderSide
            The order side.
        id : str
            The order identifier.

        """
        self.price = price
        self.volume = volume
        self.side = side
        self.id = id or str(uuid4())

    cpdef void update_price(self, double price) except *:
        """
        Update the orders price.

        Parameters
        ----------
        price : double
            The updated price.

        """
        self.price = price

    cpdef void update_volume(self, double volume) except *:
        """
        Update the orders volume.

        Parameters
        ----------
        volume : double
            The updated volume.

        """
        self.volume = volume

    cpdef void update_id(self, str value) except *:
        """
        Update the orders identifier to the given value.

        Parameters
        ----------
        value : str
            The updated identifier.

        """
        self.id = value

    cpdef double exposure(self):
        return self.price * self.volume

    cpdef double signed_volume(self):
        if self.side == OrderSide.BUY:
            return self.volume * 1.0
        else:
            return self.volume * -1.0

    def __eq__(self, Order other) -> bool:
        return self.id == other.id

    def __repr__(self) -> str:
        return f"Order({self.price}, {self.volume}, {OrderSideParser.to_str(self.side)}, {self.id})"
