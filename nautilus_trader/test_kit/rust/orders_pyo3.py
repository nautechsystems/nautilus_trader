# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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
from nautilus_trader.core.nautilus_pyo3 import ExecAlgorithmId
from nautilus_trader.core.nautilus_pyo3 import InstrumentId
from nautilus_trader.core.nautilus_pyo3 import LimitOrder
from nautilus_trader.core.nautilus_pyo3 import MarketOrder
from nautilus_trader.core.nautilus_pyo3 import OrderSide
from nautilus_trader.core.nautilus_pyo3 import Price
from nautilus_trader.core.nautilus_pyo3 import Quantity
from nautilus_trader.core.nautilus_pyo3 import StopLimitOrder
from nautilus_trader.core.nautilus_pyo3 import StrategyId
from nautilus_trader.core.nautilus_pyo3 import TimeInForce
from nautilus_trader.core.nautilus_pyo3 import TraderId
from nautilus_trader.core.nautilus_pyo3 import TriggerType
from nautilus_trader.test_kit.rust.identifiers_pyo3 import TestIdProviderPyo3


class TestOrderProviderPyo3:
    @staticmethod
    def market_order(
        instrument_id: InstrumentId | None = None,
        order_side: OrderSide | None = None,
        quantity: Quantity | None = None,
        trader_id: TraderId | None = None,
        strategy_id: StrategyId | None = None,
        client_order_id: ClientOrderId | None = None,
        time_in_force: TimeInForce | None = None,
    ) -> MarketOrder:
        return MarketOrder(
            trader_id=trader_id or TestIdProviderPyo3.trader_id(),
            strategy_id=strategy_id or TestIdProviderPyo3.strategy_id(),
            instrument_id=instrument_id or TestIdProviderPyo3.audusd_id(),
            client_order_id=client_order_id or TestIdProviderPyo3.client_order_id(),
            order_side=order_side or OrderSide.BUY,
            quantity=quantity or Quantity.from_str("100"),
            time_in_force=time_in_force or TimeInForce.GTC,
            reduce_only=False,
            quote_quantity=False,
            init_id=TestIdProviderPyo3.uuid(),
            ts_init=0,
        )

    @staticmethod
    def limit_order(
        instrument_id: InstrumentId,
        order_side: OrderSide,
        quantity: Quantity,
        price: Price,
        trader_id: TraderId | None = None,
        strategy_id: StrategyId | None = None,
        client_order_id: ClientOrderId | None = None,
        time_in_force: TimeInForce | None = None,
        exec_algorithm_id: ExecAlgorithmId | None = None,
    ) -> LimitOrder:
        return LimitOrder(
            trader_id=trader_id or TestIdProviderPyo3.trader_id(),
            strategy_id=strategy_id or TestIdProviderPyo3.strategy_id(),
            instrument_id=instrument_id or TestIdProviderPyo3.audusd_id(),
            client_order_id=client_order_id or TestIdProviderPyo3.client_order_id(1),
            order_side=order_side or OrderSide.BUY,
            quantity=quantity or Quantity.from_str("100"),
            time_in_force=time_in_force or TimeInForce.GTC,
            price=price,
            post_only=False,
            reduce_only=False,
            quote_quantity=False,
            init_id=TestIdProviderPyo3.uuid(),
            ts_init=0,
            exec_algorithm_id=exec_algorithm_id,
            exec_spawn_id=TestIdProviderPyo3.client_order_id(1),
        )

    @staticmethod
    def stop_limit_order(
        instrument_id: InstrumentId,
        order_side: OrderSide,
        quantity: Quantity,
        price: Price,
        trigger_price: Price,
        trigger_type: TriggerType = TriggerType.MID_POINT,
        trader_id: TraderId | None = None,
        strategy_id: StrategyId | None = None,
        client_order_id: ClientOrderId | None = None,
        time_in_force: TimeInForce | None = None,
        exec_algorithm_id: ExecAlgorithmId | None = None,
        tags: list[str] | None = None,
    ) -> StopLimitOrder:
        return StopLimitOrder(
            trader_id=trader_id or TestIdProviderPyo3.trader_id(),
            strategy_id=strategy_id or TestIdProviderPyo3.strategy_id(),
            instrument_id=instrument_id or TestIdProviderPyo3.audusd_id(),
            client_order_id=client_order_id or TestIdProviderPyo3.client_order_id(1),
            order_side=order_side or OrderSide.BUY,
            quantity=quantity or Quantity.from_str("100"),
            price=price,
            trigger_price=trigger_price,
            trigger_type=trigger_type,
            time_in_force=time_in_force or TimeInForce.GTC,
            post_only=False,
            reduce_only=False,
            quote_quantity=False,
            init_id=TestIdProviderPyo3.uuid(),
            ts_init=0,
            exec_algorithm_id=exec_algorithm_id,
            exec_spawn_id=TestIdProviderPyo3.client_order_id(1),
            tags=tags,
        )
