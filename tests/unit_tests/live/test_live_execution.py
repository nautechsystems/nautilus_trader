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

import time
import unittest

from nautilus_trader.analysis.performance import PerformanceAnalyzer
from nautilus_trader.backtest.loaders import InstrumentLoader
from nautilus_trader.backtest.logging import TestLogger
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.data.cache import DataCache
from nautilus_trader.execution.database import BypassExecutionDatabase
from nautilus_trader.live.execution import LiveExecutionEngine
from nautilus_trader.model.commands import SubmitOrder
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Quantity
from nautilus_trader.trading.portfolio import Portfolio
from nautilus_trader.trading.strategy import TradingStrategy
from tests.test_kit.stubs import TestStubs


AUDUSD_FXCM = InstrumentLoader.default_fx_ccy(TestStubs.symbol_audusd_fxcm())
GBPUSD_FXCM = InstrumentLoader.default_fx_ccy(TestStubs.symbol_gbpusd_fxcm())


class ExecutionEngineTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.clock = TestClock()
        self.uuid_factory = UUIDFactory()
        self.logger = TestLogger(self.clock)

        self.trader_id = TraderId("TESTER", "000")
        self.account_id = TestStubs.account_id()

        self.order_factory = OrderFactory(
            trader_id=self.trader_id,
            strategy_id=StrategyId("S", "001"),
            clock=TestClock(),
        )

        self.portfolio = Portfolio(
            clock=self.clock,
            uuid_factory=self.uuid_factory,
            logger=self.logger,
        )
        self.portfolio.register_cache(DataCache(self.logger))

        self.analyzer = PerformanceAnalyzer()

        database = BypassExecutionDatabase(trader_id=self.trader_id, logger=self.logger)
        self.exec_engine = LiveExecutionEngine(
            database=database,
            portfolio=self.portfolio,
            clock=self.clock,
            uuid_factory=self.uuid_factory,
            logger=self.logger,
        )

        self.cache = self.exec_engine.cache
        self.exec_engine.process(TestStubs.event_account_state())

    def test_start(self):
        # Arrange
        # Act
        self.exec_engine.start()

        # Assert
        # TODO: Implement test

    def test_execute_command_places_command_on_queue(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            self.clock,
            UUIDFactory(),
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        order = strategy.order_factory.market(
            AUDUSD_FXCM.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

        submit_order = SubmitOrder(
            Venue("FXCM"),
            self.trader_id,
            self.account_id,
            strategy.id,
            PositionId.null(),
            order,
            self.uuid_factory.generate(),
            self.clock.utc_now(),
        )

        # Act
        self.exec_engine.execute(submit_order)

        time.sleep(0.1)

        # Assert
        self.assertEqual(2, self.exec_engine.queue_size())

    def test_handle_position_opening_with_position_id_none(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            self.clock,
            UUIDFactory(),
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        order = strategy.order_factory.market(
            AUDUSD_FXCM.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

        event = TestStubs.event_order_submitted(order)

        # Act
        self.exec_engine.process(event)

        time.sleep(0.1)

        # Assert
        self.assertEqual(2, self.exec_engine.queue_size())
