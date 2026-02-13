# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
from nautilus_trader.core.rust.model cimport ContingencyType
from nautilus_trader.model.identifiers cimport OrderListId
from nautilus_trader.model.orders.base cimport Order


cdef class OrderList:
    """
    Represents a list of bulk or related contingent orders.

    All orders must be for the same instrument ID.

    Parameters
    ----------
    order_list_id : OrderListId
        The order list ID.
    orders : list[Order]
        The contained orders list.

    Raises
    ------
    ValueError
        If `orders` is empty.
    ValueError
        If `orders` contains a type other than `Order`.
    ValueError
        If orders contain different instrument IDs (must all be the same instrument).

    """

    def __init__(
        self,
        OrderListId order_list_id not None,
        list orders not None,
    ) -> None:
        Condition.not_empty(orders, "orders")
        Condition.list_type(orders, Order, "orders")
        cdef Order first = orders[0]
        cdef Order order
        for order in orders:
            # First condition check avoids creating an f-string for performance reasons
            if order.instrument_id != first.instrument_id:
                Condition.is_true(
                    order.instrument_id == first.instrument_id,
                    f"order.instrument_id {order.instrument_id} != instrument_id {first.instrument_id}; "
                    "all orders in the list must be for the same instrument ID",
                )

        self.id = order_list_id
        self.instrument_id = first.instrument_id
        self.strategy_id = first.strategy_id
        self.orders = orders
        self.first = first
        self.ts_init = first.ts_init

    def __eq__(self, OrderList other) -> bool:
        if other is None:
            return False
        return self.id == other.id

    def __hash__(self) -> int:
        return hash(self.id)

    def __len__(self) -> int:
        return len(self.orders)

    def __repr__(self) -> str:
        return (
            f"OrderList("
            f"id={self.id.to_str()}, "
            f"instrument_id={self.instrument_id}, "
            f"strategy_id={self.strategy_id}, "
            f"orders={self.orders})"
        )

    cpdef bint is_bracket(self):
        """
        Return whether this order list represents a bracket order.

        A bracket order has exactly 3 orders: an entry order (OTO contingency)
        with exactly 2 child orders (OUO contingency, not OCO) that are
        reduce-only TP/SL orders.

        Returns
        -------
        bool

        """
        if len(self.orders) != 3:
            return False

        cdef Order entry = self.first
        if entry.contingency_type != ContingencyType.OTO:
            return False

        cdef Order child
        cdef int ouo_child_count = 0
        for child in self.orders[1:]:
            if child.parent_order_id != entry.client_order_id:
                return False
            if child.contingency_type != ContingencyType.OUO:
                return False
            if not child.is_reduce_only:
                return False
            ouo_child_count += 1

        return ouo_child_count == 2
