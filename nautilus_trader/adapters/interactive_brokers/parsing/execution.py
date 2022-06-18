# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

from ib_insync import LimitOrder as IBLimitOrder
from ib_insync import MarketOrder as IBMarketOrder
from ib_insync import Order as IBOrder

from nautilus_trader.model.c_enums.order_side import OrderSideParser
from nautilus_trader.model.orders.base import Order as NautilusOrder
from nautilus_trader.model.orders.limit import LimitOrder as NautilusLimitOrder
from nautilus_trader.model.orders.market import MarketOrder as NautilusMarketOrder


def nautilus_order_to_ib_order(order: NautilusOrder) -> IBOrder:
    if isinstance(order, NautilusMarketOrder):
        return IBMarketOrder(
            action=OrderSideParser.to_str_py(order.side),
            totalQuantity=order.quantity.as_double(),
        )
    elif isinstance(order, NautilusLimitOrder):
        # TODO - Time in force, etc
        return IBLimitOrder(
            action=OrderSideParser.to_str_py(order.side),
            lmtPrice=order.price.as_double(),
            totalQuantity=order.quantity.as_double(),
        )
    else:
        raise NotImplementedError(f"IB order type not implemented {type(order)} for {order}")
