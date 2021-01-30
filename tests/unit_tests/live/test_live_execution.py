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

import asyncio
import unittest

from nautilus_trader.analysis.performance import PerformanceAnalyzer
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.enums import ComponentState
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.common.logging import TestLogger
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.data.cache import DataCache
from nautilus_trader.execution.database import BypassExecutionDatabase
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.live.execution_engine import LiveExecutionEngine
from nautilus_trader.model.commands import SubmitOrder
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Quantity
from nautilus_trader.trading.portfolio import Portfolio
from nautilus_trader.trading.strategy import TradingStrategy
from tests.test_kit.mocks import MockExecutionClient
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs


SIM = Venue("SIM")
AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy(TestStubs.symbol_audusd())
GBPUSD_SIM = TestInstrumentProvider.default_fx_ccy(TestStubs.symbol_gbpusd())


class ExecutionEngineTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.clock = LiveClock()
        self.uuid_factory = UUIDFactory()
        self.logger = TestLogger(self.clock)

        self.trader_id = TraderId("TESTER", "000")
        self.account_id = TestStubs.account_id()

        self.order_factory = OrderFactory(
            trader_id=self.trader_id,
            strategy_id=StrategyId("S", "001"),
            clock=self.clock,
        )

        self.random_order_factory = OrderFactory(
            trader_id=TraderId("RANDOM", "042"),
            strategy_id=StrategyId("S", "042"),
            clock=self.clock,
        )

        self.portfolio = Portfolio(
            clock=self.clock,
            logger=self.logger,
        )
        self.portfolio.register_cache(DataCache(self.logger))

        self.analyzer = PerformanceAnalyzer()

        # Fresh isolated loop testing pattern
        self.loop = asyncio.new_event_loop()
        asyncio.set_event_loop(self.loop)

        self.database = BypassExecutionDatabase(trader_id=self.trader_id, logger=self.logger)
        self.exec_engine = LiveExecutionEngine(
            loop=self.loop,
            database=self.database,
            portfolio=self.portfolio,
            clock=self.clock,
            logger=self.logger,
        )

        self.venue = Venue("SIM")
        self.exec_client = MockExecutionClient(
            self.venue,
            self.account_id,
            self.exec_engine,
            self.clock,
            self.logger,
        )

        self.exec_engine.register_client(self.exec_client)

    def tearDown(self):
        self.exec_engine.dispose()
        self.loop.stop()
        self.loop.close()

    def test_start_when_loop_not_running_logs(self):
        # Arrange
        # Act
        self.exec_engine.start()

        # Assert
        self.assertTrue(True)  # No exceptions raised
        self.exec_engine.stop()

    def test_message_qsize_at_max_blocks_on_put_command(self):
        # Arrange
        self.exec_engine = LiveExecutionEngine(
            loop=self.loop,
            database=self.database,
            portfolio=self.portfolio,
            clock=self.clock,
            logger=self.logger,
            config={"qsize": 1}
        )

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        order = strategy.order_factory.market(
            AUDUSD_SIM.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

        submit_order = SubmitOrder(
            Venue("SIM"),
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
        self.exec_engine.execute(submit_order)

        # Assert
        self.assertEqual(1, self.exec_engine.qsize())
        self.assertEqual(0, self.exec_engine.command_count)

    def test_message_qsize_at_max_blocks_on_put_event(self):
        # Arrange
        self.exec_engine = LiveExecutionEngine(
            loop=self.loop,
            database=self.database,
            portfolio=self.portfolio,
            clock=self.clock,
            logger=self.logger,
            config={"qsize": 1}
        )

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        order = strategy.order_factory.market(
            AUDUSD_SIM.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

        submit_order = SubmitOrder(
            Venue("SIM"),
            self.trader_id,
            self.account_id,
            strategy.id,
            PositionId.null(),
            order,
            self.uuid_factory.generate(),
            self.clock.utc_now(),
        )

        event = TestStubs.event_order_submitted(order)

        # Act
        self.exec_engine.execute(submit_order)
        self.exec_engine.process(event)  # Add over max size

        # Assert
        self.assertEqual(1, self.exec_engine.qsize())
        self.assertEqual(0, self.exec_engine.command_count)

    def test_get_event_loop_returns_expected_loop(self):
        # Arrange
        # Act
        loop = self.exec_engine.get_event_loop()

        # Assert
        self.assertEqual(self.loop, loop)

    def test_start(self):
        async def run_test():
            # Arrange
            # Act
            self.exec_engine.start()
            await asyncio.sleep(0.1)

            # Assert
            self.assertEqual(ComponentState.RUNNING, self.exec_engine.state)

            # Tear Down
            self.exec_engine.stop()

        self.loop.run_until_complete(run_test())

    def test_kill_when_running_and_no_messages_on_queues(self):
        async def run_test():
            # Arrange
            # Act
            self.exec_engine.start()
            self.exec_engine.kill()

            # Assert
            self.assertEqual(ComponentState.STOPPED, self.exec_engine.state)

        self.loop.run_until_complete(run_test())

    def test_kill_when_not_running_with_messages_on_queue(self):
        async def run_test():
            # Arrange
            # Act
            self.exec_engine.kill()

            # Assert
            self.assertEqual(0, self.exec_engine.qsize())

        self.loop.run_until_complete(run_test())

    def test_execute_command_places_command_on_queue(self):
        async def run_test():
            # Arrange
            self.exec_engine.start()

            strategy = TradingStrategy(order_id_tag="001")
            strategy.register_trader(
                TraderId("TESTER", "000"),
                self.clock,
                self.logger,
            )

            self.exec_engine.register_strategy(strategy)

            order = strategy.order_factory.market(
                AUDUSD_SIM.symbol,
                OrderSide.BUY,
                Quantity(100000),
            )

            submit_order = SubmitOrder(
                Venue("SIM"),
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
            await asyncio.sleep(0.1)

            # Assert
            self.assertEqual(0, self.exec_engine.qsize())
            self.assertEqual(1, self.exec_engine.command_count)

            # Tear Down
            self.exec_engine.stop()

        self.loop.run_until_complete(run_test())

    def test_handle_position_opening_with_position_id_none(self):
        async def run_test():
            # Arrange
            self.exec_engine.start()

            strategy = TradingStrategy(order_id_tag="001")
            strategy.register_trader(
                TraderId("TESTER", "000"),
                self.clock,
                self.logger,
            )

            self.exec_engine.register_strategy(strategy)

            order = strategy.order_factory.market(
                AUDUSD_SIM.symbol,
                OrderSide.BUY,
                Quantity(100000),
            )

            event = TestStubs.event_order_submitted(order)

            # Act
            self.exec_engine.process(event)
            await asyncio.sleep(0.1)

            # Assert
            self.assertEqual(0, self.exec_engine.qsize())
            self.assertEqual(1, self.exec_engine.event_count)

            # Tear Down
            self.exec_engine.stop()

        self.loop.run_until_complete(run_test())

    # TODO: WIP
    # def test_resolve_state_with_multiple_active_orders_resolved_correctly1(self):
    #     async def run_test():
    #         # Arrange
    #         self.exec_engine.start()
    #
    #         strategy = TradingStrategy(order_id_tag="001")
    #         strategy.register_trader(
    #             TraderId("TESTER", "000"),
    #             self.clock,
    #             self.logger,
    #         )
    #
    #         self.exec_engine.register_strategy(strategy)
    #
    #         order1 = strategy.order_factory.market(
    #             AUDUSD_SIM.symbol,
    #             OrderSide.BUY,
    #             Quantity(100000),
    #         )
    #
    #         order2 = strategy.order_factory.market(
    #             AUDUSD_SIM.symbol,
    #             OrderSide.BUY,
    #             Quantity(100000),
    #         )
    #
    #         random = self.random_order_factory.market(
    #             GBPUSD_SIM.symbol,
    #             OrderSide.BUY,
    #             Quantity(100000),
    #         )
    #
    #         self.exec_engine.cache.add_order(random, PositionId.null())
    #
    #         submit_order1 = SubmitOrder(
    #             self.venue,
    #             self.trader_id,
    #             self.account_id,
    #             strategy.id,
    #             PositionId.null(),
    #             order1,
    #             self.uuid_factory.generate(),
    #             self.clock.utc_now(),
    #         )
    #
    #         submit_order2 = SubmitOrder(
    #             self.venue,
    #             self.trader_id,
    #             self.account_id,
    #             strategy.id,
    #             PositionId.null(),
    #             order2,
    #             self.uuid_factory.generate(),
    #             self.clock.utc_now(),
    #         )
    #
    #         self.exec_engine.execute(submit_order1)
    #         self.exec_engine.execute(submit_order2)
    #         self.exec_engine.process(TestStubs.event_order_submitted(order1))
    #         self.exec_engine.process(TestStubs.event_order_submitted(order2))
    #
    #         # Act
    #         await self.exec_engine.resolve_state()
    #
    #     self.loop.run_until_complete(run_test())


class LiveExecutionClientTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        # Fresh isolated loop testing pattern
        self.loop = asyncio.new_event_loop()
        asyncio.set_event_loop(self.loop)

        self.clock = LiveClock()
        self.uuid_factory = UUIDFactory()
        self.logger = TestLogger(self.clock)

        self.trader_id = TraderId("TESTER", "000")
        self.account_id = TestStubs.account_id()

        self.order_factory = OrderFactory(
            trader_id=self.trader_id,
            strategy_id=StrategyId("S", "001"),
            clock=self.clock,
        )

        self.portfolio = Portfolio(
            clock=self.clock,
            logger=self.logger,
        )
        self.portfolio.register_cache(DataCache(self.logger))

        self.analyzer = PerformanceAnalyzer()

        # Fresh isolated loop testing pattern
        self.loop = asyncio.new_event_loop()
        asyncio.set_event_loop(self.loop)

        database = BypassExecutionDatabase(trader_id=self.trader_id, logger=self.logger)
        self.engine = LiveExecutionEngine(
            loop=self.loop,
            database=database,
            portfolio=self.portfolio,
            clock=self.clock,
            logger=self.logger,
        )

        self.client = LiveExecutionClient(
            venue=SIM,
            account_id=self.account_id,
            engine=self.engine,
            clock=self.clock,
            logger=self.logger,
        )

    def test_state_report_when_not_implemented_raises_exception(self):
        async def run_test():
            # Arrange
            # Act
            # Assert
            try:
                await self.client.state_report([])
            except NotImplementedError as ex:
                self.assertEqual(NotImplementedError, type(ex))

        self.loop.run_until_complete(run_test())
