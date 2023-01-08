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
from nautilus_trader.model.identifiers cimport OrderListId
from nautilus_trader.model.orders.base cimport Order


cdef class OrderList:
    """
    Represents a list of bulk or related parent-child contingent orders.

    Parameters
    ----------
    order_list_id : OrderListId
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

    def __init__(
        self,
        OrderListId order_list_id not None,
        list orders not None,
    ):
        Condition.not_empty(orders, "orders")
        Condition.list_type(orders, Order, "orders")

        cdef Order first = orders[0]
        self.id = order_list_id
        self.instrument_id = first.instrument_id
        self.strategy_id = first.strategy_id
        self.orders = orders
        self.first = first
        self.ts_init = first.ts_init

    def __eq__(self, OrderList other) -> bool:
        return self.id == other.id

    def __hash__(self) -> int:
        return hash(self.id)

    def __repr__(self) -> str:
        return (
            f"OrderList("
            f"id={self.id.to_str()}, "
            f"instrument_id={self.instrument_id}, "
            f"strategy_id={self.strategy_id}, "
            f"orders={self.orders})"
        )
