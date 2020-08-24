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

import unittest

from nautilus_trader.analysis.performance import PerformanceAnalyzer
from nautilus_trader.backtest.clock import TestClock
from nautilus_trader.backtest.config import BacktestConfig
from nautilus_trader.backtest.data import BacktestDataClient
from nautilus_trader.backtest.data import BacktestDataContainer
from nautilus_trader.backtest.execution import BacktestExecClient
from nautilus_trader.backtest.logging import TestLogger
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.backtest.uuid import TestUUIDFactory
from nautilus_trader.common.execution import ExecutionEngine
from nautilus_trader.common.execution import InMemoryExecutionDatabase
from nautilus_trader.common.portfolio import Portfolio
from nautilus_trader.model.enums import BarStructure
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import IdTag
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.trading.trader import Trader
from tests.test_kit.data import TestDataProvider
from tests.test_kit.strategies import EmptyStrategy
from tests.test_kit.stubs import TestStubs

USDJPY_FXCM = TestStubs.instrument_usdjpy().symbol


class TraderTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        usdjpy = TestStubs.instrument_usdjpy()
        data = BacktestDataContainer()
        data.add_instrument(usdjpy)
        data.add_bars(usdjpy.symbol, BarStructure.MINUTE, PriceType.BID, TestDataProvider.usdjpy_1min_bid()[:2000])
        data.add_bars(usdjpy.symbol, BarStructure.MINUTE, PriceType.ASK, TestDataProvider.usdjpy_1min_ask()[:2000])

        clock = TestClock()
        uuid_factory = TestUUIDFactory()
        logger = TestLogger()
        trader_id = TraderId("TESTER", "000")
        account_id = TestStubs.account_id()

        data_client = BacktestDataClient(
            data=data,
            tick_capacity=1000,
            bar_capacity=1000,
            clock=clock,
            logger=logger)

        self.portfolio = Portfolio(
            clock=clock,
            uuid_factory=uuid_factory,
            logger=logger)

        self.analyzer = PerformanceAnalyzer()

        self.exec_db = InMemoryExecutionDatabase(
            trader_id=trader_id,
            logger=logger)
        self.exec_engine = ExecutionEngine(
            trader_id=trader_id,
            account_id=account_id,
            database=self.exec_db,
            portfolio=self.portfolio,
            clock=clock,
            uuid_factory=uuid_factory,
            logger=logger)

        self.exec_client = BacktestExecClient(
            exec_engine=self.exec_engine,
            instruments={usdjpy.symbol: usdjpy},
            config=BacktestConfig(),
            fill_model=FillModel(),
            clock=clock,
            uuid_factory=uuid_factory,
            logger=logger)
        self.exec_engine.register_client(self.exec_client)

        strategies = [EmptyStrategy("001"),
                      EmptyStrategy("002")]

        self.trader = Trader(
            trader_id=trader_id,
            account_id=account_id,
            strategies=strategies,
            data_client=data_client,
            exec_engine=self.exec_engine,
            clock=clock,
            uuid_factory=uuid_factory,
            logger=logger)

    def test_can_initialize_trader(self):
        # Arrange
        # Act
        trader_id = self.trader.id

        # Assert
        self.assertEqual(TraderId("TESTER", "000"), trader_id)
        self.assertEqual(IdTag("000"), trader_id.order_id_tag)
        self.assertFalse(self.trader.is_running)
        self.assertEqual(2, len(self.trader.strategy_status()))

    def test_can_get_strategy_status(self):
        # Arrange
        # Act
        status = self.trader.strategy_status()

        # Assert
        self.assertTrue(StrategyId("EmptyStrategy", "001") in status)
        self.assertTrue(StrategyId("EmptyStrategy", "002") in status)
        self.assertFalse(status[StrategyId("EmptyStrategy", "001")])
        self.assertFalse(status[StrategyId("EmptyStrategy", "002")])
        self.assertEqual(2, len(status))

    def test_can_change_strategies(self):
        # Arrange
        strategies = [EmptyStrategy("003"),
                      EmptyStrategy("004")]

        # Act
        self.trader.initialize_strategies(strategies)

        # Assert
        self.assertTrue(strategies[0].id in self.trader.strategy_status())
        self.assertTrue(strategies[1].id in self.trader.strategy_status())
        self.assertEqual(2, len(self.trader.strategy_status()))

    def test_trader_detects_none_unique_identifiers(self):
        # Arrange
        strategies = [EmptyStrategy("000"),
                      EmptyStrategy("000")]

        # Act
        self.assertRaises(ValueError, self.trader.initialize_strategies, strategies)

    def test_can_start_a_trader(self):
        # Arrange
        # Act
        self.trader.start()

        # Assert
        self.assertTrue(self.trader.is_running)
        self.assertTrue(StrategyId("EmptyStrategy", "001") in self.trader.strategy_status())
        self.assertTrue(StrategyId("EmptyStrategy", "002") in self.trader.strategy_status())
        self.assertTrue(self.trader.strategy_status()[StrategyId("EmptyStrategy", "001")])
        self.assertTrue(self.trader.strategy_status()[StrategyId("EmptyStrategy", "002")])

    def test_can_stop_a_running_trader(self):
        # Arrange
        self.trader.start()

        # Act
        self.trader.stop()

        # Assert
        self.assertFalse(self.trader.is_running)
        self.assertTrue(StrategyId("EmptyStrategy", "001") in self.trader.strategy_status())
        self.assertTrue(StrategyId("EmptyStrategy", "002") in self.trader.strategy_status())
        self.assertFalse(self.trader.strategy_status()[StrategyId("EmptyStrategy", "001")])
        self.assertFalse(self.trader.strategy_status()[StrategyId("EmptyStrategy", "002")])
