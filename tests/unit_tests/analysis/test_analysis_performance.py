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

from datetime import datetime
import unittest

from nautilus_trader.analysis.performance import PerformanceAnalyzer
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.model.currencies import AUD
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.position import Position
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
GBPUSD_SIM = TestInstrumentProvider.default_fx_ccy("GBP/USD")


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
        result = self.analyzer.daily_returns()

        # Assert
        self.assertTrue(result.empty)

    def test_get_realized_pnls_when_no_data_returns_none(self):
        # Arrange
        # Act
        result = self.analyzer.realized_pnls()

        # Assert
        self.assertIsNone(result)

    def test_get_realized_pnls_with_currency_when_no_data_returns_none(self):
        # Arrange
        # Act
        result = self.analyzer.realized_pnls(AUD)

        # Assert
        self.assertIsNone(result)

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
        result = self.analyzer.daily_returns()

        # Assert
        self.assertEqual(10, len(result))
        self.assertEqual(-0.12, sum(result))
        self.assertEqual(-0.20, result.iloc[9])

    def test_get_realized_pnls_when_all_flat_positions_returns_expected_series(self):
        # Arrange
        order1 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        order2 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100000),
        )

        order3 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        order4 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100000),
        )

        fill1 = TestStubs.event_order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-1"),
            strategy_id=StrategyId("S", "001"),
            last_px=Price.from_str("1.00000"),
        )

        fill2 = TestStubs.event_order_filled(
            order2,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-1"),
            strategy_id=StrategyId("S", "001"),
            last_px=Price.from_str("1.00010"),
        )

        fill3 = TestStubs.event_order_filled(
            order3,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-2"),
            strategy_id=StrategyId("S", "001"),
            last_px=Price.from_str("1.00000"),
        )

        fill4 = TestStubs.event_order_filled(
            order4,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-2"),
            strategy_id=StrategyId("S", "001"),
            last_px=Price.from_str("1.00020"),
        )

        position1 = Position(instrument=AUDUSD_SIM, fill=fill1)
        position1.apply(fill2)

        position2 = Position(instrument=AUDUSD_SIM, fill=fill3)
        position2.apply(fill4)

        self.analyzer.add_positions([position1, position2])

        # Act
        result = self.analyzer.realized_pnls(USD)

        # Assert
        self.assertEqual(2, len(result))
        self.assertEqual(6.0, result["P-1"])
        self.assertEqual(16.0, result["P-2"])
