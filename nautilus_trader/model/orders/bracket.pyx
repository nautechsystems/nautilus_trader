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
from nautilus_trader.model.identifiers cimport ClientOrderLinkId
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.orders.limit cimport LimitOrder
from nautilus_trader.model.orders.stop_market cimport StopMarketOrder


cdef class BracketOrder:
    """
    Represents a bracket order.

    A bracket order is designed to help limit a traders loss and optionally
    lock in a profit by "bracketing" an entry order with two opposite-side exit
    orders. A BUY order is bracketed by a high-side sell order and a
    low-side sell stop order. A SELL order is bracketed by a high-side buy stop
    order and a low-side buy order.

    Once the 'parent' entry order is triggered the 'child' OCO orders being a
    `StopMarketOrder` and take-profit `LimitOrder` automatically become
    working on the exchange.
    """

    def __init__(
        self,
        Order entry not None,
        StopMarketOrder stop_loss not None,
        LimitOrder take_profit not None,
    ):
        """
        Initialize a new instance of the ``BracketOrder`` class.

        Parameters
        ----------
        entry : Order
            The entry 'parent' order.
        stop_loss : StopMarketOrder
            The stop-loss (SL) 'child' order.
        take_profit : Limit
            The take-profit (TP) 'child' order.

        Raises
        ------
        ValueError
            If entry.quantity != stop_loss.quantity.
        ValueError
            If entry.quantity != take_profit.quantity.

        """
        Condition.equal(entry.quantity, stop_loss.quantity, "entry.quantity", "stop_loss.quantity")
        Condition.equal(entry.quantity, take_profit.quantity, "entry.quantity", "take_profit.quantity")

        self.id = ClientOrderLinkId(f"B{entry.client_order_id.value}")
        self.instrument_id = entry.instrument_id
        self.entry = entry
        self.stop_loss = stop_loss
        self.take_profit = take_profit
        self.timestamp_ns = entry.timestamp_ns

    def __eq__(self, BracketOrder other) -> bool:
        return self.id.value == other.id.value

    def __repr__(self) -> str:
        return f"BracketOrder(id={self.id.value}, Entry{self.entry}, SL={self.stop_loss.price}, TP={str(self.take_profit.price)})"
