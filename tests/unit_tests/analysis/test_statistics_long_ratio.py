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

from nautilus_trader.analysis import LongRatio
from nautilus_trader.common.component import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.position import Position
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.events import TestEventStubs


ETHUSDT_PERP_BINANCE = TestInstrumentProvider.ethusdt_perp_binance()


class TestLongRatioPortfolioStatistics:
    def setup(self):
        # Fixture Setup
        self.order_factory = OrderFactory(
            trader_id=TraderId("TESTER-000"),
            strategy_id=StrategyId("S-001"),
            clock=TestClock(),
        )

    def test_name_returns_expected_returns_expected(self):
        # Arrange
        stat = LongRatio()

        # Act
        result = stat.name

        # Assert
        assert result == "Long Ratio"

    def test_calculate_given_empty_list_returns_none(self):
        # Arrange
        stat = LongRatio()

        # Act
        result = stat.calculate_from_positions([])

        # Assert
        assert result is None

    def test_calculate_given_two_long_returns_expected(self):
        # Arrange
        stat = LongRatio()

        order1 = self.order_factory.market(
            ETHUSDT_PERP_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_int(1),
        )

        order2 = self.order_factory.market(
            ETHUSDT_PERP_BINANCE.id,
            OrderSide.SELL,
            Quantity.from_int(1),
        )

        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=ETHUSDT_PERP_BINANCE,
            position_id=PositionId("P-1"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_int(10_000),
        )

        fill2 = TestEventStubs.order_filled(
            order2,
            instrument=ETHUSDT_PERP_BINANCE,
            position_id=PositionId("P-2"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_int(10_000),
        )

        position1 = Position(instrument=ETHUSDT_PERP_BINANCE, fill=fill1)
        position1.apply(fill2)

        position2 = Position(instrument=ETHUSDT_PERP_BINANCE, fill=fill1)
        position2.apply(fill2)

        data = [position1, position2]

        # Act
        result = stat.calculate_from_positions(data)

        # Assert
        assert result == 1.0

    def test_calculate_given_one_long_one_short_returns_expected(self):
        # Arrange
        stat = LongRatio()

        order1 = self.order_factory.market(
            ETHUSDT_PERP_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_int(1),
        )

        order2 = self.order_factory.market(
            ETHUSDT_PERP_BINANCE.id,
            OrderSide.SELL,
            Quantity.from_int(1),
        )

        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=ETHUSDT_PERP_BINANCE,
            position_id=PositionId("P-1"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_int(10_000),
        )

        fill2 = TestEventStubs.order_filled(
            order2,
            instrument=ETHUSDT_PERP_BINANCE,
            position_id=PositionId("P-2"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_int(10_000),
        )

        position1 = Position(instrument=ETHUSDT_PERP_BINANCE, fill=fill1)
        position1.apply(fill2)

        position2 = Position(instrument=ETHUSDT_PERP_BINANCE, fill=fill2)
        position2.apply(fill1)

        data = [position1, position2]

        # Act
        result = stat.calculate_from_positions(data)

        # Assert
        assert result == 0.5
