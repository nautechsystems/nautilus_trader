# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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

from datetime import datetime
import unittest

from nautilus_trader.analysis.performance import PerformanceAnalyzer
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.position import Position
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy(TestStubs.symbol_audusd_fxcm())
GBPUSD_SIM = TestInstrumentProvider.default_fx_ccy(TestStubs.symbol_gbpusd_fxcm())


class AnalyzerTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.analyzer = PerformanceAnalyzer()
        self.order_factory = OrderFactory(
            trader_id=TraderId("TESTER", "000"),
            strategy_id=StrategyId("S", "001"),
            clock=TestClock(),
        )

    def test_get_daily_returns_when_no_data_returns_empty_series(self):
        # Arrange
        # Act
        result = self.analyzer.get_daily_returns()

        # Assert
        self.assertTrue(result.empty)

    def test_get_realized_pnls_when_no_data_returns_empty_series(self):
        # Arrange
        # Act
        result = self.analyzer.get_realized_pnls()

        # Assert
        self.assertTrue(result.empty)

    def test_analyzer_tracks_daily_returns(self):
        # Arrange
        t1 = datetime(year=2010, month=1, day=1)
        t2 = datetime(year=2010, month=1, day=2)
        t3 = datetime(year=2010, month=1, day=3)
        t4 = datetime(year=2010, month=1, day=4)
        t5 = datetime(year=2010, month=1, day=5)
        t6 = datetime(year=2010, month=1, day=6)
        t7 = datetime(year=2010, month=1, day=7)
        t8 = datetime(year=2010, month=1, day=8)
        t9 = datetime(year=2010, month=1, day=9)
        t10 = datetime(year=2010, month=1, day=10)

        # Act
        self.analyzer.add_return(t1, 0.05)
        self.analyzer.add_return(t2, -0.10)
        self.analyzer.add_return(t3, 0.10)
        self.analyzer.add_return(t4, -0.21)
        self.analyzer.add_return(t5, 0.22)
        self.analyzer.add_return(t6, -0.23)
        self.analyzer.add_return(t7, 0.24)
        self.analyzer.add_return(t8, -0.25)
        self.analyzer.add_return(t9, 0.26)
        self.analyzer.add_return(t10, -0.10)
        self.analyzer.add_return(t10, -0.10)
        result = self.analyzer.get_daily_returns()

        # Assert
        self.assertEqual(10, len(result))
        self.assertEqual(-0.12, sum(result))
        self.assertEqual(-0.20, result.iloc[9])

    def test_get_realized_pnls_when_all_flat_positions_returns_expected_series(self):
        # Arrange
        order1 = self.order_factory.market(
            AUDUSD_SIM.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

        order2 = self.order_factory.market(
            AUDUSD_SIM.symbol,
            OrderSide.SELL,
            Quantity(100000),
        )

        fill1 = TestStubs.event_order_filled(
            order1,
            instrument=AUDUSD_SIM,
        )

        fill2 = TestStubs.event_order_filled(
            order2,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S", "001"),
            fill_price=Price("1.00000"),
        )

        position1 = Position(fill1)
        position1.apply(fill2)

        position2 = Position(fill1)
        position2.apply(fill2)

        self.analyzer.add_positions([position1, position2])

        # Act

        # Assert
        self.assertTrue(all(self.analyzer.get_realized_pnls()))
