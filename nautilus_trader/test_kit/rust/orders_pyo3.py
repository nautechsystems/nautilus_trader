# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.core.nautilus_pyo3 import ClientOrderId
from nautilus_trader.core.nautilus_pyo3 import MarketOrder
from nautilus_trader.core.nautilus_pyo3 import OrderSide
from nautilus_trader.core.nautilus_pyo3 import Quantity
from nautilus_trader.core.nautilus_pyo3 import StrategyId
from nautilus_trader.core.nautilus_pyo3 import TimeInForce
from nautilus_trader.test_kit.rust.identifiers_pyo3 import TestIdProviderPyo3


class TestOrderProviderPyo3:
    @staticmethod
    def market_order(
        instrument_id=None,
        order_side=None,
        quantity=None,
        trader_id=None,
        strategy_id: StrategyId | None = None,
        client_order_id: ClientOrderId | None = None,
        time_in_force=None,
    ) -> MarketOrder:
        return MarketOrder(
            trader_id=trader_id or TestIdProviderPyo3.trader_id(),
            strategy_id=strategy_id or TestIdProviderPyo3.strategy_id(),
            instrument_id=instrument_id or TestIdProviderPyo3.audusd_id(),
            client_order_id=client_order_id or TestIdProviderPyo3.client_order_id(),
            order_side=order_side or OrderSide.BUY,
            quantity=quantity or Quantity.from_str("100"),
            time_in_force=time_in_force or TimeInForce.GTC,
            init_id=TestIdProviderPyo3.uuid(),
            ts_init=0,
            reduce_only=False,
            contingency_type=None,
            order_list_id=None,
            linked_order_ids=None,
            parent_order_id=None,
            tags=None,
        )
