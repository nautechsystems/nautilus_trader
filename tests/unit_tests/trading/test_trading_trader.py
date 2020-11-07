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
from nautilus_trader.backtest.config import BacktestConfig
from nautilus_trader.backtest.data import BacktestDataClient
from nautilus_trader.backtest.data import BacktestDataContainer
from nautilus_trader.backtest.exchange import SimulatedExchange
from nautilus_trader.backtest.execution import BacktestExecClient
from nautilus_trader.backtest.loaders import InstrumentLoader
from nautilus_trader.backtest.logging import TestLogger
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.enums import ComponentState
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.execution.database import BypassExecutionDatabase
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import OMSType
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import IdTag
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.trading.portfolio import Portfolio
from nautilus_trader.trading.trader import Trader
from tests.test_kit.data import TestDataProvider
from tests.test_kit.strategies import EmptyStrategy
from tests.test_kit.stubs import TestStubs


USDJPY_FXCM = TestStubs.symbol_usdjpy_fxcm()


class TraderTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        usdjpy = InstrumentLoader.default_fx_ccy(TestStubs.symbol_usdjpy_fxcm())
        data = BacktestDataContainer()
        data.add_instrument(usdjpy)
        data.add_bars(usdjpy.symbol, BarAggregation.MINUTE, PriceType.BID, TestDataProvider.usdjpy_1min_bid()[:2000])
        data.add_bars(usdjpy.symbol, BarAggregation.MINUTE, PriceType.ASK, TestDataProvider.usdjpy_1min_ask()[:2000])

        clock = TestClock()
        uuid_factory = UUIDFactory()
        logger = TestLogger(clock)
        trader_id = TraderId("TESTER", "000")
        account_id = TestStubs.account_id()

        self.portfolio = Portfolio(
            clock=clock,
            uuid_factory=uuid_factory,
            logger=logger,
        )

        self.data_engine = DataEngine(
            portfolio=self.portfolio,
            clock=clock,
            uuid_factory=uuid_factory,
            logger=logger,
            config={'use_previous_close': False},
        )

        self.analyzer = PerformanceAnalyzer()

        self.exec_db = BypassExecutionDatabase(
            trader_id=trader_id,
            logger=logger,
        )

        self.exec_engine = ExecutionEngine(
            database=self.exec_db,
            portfolio=self.portfolio,
            clock=clock,
            uuid_factory=uuid_factory,
            logger=logger,
        )

        self.market = SimulatedExchange(
            venue=Venue("FXCM"),
            oms_type=OMSType.HEDGING,
            generate_position_ids=True,
            exec_cache=self.exec_engine.cache,
            instruments={usdjpy.symbol: usdjpy},
            config=BacktestConfig(),
            fill_model=FillModel(),
            clock=clock,
            uuid_factory=UUIDFactory(),
            logger=logger,
        )

        self.data_client = BacktestDataClient(
            data=data,
            venue=Venue("FXCM"),
            engine=self.data_engine,
            clock=clock,
            uuid_factory=uuid_factory,
            logger=logger,
        )

        self.data_engine.register_client(self.data_client)

        self.exec_client = BacktestExecClient(
            market=self.market,
            account_id=account_id,
            engine=self.exec_engine,
            logger=logger,
        )

        self.exec_engine.register_client(self.exec_client)

        strategies = [
            EmptyStrategy("001"),
            EmptyStrategy("002"),
        ]

        self.trader = Trader(
            trader_id=trader_id,
            strategies=strategies,
            data_engine=self.data_engine,
            exec_engine=self.exec_engine,
            clock=clock,
            uuid_factory=uuid_factory,
            logger=logger,
        )

    def test_initialize_trader(self):
        # Arrange
        # Act
        trader_id = self.trader.id

        # Assert
        self.assertEqual(TraderId("TESTER", "000"), trader_id)
        self.assertEqual(IdTag("000"), trader_id.tag)
        self.assertEqual(ComponentState.INITIALIZED, self.trader.state)
        self.assertEqual(2, len(self.trader.strategy_states()))

    def test_get_strategy_states(self):
        # Arrange
        # Act
        status = self.trader.strategy_states()

        # Assert
        self.assertTrue(StrategyId("EmptyStrategy", "001") in status)
        self.assertTrue(StrategyId("EmptyStrategy", "002") in status)
        self.assertEqual('INITIALIZED', status[StrategyId("EmptyStrategy", "001")])
        self.assertEqual('INITIALIZED', status[StrategyId("EmptyStrategy", "002")])
        self.assertEqual(2, len(status))

    def test_change_strategies(self):
        # Arrange
        strategies = [
            EmptyStrategy("003"),
            EmptyStrategy("004"),
        ]

        # Act
        self.trader.initialize_strategies(strategies)

        # Assert
        self.assertTrue(strategies[0].id in self.trader.strategy_states())
        self.assertTrue(strategies[1].id in self.trader.strategy_states())
        self.assertEqual(2, len(self.trader.strategy_states()))

    def test_trader_detects_duplicate_identifiers(self):
        # Arrange
        strategies = [
            EmptyStrategy("000"),
            EmptyStrategy("000"),
        ]

        # Act
        self.assertRaises(ValueError, self.trader.initialize_strategies, strategies)

    def test_start_a_trader(self):
        # Arrange
        # Act
        self.trader.start()

        strategy_states = self.trader.strategy_states()

        # Assert
        self.assertEqual(ComponentState.RUNNING, self.trader.state)
        self.assertEqual('RUNNING', strategy_states[StrategyId("EmptyStrategy", "001")])
        self.assertEqual('RUNNING', strategy_states[StrategyId("EmptyStrategy", "002")])

    def test_stop_a_running_trader(self):
        # Arrange
        self.trader.start()

        # Act
        self.trader.stop()

        strategy_states = self.trader.strategy_states()

        # Assert
        self.assertEqual(ComponentState.STOPPED, self.trader.state)
        self.assertEqual('STOPPED', strategy_states[StrategyId("EmptyStrategy", "001")])
        self.assertEqual('STOPPED', strategy_states[StrategyId("EmptyStrategy", "002")])
