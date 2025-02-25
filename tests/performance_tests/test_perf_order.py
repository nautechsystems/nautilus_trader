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

from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.common.generators import ClientOrderIdGenerator
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


class TestOrderPerformance:
    def setup(self):
        self.generator = ClientOrderIdGenerator(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("S-001"),
            clock=LiveClock(),
        )

        self.order_factory = OrderFactory(
            trader_id=TraderId("TESTER-000"),
            strategy_id=StrategyId("S-001"),
            clock=TestClock(),
        )

    def test_order_id_generator(self, benchmark):
        benchmark(self.generator.generate)

    def test_market_order_creation(self, benchmark):
        benchmark(
            self.order_factory.market,
            TestIdStubs.audusd_id(),
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

    def test_limit_order_creation(self, benchmark):
        benchmark(
            self.order_factory.limit,
            TestIdStubs.audusd_id(),
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("0.80010"),
        )

    def test_to_own_book_order(self, benchmark):
        order = self.order_factory.limit(
            TestIdStubs.audusd_id(),
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("0.80010"),
        )
        benchmark(order.to_own_book_order)
