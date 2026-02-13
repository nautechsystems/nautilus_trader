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

from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.component import TestClock
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.manager import OrderManager
from nautilus_trader.model.enums import ContingencyType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import OrderListId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import LimitOrder
from nautilus_trader.model.orders import MarketOrder
from nautilus_trader.model.orders import StopMarketOrder
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


ETHUSDT_PERP_BINANCE = TestInstrumentProvider.ethusdt_perp_binance()


class TestOrderManager:
    def setup(self) -> None:
        self.clock = TestClock()
        self.trader_id = TestIdStubs.trader_id()
        self.strategy_id = TestIdStubs.strategy_id()

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
        )

        self.cache = Cache(database=None)

        self.modify_calls: list[tuple] = []
        self.submit_calls: list[tuple] = []

        def mock_submit_handler(order):
            self.submit_calls.append(order.client_order_id)

        def mock_modify_handler(order, quantity):
            # Record the call but don't actually update the order
            # This simulates the async nature where OrderUpdated hasn't arrived yet
            self.modify_calls.append((order.client_order_id, quantity))

        self.manager = OrderManager(
            clock=self.clock,
            msgbus=self.msgbus,
            cache=self.cache,
            component_name="TestManager",
            active_local=True,  # Manage local orders (INITIALIZED, EMULATED, RELEASED)
            submit_order_handler=mock_submit_handler,
            cancel_order_handler=None,
            modify_order_handler=mock_modify_handler,
            debug=False,
        )

    def test_oto_rapid_fills_sends_modify_for_each_fill(self) -> None:
        # Arrange
        # Regression test for https://github.com/nautechsystems/nautilus_trader/issues/3435
        # When parent fills arrive rapidly before OrderUpdated events for children,
        # the manager should still send modify commands for each fill
        order_list_id = OrderListId("OL-001")
        entry_id = ClientOrderId("O-001")
        sl_id = ClientOrderId("O-002")
        tp_id = ClientOrderId("O-003")

        entry_order = MarketOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            client_order_id=entry_id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_str("0.030"),
            init_id=UUID4(),
            ts_init=0,
            time_in_force=TimeInForce.GTC,
            contingency_type=ContingencyType.OTO,
            order_list_id=order_list_id,
            linked_order_ids=[sl_id, tp_id],
        )

        sl_order = StopMarketOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            client_order_id=sl_id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_str("0.030"),
            trigger_price=Price.from_str("4900.00"),
            trigger_type=TriggerType.DEFAULT,
            init_id=UUID4(),
            ts_init=0,
            time_in_force=TimeInForce.GTC,
            contingency_type=ContingencyType.OCO,
            order_list_id=order_list_id,
            linked_order_ids=[tp_id],
            parent_order_id=entry_id,
        )

        tp_order = LimitOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            client_order_id=tp_id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_str("0.030"),
            price=Price.from_str("5100.00"),
            init_id=UUID4(),
            ts_init=0,
            time_in_force=TimeInForce.GTC,
            contingency_type=ContingencyType.OCO,
            order_list_id=order_list_id,
            linked_order_ids=[sl_id],
            parent_order_id=entry_id,
        )

        self.cache.add_order(entry_order)
        self.cache.add_order(sl_order)
        self.cache.add_order(tp_order)
        entry_order.apply(TestEventStubs.order_submitted(entry_order))
        entry_order.apply(TestEventStubs.order_accepted(entry_order))

        # Act
        fill1 = TestEventStubs.order_filled(
            entry_order,
            instrument=ETHUSDT_PERP_BINANCE,
            trade_id=TradeId("T-001"),
            last_qty=Quantity.from_str("0.010"),
        )
        entry_order.apply(fill1)
        self.manager.handle_order_filled(fill1)

        fill2 = TestEventStubs.order_filled(
            entry_order,
            instrument=ETHUSDT_PERP_BINANCE,
            trade_id=TradeId("T-002"),
            last_qty=Quantity.from_str("0.020"),
        )
        entry_order.apply(fill2)
        self.manager.handle_order_filled(fill2)

        # Assert: 4 modify calls (2 children x 2 fills)
        assert len(self.modify_calls) == 4
        first_fill_qty = Quantity.from_str("0.010")
        second_fill_qty = Quantity.from_str("0.030")
        assert self.modify_calls[0][1] == first_fill_qty
        assert self.modify_calls[1][1] == first_fill_qty
        assert self.modify_calls[2][1] == second_fill_qty
        assert self.modify_calls[3][1] == second_fill_qty
