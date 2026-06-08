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
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport OrderListId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.orders.base cimport Order


cdef class OrderList:
    """
    Represents a list of bulk or related contingent orders.

    All orders must share the same venue. They may target different instruments
    at that venue (e.g. pairs, calendar spreads, multi-leg legs); the list's
    `instrument_id` is taken from the first order as a representative value.

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
        If orders contain different venues (must all share the same venue).

    """

    def __init__(
        self,
        OrderListId order_list_id not None,
        list orders not None,
    ) -> None:
        Condition.not_empty(orders, "orders")
        Condition.list_type(orders, Order, "orders")
        cdef Order first_order = orders[0]
        cdef Venue first_venue = first_order.instrument_id.venue
        cdef Order order
        for order in orders:
            # First condition check avoids creating an f-string for performance reasons
            if order.instrument_id.venue != first_venue:
                Condition.is_true(
                    order.instrument_id.venue == first_venue,
                    f"order.instrument_id.venue {order.instrument_id.venue} != venue {first_venue}; "
                    "all orders in the list must share the same venue",
                )

        self.id = order_list_id
        self.instrument_id = first_order.instrument_id
        self.strategy_id = first_order.strategy_id
        self.orders = orders
        self.first = first_order
        self.ts_init = first_order.ts_init

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

    cpdef set instrument_ids(self):
        """
        Return the set of distinct instrument IDs across all orders in the list.

        Returns
        -------
        set[InstrumentId]

        """
        cdef set ids = set()
        cdef Order order
        for order in self.orders:
            ids.add(order.instrument_id)
        return ids

    cpdef bint is_uniform_instrument(self):
        """
        Return whether all orders in the list share the same instrument ID.

        Returns
        -------
        bool

        """
        cdef InstrumentId first_id = self.first.instrument_id
        cdef Order order
        for order in self.orders:
            if order.instrument_id != first_id:
                return False
        return True

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
