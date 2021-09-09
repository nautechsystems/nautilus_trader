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
from nautilus_trader.model.identifiers cimport OrderListId
from nautilus_trader.model.orders.base cimport Order


cdef class OrderList:
    """
    Represents a list of bulk or related parent-child contingent orders.
    """

    def __init__(
        self,
        OrderListId list_id not None,
        list orders not None,
    ):
        """
        Initialize a new instance of the ``OrderList`` class.

        Parameters
        ----------
        list_id : OrderListId
            The order list ID.
        orders : list[Order]
            The order bulk for the list.

        Raises
        ------
        ValueError
            If `orders` is empty.
        ValueError
            If `orders` contains a type other than `Order`.

        """
        Condition.not_empty(orders, "orders")
        Condition.list_type(orders, Order, "orders")

        cdef Order first = orders[0]
        self.id = list_id
        self.instrument_id = first.instrument_id
        self.orders = orders
        self.first = first
        self.ts_init = first.ts_init

    def __eq__(self, OrderList other) -> bool:
        return self.id.value == other.id.value

    def __hash__(self) -> int:
        return hash(self.id.value)

    def __repr__(self) -> str:
        return f"OrderList(id={self.id.value}, instrument_id={self.instrument_id.value}, orders={self.orders})"
