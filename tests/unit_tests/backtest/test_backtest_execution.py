# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  you may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

import pandas as pd
import unittest

from nautilus_trader.model.enums import OrderSide, Currency
from nautilus_trader.model.objects import Quantity, Price
from nautilus_trader.model.events import OrderRejected, OrderWorking, OrderModified, OrderFilled
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.common.data import DataClient
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.guid import TestGuidFactory
from nautilus_trader.common.portfolio import Portfolio
from nautilus_trader.common.logging import TestLogger
from nautilus_trader.common.execution import InMemoryExecutionDatabase, ExecutionEngine
from nautilus_trader.trading.strategy import TradingStrategy
from nautilus_trader.analysis.performance import PerformanceAnalyzer
from nautilus_trader.backtest.config import BacktestConfig
from nautilus_trader.backtest.execution import BacktestExecClient
from nautilus_trader.backtest.models import FillModel

from tests.test_kit.strategies import TestStrategy1
from tests.test_kit.data import TestDataProvider
from tests.test_kit.stubs import TestStubs

USDJPY_FXCM = TestStubs.symbol_usdjpy_fxcm()


class BacktestExecClientTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.usdjpy = TestStubs.instrument_usdjpy()
        self.bid_data_1min = TestDataProvider.usdjpy_1min_bid()[:2000]
        self.ask_data_1min = TestDataProvider.usdjpy_1min_ask()[:2000]

        self.data_ticks = {self.usdjpy.symbol: pd.DataFrame()}
        self.data_bars_bid = {self.usdjpy.symbol: self.bid_data_1min}
        self.data_bars_ask = {self.usdjpy.symbol: self.ask_data_1min}

        self.strategies = [TestStrategy1(TestStubs.bartype_usdjpy_1min_bid())]

        self.clock = TestClock()
        self.guid_factory = TestGuidFactory()
        self.logger = TestLogger()

        self.data_client = DataClient(
            tick_capacity=100,
            clock=self.clock,
            guid_factory=self.guid_factory,
            logger=self.logger)

        self.portfolio = Portfolio(
            currency=Currency.USD,
            clock=self.clock,
            guid_factory=self.guid_factory,
            logger=self.logger)

        self.analyzer = PerformanceAnalyzer()

        trader_id = TraderId('TESTER', '000')
        account_id = TestStubs.account_id()

        self.exec_db = InMemoryExecutionDatabase(
            trader_id=trader_id,
            logger=self.logger)
        self.exec_engine = ExecutionEngine(
            trader_id=trader_id,
            account_id=account_id,
            database=self.exec_db,
            portfolio=self.portfolio,
            clock=self.clock,
            guid_factory=self.guid_factory,
            logger=self.logger)

        self.exec_client = BacktestExecClient(
            exec_engine=self.exec_engine,
            instruments={self.usdjpy.symbol: self.usdjpy},
            config=BacktestConfig(),
            fill_model=FillModel(),
            clock=TestClock(),
            guid_factory=TestGuidFactory(),
            logger=TestLogger())
        self.exec_engine.register_client(self.exec_client)

    def test_can_account_collateral_inquiry(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag='001')
        self.exec_engine.register_strategy(strategy)

        # Act
        strategy.account_inquiry()

        # Assert
        self.assertEqual(1, len(strategy.account().get_events()))

    def test_can_submit_market_order(self):
        # Arrange
        strategy = TestStrategy1(bar_type=TestStubs.bartype_usdjpy_1min_bid())
        self.data_client.register_strategy(strategy)
        self.exec_engine.register_strategy(strategy)
        strategy.start()

        self.exec_client.process_tick(TestStubs.tick_3decimal(self.usdjpy.symbol))  # Prepare market
        order = strategy.order_factory.market(
            USDJPY_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        # Act
        strategy.submit_order(order, strategy.position_id_generator.generate())

        # Assert
        self.assertEqual(5, strategy.object_storer.count)
        self.assertTrue(isinstance(strategy.object_storer.get_store()[3], OrderFilled))
        self.assertEqual(Price(90.003, 3), strategy.order(order.id).average_price)

    def test_can_submit_limit_order(self):
        # Arrange
        strategy = TestStrategy1(bar_type=TestStubs.bartype_usdjpy_1min_bid())
        self.data_client.register_strategy(strategy)
        self.exec_engine.register_strategy(strategy)
        strategy.start()

        self.exec_client.process_tick(TestStubs.tick_3decimal(self.usdjpy.symbol))  # Prepare market
        order = strategy.order_factory.limit(
            USDJPY_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price(80.000, 3))

        # Act
        strategy.submit_order(order, strategy.position_id_generator.generate())

        # Assert
        self.assertEqual(4, strategy.object_storer.count)
        self.assertTrue(isinstance(strategy.object_storer.get_store()[3], OrderWorking))
        self.assertEqual(Price(80.000, 3), order.price)

    def test_can_submit_atomic_market_order(self):
        # Arrange
        strategy = TestStrategy1(bar_type=TestStubs.bartype_usdjpy_1min_bid())
        self.data_client.register_strategy(strategy)
        self.exec_engine.register_strategy(strategy)
        strategy.start()

        self.exec_client.process_tick(TestStubs.tick_3decimal(self.usdjpy.symbol))  # Prepare market
        atomic_order = strategy.order_factory.atomic_market(
            USDJPY_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price(80.000, 3))

        # Act
        strategy.submit_atomic_order(atomic_order, strategy.position_id_generator.generate())

        # Assert
        self.assertEqual(7, strategy.object_storer.count)
        self.assertTrue(isinstance(strategy.object_storer.get_store()[3], OrderFilled))
        self.assertEqual(Price(80.000, 3), atomic_order.stop_loss.price)

    def test_can_submit_atomic_stop_order(self):
        # Arrange
        strategy = TestStrategy1(bar_type=TestStubs.bartype_usdjpy_1min_bid())
        self.data_client.register_strategy(strategy)
        self.exec_engine.register_strategy(strategy)
        strategy.start()

        self.exec_client.process_tick(TestStubs.tick_3decimal(self.usdjpy.symbol))  # Prepare market
        atomic_order = strategy.order_factory.atomic_stop_market(
            USDJPY_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price(97.000, 3),
            Price(96.710, 3),
            Price(86.000, 3))

        # Act
        strategy.submit_atomic_order(atomic_order, strategy.position_id_generator.generate())

        # Assert
        self.assertEqual(4, strategy.object_storer.count)
        self.assertTrue(isinstance(strategy.object_storer.get_store()[3], OrderWorking))

    def test_can_modify_stop_order(self):
        # Arrange
        strategy = TestStrategy1(bar_type=TestStubs.bartype_usdjpy_1min_bid())
        self.data_client.register_strategy(strategy)
        self.exec_engine.register_strategy(strategy)
        strategy.start()

        self.exec_client.process_tick(TestStubs.tick_3decimal(self.usdjpy.symbol))  # Prepare market
        order = strategy.order_factory.stop(
            USDJPY_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price(96.711, 3))

        strategy.submit_order(order, strategy.position_id_generator.generate())

        # Act
        strategy.modify_order(order, order.quantity, Price(96.714, 3))

        # Assert
        self.assertEqual(Price(96.714, 3), strategy.order(order.id).price)
        self.assertEqual(5, strategy.object_storer.count)
        self.assertTrue(isinstance(strategy.object_storer.get_store()[4], OrderModified))

    def test_can_modify_atomic_order_working_stop_loss(self):
        # Arrange
        strategy = TestStrategy1(bar_type=TestStubs.bartype_usdjpy_1min_bid())
        self.data_client.register_strategy(strategy)
        self.exec_engine.register_strategy(strategy)
        strategy.start()

        self.exec_client.process_tick(TestStubs.tick_3decimal(self.usdjpy.symbol))  # Prepare market
        atomic_order = strategy.order_factory.atomic_market(
            USDJPY_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price(85.000, 3))

        strategy.submit_atomic_order(atomic_order, strategy.position_id_generator.generate())

        # Act
        strategy.modify_order(atomic_order.stop_loss, atomic_order.entry.quantity, Price(85.100, 3))

        # Assert
        self.assertEqual(Price(85.100, 3), strategy.order(atomic_order.stop_loss.id).price)
        self.assertEqual(8, strategy.object_storer.count)
        self.assertTrue(isinstance(strategy.object_storer.get_store()[7], OrderModified))

    # TODO: Fix failing test - market not updating inside BacktestExecution Client
    # def test_submit_market_order_with_slippage_fill_model_slips_order(self):
    #     # Arrange
    #     fill_model = FillModel(
    #         prob_fill_at_limit=0.0,
    #         prob_fill_at_stop=1.0,
    #         prob_slippage=1.0,
    #         random_seed=None)
    #
    #     exec_client = BacktestExecClient(
    #         exec_engine=self.exec_engine,
    #         instruments={self.usdjpy.symbol: self.usdjpy},
    #         config=BacktestConfig(),
    #         fill_model=fill_model,
    #         clock=TestClock(),
    #         guid_factory=TestGuidFactory(),
    #         logger=TestLogger())
    #
    #     self.exec_engine.register_client(exec_client)
    #     strategy = TestStrategy1(bar_type=TestStubs.bartype_usdjpy_1min_bid())
    #     self.data_client.register_strategy(strategy)
    #     self.exec_engine.register_strategy(strategy)
    #     strategy.start()
    #
    #     self.exec_client.process_tick(TestStubs.tick_3decimal(self.usdjpy.symbol))  # Prepare market
    #     order = strategy.order_factory.market(
    #         USDJPY_FXCM,
    #         OrderSide.BUY,
    #         Quantity(100000))
    #
    #     # Act
    #     strategy.submit_order(order, strategy.position_id_generator.generate())
    #
    #     # Assert
    #     self.assertEqual(5, strategy.object_storer.count)
    #     self.assertTrue(isinstance(strategy.object_storer.get_store()[3], OrderFilled))
    #     self.assertEqual(Price(90.004, 3), strategy.order(order.id).average_price)

    def test_submit_order_with_no_market_rejects_order(self):
        # Arrange
        strategy = TestStrategy1(bar_type=TestStubs.bartype_usdjpy_1min_bid())
        self.data_client.register_strategy(strategy)
        self.exec_engine.register_strategy(strategy)
        strategy.start()

        order = strategy.order_factory.stop(
            USDJPY_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price(80.000, 3))

        # Act
        strategy.submit_order(order, strategy.position_id_generator.generate())

        # Assert
        self.assertEqual(3, strategy.object_storer.count)
        self.assertTrue(isinstance(strategy.object_storer.get_store()[2], OrderRejected))

    def test_submit_order_with_invalid_price_gets_rejected(self):
        # Arrange
        strategy = TestStrategy1(bar_type=TestStubs.bartype_usdjpy_1min_bid())
        self.data_client.register_strategy(strategy)
        self.exec_engine.register_strategy(strategy)
        strategy.start()

        self.exec_client.process_tick(TestStubs.tick_3decimal(self.usdjpy.symbol))  # Prepare market
        order = strategy.order_factory.stop(
            USDJPY_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price(80.000, 3))

        # Act
        strategy.submit_order(order, strategy.position_id_generator.generate())

        # Assert
        self.assertEqual(3, strategy.object_storer.count)
        self.assertTrue(isinstance(strategy.object_storer.get_store()[2], OrderRejected))
