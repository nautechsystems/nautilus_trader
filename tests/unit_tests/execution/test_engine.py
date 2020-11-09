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
from nautilus_trader.backtest.logging import TestLogger
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.data.cache import DataCache
from nautilus_trader.execution.database import BypassExecutionDatabase
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.model.commands import SubmitOrder
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.position import Position
from nautilus_trader.trading.portfolio import Portfolio
from nautilus_trader.trading.strategy import TradingStrategy
from tests.test_kit.mocks import MockExecutionClient
from tests.test_kit.stubs import TestStubs


AUDUSD_FXCM = TestStubs.symbol_audusd_fxcm()
GBPUSD_FXCM = TestStubs.symbol_gbpusd_fxcm()


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
        self.exec_engine = ExecutionEngine(
            database=database,
            portfolio=self.portfolio,
            clock=self.clock,
            uuid_factory=self.uuid_factory,
            logger=self.logger,
        )

        self.cache = self.exec_engine.cache
        self.exec_engine.process(TestStubs.event_account_state())

        self.venue = Venue("FXCM")
        self.exec_client = MockExecutionClient(
            self.venue,
            self.account_id,
            self.exec_engine,
            self.clock,
            self.uuid_factory,
            self.logger,
        )

        self.exec_engine.register_client(self.exec_client)

    def test_register_strategy(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            trader_id=self.trader_id,
            clock=self.clock,
            uuid_factory=UUIDFactory(),
            logger=self.logger,
        )

        # Act
        self.exec_engine.register_strategy(strategy)

        # Assert
        self.assertIn(strategy.id, self.exec_engine.registered_strategies)

    def test_deregister_strategy(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            clock=self.clock,
            uuid_factory=UUIDFactory(),
            logger=self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        # Act
        self.exec_engine.deregister_strategy(strategy)

        # Assert
        self.assertNotIn(strategy.id, self.exec_engine.registered_strategies)

    def test_reset_execution_engine(self):
        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            clock=self.clock,
            uuid_factory=UUIDFactory(),
            logger=self.logger,
        )

        self.exec_engine.register_strategy(strategy)  # Also registers with portfolio

        # Act
        self.exec_engine.reset()

        # Assert
        self.assertIn(strategy.id, self.exec_engine.registered_strategies)

    def test_submit_order(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            clock=self.clock,
            uuid_factory=UUIDFactory(),
            logger=self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        order = strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
        )

        submit_order = SubmitOrder(
            self.venue,
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

        # Assert
        self.assertIn(submit_order, self.exec_client.commands)
        self.assertTrue(self.cache.order_exists(order.cl_ord_id))

    def test_handle_order_fill_event(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            clock=self.clock,
            uuid_factory=UUIDFactory(),
            logger=self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        order = strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
        )

        submit_order = SubmitOrder(
            self.venue,
            self.trader_id,
            self.account_id,
            strategy.id,
            PositionId.null(),
            order,
            self.uuid_factory.generate(),
            self.clock.utc_now(),
        )

        self.exec_engine.execute(submit_order)

        # Act
        self.exec_engine.process(TestStubs.event_order_submitted(order))
        self.exec_engine.process(TestStubs.event_order_accepted(order))
        self.exec_engine.process(TestStubs.event_order_filled(order))

        expected_position_id = PositionId("O-19700101-000000-000-001-1")  # Stubbed from order id?

        # Assert
        self.assertTrue(self.cache.position_exists(expected_position_id))
        self.assertTrue(self.cache.is_position_open(expected_position_id))
        self.assertFalse(self.cache.is_position_closed(expected_position_id))
        self.assertEqual(Position, type(self.cache.position(expected_position_id)))
        self.assertIn(expected_position_id, self.cache.position_ids())
        self.assertNotIn(expected_position_id, self.cache.position_closed_ids(strategy_id=strategy.id))
        self.assertNotIn(expected_position_id, self.cache.position_closed_ids())
        self.assertIn(expected_position_id, self.cache.position_open_ids(strategy_id=strategy.id))
        self.assertIn(expected_position_id, self.cache.position_open_ids())
        self.assertEqual(1, self.cache.positions_total_count())
        self.assertEqual(1, self.cache.positions_open_count())
        self.assertEqual(0, self.cache.positions_closed_count())

    def test_handle_position_opening_with_position_id_none(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            clock=self.clock,
            uuid_factory=UUIDFactory(),
            logger=self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        order = strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
        )

        submit_order = SubmitOrder(
            self.venue,
            self.trader_id,
            self.account_id,
            strategy.id,
            PositionId.null(),
            order,
            self.uuid_factory.generate(),
            self.clock.utc_now())

        self.exec_engine.execute(submit_order)

        # Act
        self.exec_engine.process(TestStubs.event_order_submitted(order))
        self.exec_engine.process(TestStubs.event_order_accepted(order))
        self.exec_engine.process(TestStubs.event_order_filled(order))

        expected_id = PositionId("O-19700101-000000-000-001-1")  # Stubbed from order id

        # Assert
        self.assertTrue(self.cache.position_exists(expected_id))
        self.assertTrue(self.cache.is_position_open(expected_id))
        self.assertFalse(self.cache.is_position_closed(expected_id))
        self.assertEqual(Position, type(self.cache.position(expected_id)))
        self.assertIn(expected_id, self.cache.position_ids())
        self.assertNotIn(expected_id, self.cache.position_closed_ids(strategy_id=strategy.id))
        self.assertNotIn(expected_id, self.cache.position_closed_ids())
        self.assertIn(expected_id, self.cache.position_open_ids(strategy_id=strategy.id))
        self.assertIn(expected_id, self.cache.position_open_ids())
        self.assertEqual(1, self.cache.positions_total_count())
        self.assertEqual(1, self.cache.positions_open_count())
        self.assertEqual(0, self.cache.positions_closed_count())

    def test_add_to_existing_position_on_order_fill(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            clock=self.clock,
            uuid_factory=UUIDFactory(),
            logger=self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        order1 = strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
        )

        order2 = strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
        )

        submit_order1 = SubmitOrder(
            self.venue,
            self.trader_id,
            self.account_id,
            strategy.id,
            PositionId.null(),
            order1,
            self.uuid_factory.generate(),
            self.clock.utc_now(),
        )

        self.exec_engine.execute(submit_order1)
        self.exec_engine.process(TestStubs.event_order_submitted(order1))
        self.exec_engine.process(TestStubs.event_order_accepted(order1))
        self.exec_engine.process(TestStubs.event_order_filled(order1))

        expected_position_id = PositionId("O-19700101-000000-000-001-1")  # Stubbed from order id?

        submit_order2 = SubmitOrder(
            self.venue,
            self.trader_id,
            self.account_id,
            strategy.id,
            expected_position_id,
            order2,
            self.uuid_factory.generate(),
            self.clock.utc_now(),
        )

        # Act
        self.exec_engine.execute(submit_order2)
        self.exec_engine.process(TestStubs.event_order_submitted(order2))
        self.exec_engine.process(TestStubs.event_order_accepted(order2))
        self.exec_engine.process(TestStubs.event_order_filled(order2, expected_position_id))

        # Assert
        self.assertTrue(self.cache.position_exists(TestStubs.event_order_filled(order1).position_id))
        self.assertTrue(self.cache.is_position_open(expected_position_id))
        self.assertFalse(self.cache.is_position_closed(expected_position_id))
        self.assertEqual(Position, type(self.cache.position(expected_position_id)))
        self.assertEqual(0, len(self.cache.positions_closed(strategy_id=strategy.id)))
        self.assertEqual(0, len(self.cache.positions_closed()))
        self.assertEqual(1, len(self.cache.positions_open(strategy_id=strategy.id)))
        self.assertEqual(1, len(self.cache.positions_open()))
        self.assertEqual(1, self.cache.positions_total_count())
        self.assertEqual(1, self.cache.positions_open_count())
        self.assertEqual(0, self.cache.positions_closed_count())

    def test_close_position_on_order_fill(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            clock=self.clock,
            uuid_factory=UUIDFactory(),
            logger=self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        order1 = strategy.order_factory.stop_market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price("1.00000"),
        )

        order2 = strategy.order_factory.stop_market(
            AUDUSD_FXCM,
            OrderSide.SELL,
            Quantity(100000),
            Price("1.00000"),
        )

        submit_order1 = SubmitOrder(
            self.venue,
            self.trader_id,
            self.account_id,
            strategy.id,
            PositionId.null(),
            order1,
            self.uuid_factory.generate(),
            self.clock.utc_now(),
        )

        position_id = PositionId("P-1")

        self.exec_engine.execute(submit_order1)
        self.exec_engine.process(TestStubs.event_order_submitted(order1))
        self.exec_engine.process(TestStubs.event_order_accepted(order1))
        self.exec_engine.process(TestStubs.event_order_filled(order1, position_id))

        submit_order2 = SubmitOrder(
            self.venue,
            self.trader_id,
            self.account_id,
            strategy.id,
            position_id,
            order2,
            self.uuid_factory.generate(),
            self.clock.utc_now(),
        )

        # Act
        self.exec_engine.execute(submit_order2)
        self.exec_engine.process(TestStubs.event_order_submitted(order2))
        self.exec_engine.process(TestStubs.event_order_accepted(order2))
        self.exec_engine.process(TestStubs.event_order_filled(order2, position_id))

        # # Assert
        self.assertTrue(self.cache.position_exists(position_id))
        self.assertFalse(self.cache.is_position_open(position_id))
        self.assertTrue(self.cache.is_position_closed(position_id))
        self.assertEqual(position_id, self.cache.position(position_id).id)
        self.assertEqual(position_id, self.cache.positions(strategy_id=strategy.id)[0].id)
        self.assertEqual(position_id, self.cache.positions()[0].id)
        self.assertEqual(0, len(self.cache.positions_open(strategy_id=strategy.id)))
        self.assertEqual(0, len(self.cache.positions_open()))
        self.assertEqual(position_id, self.cache.positions_closed(strategy_id=strategy.id)[0].id)
        self.assertEqual(position_id, self.cache.positions_closed()[0].id)
        self.assertNotIn(position_id, self.cache.position_open_ids(strategy_id=strategy.id))
        self.assertNotIn(position_id, self.cache.position_open_ids())
        self.assertEqual(1, self.cache.positions_total_count())
        self.assertEqual(0, self.cache.positions_open_count())
        self.assertEqual(1, self.cache.positions_closed_count())

    def test_multiple_strategy_positions_opened(self):
        # Arrange
        strategy1 = TradingStrategy(order_id_tag="001")
        strategy1.register_trader(
            TraderId("TESTER", "000"),
            clock=self.clock,
            uuid_factory=UUIDFactory(),
            logger=self.logger,
        )

        strategy2 = TradingStrategy(order_id_tag="002")
        strategy2.register_trader(
            TraderId("TESTER", "000"),
            clock=self.clock,
            uuid_factory=UUIDFactory(),
            logger=self.logger,
        )

        self.exec_engine.register_strategy(strategy1)
        self.exec_engine.register_strategy(strategy2)

        order1 = strategy1.order_factory.stop_market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price("1.00000"),
        )

        order2 = strategy2.order_factory.stop_market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price("1.00000"),
        )

        submit_order1 = SubmitOrder(
            self.venue,
            self.trader_id,
            self.account_id,
            strategy1.id,
            PositionId.null(),
            order1,
            self.uuid_factory.generate(),
            self.clock.utc_now(),
        )

        submit_order2 = SubmitOrder(
            self.venue,
            self.trader_id,
            self.account_id,
            strategy2.id,
            PositionId.null(),
            order2,
            self.uuid_factory.generate(),
            self.clock.utc_now(),
        )

        position1_id = PositionId('P-1')
        position2_id = PositionId('P-2')

        # Act
        self.exec_engine.execute(submit_order1)
        self.exec_engine.execute(submit_order2)
        self.exec_engine.process(TestStubs.event_order_submitted(order1))
        self.exec_engine.process(TestStubs.event_order_accepted(order1))
        self.exec_engine.process(TestStubs.event_order_filled(order1, position1_id))
        self.exec_engine.process(TestStubs.event_order_submitted(order2))
        self.exec_engine.process(TestStubs.event_order_accepted(order2))
        self.exec_engine.process(TestStubs.event_order_filled(order2, position2_id))

        # Assert
        self.assertTrue(self.cache.position_exists(position1_id))
        self.assertTrue(self.cache.position_exists(position2_id))
        self.assertTrue(self.cache.is_position_open(position1_id))
        self.assertTrue(self.cache.is_position_open(position2_id))
        self.assertFalse(self.cache.is_position_closed(position1_id))
        self.assertFalse(self.cache.is_position_closed(position2_id))
        self.assertEqual(Position, type(self.cache.position(position1_id)))
        self.assertEqual(Position, type(self.cache.position(position2_id)))
        self.assertIn(position1_id, self.cache.position_ids(strategy_id=strategy1.id))
        self.assertIn(position2_id, self.cache.position_ids(strategy_id=strategy2.id))
        self.assertIn(position1_id, self.cache.position_ids())
        self.assertIn(position2_id, self.cache.position_ids())
        self.assertEqual(2, len(self.cache.position_open_ids()))
        self.assertEqual(1, len(self.cache.positions_open(strategy_id=strategy1.id)))
        self.assertEqual(1, len(self.cache.positions_open(strategy_id=strategy2.id)))
        self.assertEqual(1, len(self.cache.positions_open(strategy_id=strategy2.id)))
        self.assertEqual(2, len(self.cache.positions_open()))
        self.assertEqual(1, len(self.cache.positions_open(strategy_id=strategy1.id)))
        self.assertEqual(1, len(self.cache.positions_open(strategy_id=strategy2.id)))
        self.assertIn(position1_id, self.cache.position_open_ids(strategy_id=strategy1.id))
        self.assertIn(position2_id, self.cache.position_open_ids(strategy_id=strategy2.id))
        self.assertIn(position1_id, self.cache.position_open_ids())
        self.assertIn(position2_id, self.cache.position_open_ids())
        self.assertNotIn(position1_id, self.cache.position_closed_ids(strategy_id=strategy1.id))
        self.assertNotIn(position2_id, self.cache.position_closed_ids(strategy_id=strategy2.id))
        self.assertNotIn(position1_id, self.cache.position_closed_ids())
        self.assertNotIn(position2_id, self.cache.position_closed_ids())
        self.assertEqual(2, self.cache.positions_total_count())
        self.assertEqual(2, self.cache.positions_open_count())
        self.assertEqual(0, self.cache.positions_closed_count())

    def test_multiple_strategy_positions_one_active_one_closed(self):
        # Arrange
        strategy1 = TradingStrategy(order_id_tag="001")
        strategy1.register_trader(
            TraderId("TESTER", "000"),
            clock=self.clock,
            uuid_factory=UUIDFactory(),
            logger=self.logger,
        )

        strategy2 = TradingStrategy(order_id_tag="002")
        strategy2.register_trader(
            TraderId("TESTER", "000"),
            clock=self.clock,
            uuid_factory=UUIDFactory(),
            logger=self.logger,
        )

        self.exec_engine.register_strategy(strategy1)
        self.exec_engine.register_strategy(strategy2)

        order1 = strategy1.order_factory.stop_market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price("1.00000"),
        )

        order2 = strategy1.order_factory.stop_market(
            AUDUSD_FXCM,
            OrderSide.SELL,
            Quantity(100000),
            Price("1.00000"),
        )

        order3 = strategy2.order_factory.stop_market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price("1.00000"),
        )

        submit_order1 = SubmitOrder(
            self.venue,
            self.trader_id,
            self.account_id,
            strategy1.id,
            PositionId.null(),
            order1,
            self.uuid_factory.generate(),
            self.clock.utc_now(),
        )

        position_id1 = PositionId('P-1')

        submit_order2 = SubmitOrder(
            self.venue,
            self.trader_id,
            self.account_id,
            strategy1.id,
            position_id1,
            order2,
            self.uuid_factory.generate(),
            self.clock.utc_now(),
        )

        submit_order3 = SubmitOrder(
            self.venue,
            self.trader_id,
            self.account_id,
            strategy2.id,
            PositionId.null(),
            order3,
            self.uuid_factory.generate(),
            self.clock.utc_now(),
        )

        position_id2 = PositionId('P-2')

        # Act
        self.exec_engine.execute(submit_order1)
        self.exec_engine.process(TestStubs.event_order_submitted(order1))
        self.exec_engine.process(TestStubs.event_order_accepted(order1))
        self.exec_engine.process(TestStubs.event_order_filled(order1, position_id1))

        self.exec_engine.execute(submit_order2)
        self.exec_engine.process(TestStubs.event_order_submitted(order2))
        self.exec_engine.process(TestStubs.event_order_accepted(order2))
        self.exec_engine.process(TestStubs.event_order_filled(order2, position_id1))

        self.exec_engine.execute(submit_order3)
        self.exec_engine.process(TestStubs.event_order_submitted(order3))
        self.exec_engine.process(TestStubs.event_order_accepted(order3))
        self.exec_engine.process(TestStubs.event_order_filled(order3, position_id2))

        # Assert
        # Already tested .is_position_active and .is_position_closed above
        self.assertTrue(self.cache.position_exists(position_id1))
        self.assertTrue(self.cache.position_exists(position_id2))
        self.assertIn(position_id1, self.cache.position_ids(strategy_id=strategy1.id))
        self.assertIn(position_id2, self.cache.position_ids(strategy_id=strategy2.id))
        self.assertIn(position_id1, self.cache.position_ids())
        self.assertIn(position_id2, self.cache.position_ids())
        self.assertEqual(0, len(self.cache.positions_open(strategy_id=strategy1.id)))
        self.assertEqual(1, len(self.cache.positions_open(strategy_id=strategy2.id)))
        self.assertEqual(1, len(self.cache.positions_open()))
        self.assertEqual(1, len(self.cache.positions_closed()))
        self.assertEqual(2, len(self.cache.positions()))
        self.assertNotIn(position_id1, self.cache.position_open_ids(strategy_id=strategy1.id))
        self.assertIn(position_id2, self.cache.position_open_ids(strategy_id=strategy2.id))
        self.assertNotIn(position_id1, self.cache.position_open_ids())
        self.assertIn(position_id2, self.cache.position_open_ids())
        self.assertIn(position_id1, self.cache.position_closed_ids(strategy_id=strategy1.id))
        self.assertNotIn(position_id2, self.cache.position_closed_ids(strategy_id=strategy2.id))
        self.assertIn(position_id1, self.cache.position_closed_ids())
        self.assertNotIn(position_id2, self.cache.position_closed_ids())
        self.assertEqual(2, self.cache.positions_total_count())
        self.assertEqual(1, self.cache.positions_open_count())
        self.assertEqual(1, self.cache.positions_closed_count())

    def test_flip_position_on_opposite_filled_same_position(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            clock=self.clock,
            uuid_factory=UUIDFactory(),
            logger=self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        order1 = strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
        )

        order2 = strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.SELL,
            Quantity(150000),
        )

        submit_order1 = SubmitOrder(
            self.venue,
            self.trader_id,
            self.account_id,
            strategy.id,
            PositionId.null(),
            order1,
            self.uuid_factory.generate(),
            self.clock.utc_now(),
        )

        position_id = PositionId("P-000-AUD/USD.FXCM-1")

        self.exec_engine.execute(submit_order1)
        self.exec_engine.process(TestStubs.event_order_submitted(order1))
        self.exec_engine.process(TestStubs.event_order_accepted(order1))
        self.exec_engine.process(TestStubs.event_order_filled(order1, position_id))

        submit_order2 = SubmitOrder(
            self.venue,
            self.trader_id,
            self.account_id,
            strategy.id,
            position_id,
            order2,
            self.uuid_factory.generate(),
            self.clock.utc_now(),
        )

        # Act
        self.exec_engine.execute(submit_order2)
        self.exec_engine.process(TestStubs.event_order_submitted(order2))
        self.exec_engine.process(TestStubs.event_order_accepted(order2))
        self.exec_engine.process(TestStubs.event_order_filled(order2, position_id))

        position_id_flipped = PositionId("P-000-AUD/USD.FXCM-1F")
        order_id_flipped = ClientOrderId(order2.cl_ord_id.value + 'F')

        # Assert
        self.assertTrue(self.cache.position_exists(position_id))
        self.assertTrue(self.cache.position_exists(position_id_flipped))
        self.assertTrue(self.cache.is_position_closed(position_id))
        self.assertTrue(self.cache.is_position_open(position_id_flipped))
        self.assertIn(position_id, self.cache.position_ids())
        self.assertIn(position_id, self.cache.position_ids(strategy_id=strategy.id))
        self.assertIn(position_id_flipped, self.cache.position_ids())
        self.assertIn(position_id_flipped, self.cache.position_ids(strategy_id=strategy.id))
        self.assertEqual(2, self.cache.positions_total_count())
        self.assertEqual(1, self.cache.positions_open_count())
        self.assertEqual(1, self.cache.positions_closed_count())
