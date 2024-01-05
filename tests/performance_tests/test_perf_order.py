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

import pytest

from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.common.generators import ClientOrderIdGenerator
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.performance import PerformanceHarness
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


AUDUSD_SIM = TestIdStubs.audusd_id()


class TestOrderPerformance(PerformanceHarness):
    def setup(self):
        # Fixture Setup
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

    @pytest.fixture(autouse=True)
    def setup_benchmark(self, benchmark):
        self.benchmark = benchmark

    def test_order_id_generator(self):
        self.benchmark.pedantic(
            target=self.generator.generate,
            iterations=100_000,
            rounds=1,
        )
        # ~0.0ms / ~2.9μs / 2894ns minimum of 100,000 runs @ 1 iteration each run.

    def test_market_order_creation(self):
        self.benchmark.pedantic(
            target=self.order_factory.market,
            args=(
                AUDUSD_SIM,
                OrderSide.BUY,
                Quantity.from_int(100_000),
            ),
            iterations=10_000,
            rounds=1,
        )
        # ~0.0ms / ~10.7μs / 10682ns minimum of 10,000 runs @ 1 iteration each run.

    def test_limit_order_creation(self):
        self.benchmark.pedantic(
            target=self.order_factory.limit,
            args=(
                AUDUSD_SIM,
                OrderSide.BUY,
                Quantity.from_int(100_000),
                Price.from_str("0.80010"),
            ),
            iterations=10_000,
            rounds=1,
        )
        # ~0.0ms / ~14.5μs / 14469ns minimum of 10,000 runs @ 1 iteration each run.
