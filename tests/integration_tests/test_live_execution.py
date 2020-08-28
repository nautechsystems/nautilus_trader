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

import redis

from nautilus_trader.backtest.clock import TestClock
from nautilus_trader.backtest.logging import TestLogger
from nautilus_trader.common.account import Account
from nautilus_trader.live.execution_engine import RedisExecutionDatabase
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import OrderId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.position import Position
from nautilus_trader.serialization.serializers import MsgPackCommandSerializer
from nautilus_trader.serialization.serializers import MsgPackEventSerializer
from tests.test_kit.strategies import EmptyStrategy
from tests.test_kit.stubs import TestStubs

AUDUSD_FXCM = TestStubs.symbol_audusd_fxcm()

# Requirements:
#    - A Redis instance listening on the default port 6379


class RedisExecutionDatabaseTests(unittest.TestCase):

    # These tests require a Redis instance listening on the default port 6379

    def setUp(self):
        # Fixture Setup
        clock = TestClock()
        logger = TestLogger()

        self.trader_id = TraderId("TESTER", "000")

        self.strategy = EmptyStrategy(order_id_tag="001")
        self.strategy.register_trader(TraderId("TESTER", "000"))
        self.strategy.change_clock(clock)
        self.strategy.change_logger(logger)

        self.database = RedisExecutionDatabase(
            trader_id=self.trader_id,
            host="localhost",
            port=6379,
            command_serializer=MsgPackCommandSerializer(),
            event_serializer=MsgPackEventSerializer(),
            logger=logger)

        self.test_redis = redis.Redis(host="localhost", port=6379, db=0)

    def tearDown(self):
        # Tests will start failing if redis is not flushed on tear down
        self.test_redis.flushall()  # Comment this line out to preserve data between tests
        pass

    def test_keys(self):
        # Arrange
        # Act
        # Assert
        self.assertEqual("Trader-TESTER-000", self.database.key_trader)
        self.assertEqual("Trader-TESTER-000:Accounts:", self.database.key_accounts)
        self.assertEqual("Trader-TESTER-000:Orders:", self.database.key_orders)
        self.assertEqual("Trader-TESTER-000:Positions:", self.database.key_positions)
        self.assertEqual("Trader-TESTER-000:Strategies:", self.database.key_strategies)
        self.assertEqual("Trader-TESTER-000:Index:OrderPosition", self.database.key_index_order_position)
        self.assertEqual("Trader-TESTER-000:Index:OrderStrategy", self.database.key_index_order_strategy)
        self.assertEqual("Trader-TESTER-000:Index:PositionStrategy", self.database.key_index_position_strategy)
        self.assertEqual("Trader-TESTER-000:Index:PositionOrders:", self.database.key_index_position_orders)
        self.assertEqual("Trader-TESTER-000:Index:StrategyOrders:", self.database.key_index_strategy_orders)
        self.assertEqual("Trader-TESTER-000:Index:StrategyPositions:", self.database.key_index_strategy_positions)
        self.assertEqual("Trader-TESTER-000:Index:Orders:Working", self.database.key_index_orders_working)
        self.assertEqual("Trader-TESTER-000:Index:Orders:Completed", self.database.key_index_orders_completed)
        self.assertEqual("Trader-TESTER-000:Index:Positions:Open", self.database.key_index_positions_open)
        self.assertEqual("Trader-TESTER-000:Index:Positions:Closed", self.database.key_index_positions_closed)

    def test_can_add_account(self):
        # Arrange
        event = TestStubs.account_event()
        account = Account(event)

        # Act
        self.database.add_account(account)

        # Assert
        self.assertEqual(account, self.database.get_account(account.id))

    def test_can_add_order(self):
        # Arrange
        order = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        position_id = self.strategy.position_id_generator.generate()

        # Act
        self.database.add_order(order, self.strategy.id, position_id)

        # Assert
        self.assertTrue(order.id in self.database.get_order_ids())
        self.assertEqual(order, self.database.get_orders()[order.id])

    def test_can_add_position(self):
        # Arrange
        order = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        position_id = self.strategy.position_id_generator.generate()
        self.database.add_order(order, self.strategy.id, position_id)

        order_filled = TestStubs.event_order_filled(order, fill_price=Price(1.00000, 5))
        position = Position(position_id, order_filled)

        # Act
        self.database.add_position(position, self.strategy.id)

        # Assert
        self.assertTrue(self.database.position_exists_for_order(order.id))
        self.assertTrue(self.database.position_exists(position.id))
        self.assertTrue(position.id in self.database.get_position_ids())
        self.assertTrue(position.id in self.database.get_positions())
        self.assertTrue(position.id in self.database.get_positions_open(self.strategy.id))
        self.assertTrue(position.id in self.database.get_positions_open())
        self.assertTrue(position.id not in self.database.get_positions_closed(self.strategy.id))
        self.assertTrue(position.id not in self.database.get_positions_closed())

    def test_can_update_account(self):
        # Arrange
        event = TestStubs.account_event()
        account = Account(event)
        self.database.add_account(account)

        # Act
        self.database.update_account(account)

        # Assert
        self.assertEqual(account, self.database.get_account(account.id))

    def test_can_update_order_for_working_order(self):
        # Arrange
        order = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        position_id = self.strategy.position_id_generator.generate()
        self.database.add_order(order, self.strategy.id, position_id)

        order.apply(TestStubs.event_order_submitted(order))
        self.database.update_order(order)

        order.apply(TestStubs.event_order_accepted(order))
        self.database.update_order(order)

        # Act
        order.apply(TestStubs.event_order_working(order))
        self.database.update_order(order)

        # Assert
        self.assertTrue(self.database.order_exists(order.id))
        self.assertTrue(order.id in self.database.get_order_ids())
        self.assertTrue(order.id in self.database.get_orders())
        self.assertTrue(order.id in self.database.get_orders_working(self.strategy.id))
        self.assertTrue(order.id in self.database.get_orders_working())
        self.assertTrue(order.id not in self.database.get_orders_completed(self.strategy.id))
        self.assertTrue(order.id not in self.database.get_orders_completed())

    def test_can_update_order_for_completed_order(self):
        # Arrange
        order = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        position_id = self.strategy.position_id_generator.generate()
        self.database.add_order(order, self.strategy.id, position_id)

        order.apply(TestStubs.event_order_submitted(order))
        self.database.update_order(order)

        order.apply(TestStubs.event_order_accepted(order))
        self.database.update_order(order)

        order.apply(TestStubs.event_order_filled(order, fill_price=Price(1.00001, 5)))

        # Act
        self.database.update_order(order)

        # Assert
        self.assertTrue(self.database.order_exists(order.id))
        self.assertTrue(order.id in self.database.get_order_ids())
        self.assertTrue(order.id in self.database.get_orders())
        self.assertTrue(order.id in self.database.get_orders_completed(self.strategy.id))
        self.assertTrue(order.id in self.database.get_orders_completed())
        self.assertTrue(order.id not in self.database.get_orders_working(self.strategy.id))
        self.assertTrue(order.id not in self.database.get_orders_working())

    def test_can_update_position_for_closed_position(self):
        # Arrange
        order1 = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        position_id = self.strategy.position_id_generator.generate()
        self.database.add_order(order1, self.strategy.id, position_id)

        order1.apply(TestStubs.event_order_submitted(order1))
        self.database.update_order(order1)

        order1.apply(TestStubs.event_order_accepted(order1))
        self.database.update_order(order1)

        order1.apply(TestStubs.event_order_filled(order1, fill_price=Price(1.00001, 5)))
        self.database.update_order(order1)

        # Act
        position = Position(position_id, order1.last_event)
        self.database.add_position(position, self.strategy.id)

        order2 = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.SELL,
            Quantity(100000))
        self.database.add_order(order2, self.strategy.id, position_id)

        order2.apply(TestStubs.event_order_submitted(order2))
        self.database.update_order(order2)

        order2.apply(TestStubs.event_order_accepted(order2))
        self.database.update_order(order2)

        filled = TestStubs.event_order_filled(order2, fill_price=Price(1.00001, 5))
        order2.apply(filled)
        self.database.update_order(order2)

        position.apply(filled)

        # Act
        self.database.update_position(position)

        # Assert
        self.assertTrue(self.database.position_exists(position.id))
        self.assertTrue(position.id in self.database.get_position_ids())
        self.assertTrue(position.id in self.database.get_positions())
        self.assertTrue(position.id in self.database.get_positions_closed(self.strategy.id))
        self.assertTrue(position.id in self.database.get_positions_closed())
        self.assertTrue(position.id not in self.database.get_positions_open(self.strategy.id))
        self.assertTrue(position.id not in self.database.get_positions_open())
        self.assertEqual(position, self.database.get_position_for_order(order1.id))

    def test_load_account_when_no_account_in_database_returns_none(self):
        # Arrange
        event = TestStubs.account_event()
        account = Account(event)

        # Act
        result = self.database.load_account(account.id)

        # Assert
        self.assertIsNone(result)

    def test_load_account_when_account_in_database_returns_account(self):
        # Arrange
        event = TestStubs.account_event()
        account = Account(event)
        self.database.add_account(account)

        # Act
        result = self.database.load_account(account.id)

        # Assert
        self.assertEqual(account, result)

    def test_load_order_when_no_order_in_database_returns_none(self):
        # Arrange
        order = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        # Act
        result = self.database.load_order(order.id)

        # Assert
        self.assertIsNone(result)

    def test_load_order_when_order_in_database_returns_order(self):
        # Arrange
        order = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        position_id = self.strategy.position_id_generator.generate()
        self.database.add_order(order, self.strategy.id, position_id)

        # Act
        result = self.database.load_order(order.id)

        # Assert
        self.assertEqual(order, result)

    def test_load_position_when_no_position_in_database_returns_none(self):
        # Arrange
        position_id = self.strategy.position_id_generator.generate()

        # Act
        result = self.database.load_position(position_id)

        # Assert
        self.assertIsNone(result)

    def test_load_order_when_position_in_database_returns_position(self):
        # Arrange
        order = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        position_id = self.strategy.position_id_generator.generate()
        self.database.add_order(order, self.strategy.id, position_id)

        order_filled = TestStubs.event_order_filled(order, fill_price=Price(1.00000, 5))
        position = Position(position_id, order_filled)

        self.database.add_position(position, self.strategy.id)

        # Act
        result = self.database.load_position(position_id)
        # Assert
        self.assertEqual(position, result)

    def test_can_load_accounts_cache_when_no_accounts(self):
        # Arrange
        # Act
        self.database.load_accounts_cache()
        event = TestStubs.account_event()
        account = Account(event)

        # Assert
        self.assertIsNone(self.database.get_account(account.id))

    def test_can_load_accounts_cache_when_one_account_in_database(self):
        # Arrange
        event = TestStubs.account_event()
        account = Account(event)
        self.database.add_account(account)
        self.database.reset()

        # Act
        self.database.load_accounts_cache()

        # Assert
        self.assertEqual(account, self.database.get_account(account.id))

    def test_can_load_orders_cache_when_no_orders(self):
        # Arrange
        # Act
        self.database.load_orders_cache()

        # Assert
        self.assertEqual({}, self.database.get_orders())

    def test_can_load_orders_cache_when_one_order_in_database(self):
        # Arrange
        order = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        position_id = self.strategy.position_id_generator.generate()
        self.database.add_order(order, self.strategy.id, position_id)

        # Act
        self.database.load_orders_cache()

        # Assert
        self.assertEqual({order.id: order}, self.database.get_orders())

    def test_can_load_positions_cache_when_no_positions(self):
        # Arrange
        # Act
        self.database.load_positions_cache()

        # Assert
        self.assertEqual({}, self.database.get_positions())

    def test_can_load_positions_cache_when_one_position_in_database(self):
        # Arrange
        order1 = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        position_id = self.strategy.position_id_generator.generate()
        self.database.add_order(order1, self.strategy.id, position_id)

        order1.apply(TestStubs.event_order_submitted(order1))
        order1.apply(TestStubs.event_order_accepted(order1))
        order1.apply(TestStubs.event_order_working(order1))
        order1.apply(TestStubs.event_order_filled(order1, fill_price=Price(1.00001, 5)))

        position = Position(position_id, order1.last_event)
        self.database.add_position(position, self.strategy.id)

        # Act
        self.database.load_positions_cache()

        # Assert
        self.assertEqual({position_id: position}, self.database.get_positions())

    def test_can_delete_strategy(self):
        # Arrange
        # Act
        self.database.delete_strategy(self.strategy)

        # Assert
        self.assertTrue(self.strategy.id not in self.database.get_strategy_ids())

    def test_can_check_residuals(self):
        # Arrange
        order1 = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        position1_id = self.strategy.position_id_generator.generate()
        self.database.add_order(order1, self.strategy.id, position1_id)

        order1.apply(TestStubs.event_order_submitted(order1))
        order1.apply(TestStubs.event_order_accepted(order1))
        order1.apply(TestStubs.event_order_working(order1))

        filled = TestStubs.event_order_filled(order1, fill_price=Price(1.00001, 5))

        order1.apply(filled)

        position1 = Position(position1_id, filled)
        self.database.update_order(order1)
        self.database.add_position(position1, self.strategy.id)

        order2 = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        position2_id = self.strategy.position_id_generator.generate()
        self.database.add_order(order2, self.strategy.id, position2_id)

        order2.apply(TestStubs.event_order_submitted(order2))
        order2.apply(TestStubs.event_order_accepted(order2))
        order2.apply(TestStubs.event_order_working(order2))

        self.database.update_order(order2)

        # Act
        self.database.check_residuals()

        # Does not raise exception

    def test_can_reset(self):
        # Arrange
        order1 = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        position1_id = self.strategy.position_id_generator.generate()
        self.database.add_order(order1, self.strategy.id, position1_id)

        order1.apply(TestStubs.event_order_submitted(order1))
        self.database.update_order(order1)

        order1.apply(TestStubs.event_order_accepted(order1))
        self.database.update_order(order1)

        order1.apply(TestStubs.event_order_working(order1))
        self.database.update_order(order1)

        filled = TestStubs.event_order_filled(order1, fill_price=Price(1.00001, 5))

        order1.apply(filled)

        position1 = Position(position1_id, filled)
        self.database.update_order(order1)
        self.database.add_position(position1, self.strategy.id)

        order2 = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        position2_id = self.strategy.position_id_generator.generate()
        self.database.add_order(order2, self.strategy.id, position2_id)

        order2.apply(TestStubs.event_order_submitted(order2))
        self.database.update_order(order2)

        order2.apply(TestStubs.event_order_accepted(order2))
        self.database.update_order(order2)

        order2.apply(TestStubs.event_order_working(order2))
        self.database.update_order(order2)

        self.database.update_order(order2)

        # Act
        self.database.reset()

        # Assert
        self.assertEqual(0, len(self.database.get_orders()))
        self.assertEqual(0, len(self.database.get_positions()))

    def test_can_flush(self):
        # Arrange
        order1 = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        position1_id = self.strategy.position_id_generator.generate()
        self.database.add_order(order1, self.strategy.id, position1_id)

        order1_filled = TestStubs.event_order_filled(order1, fill_price=Price(1.00000, 5))
        position1 = Position(position1_id, order1_filled)
        self.database.update_order(order1)
        self.database.add_position(position1, self.strategy.id)

        order2 = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        position2_id = self.strategy.position_id_generator.generate()
        self.database.add_order(order2, self.strategy.id, position2_id)

        order2.apply(TestStubs.event_order_submitted(order2))
        order2.apply(TestStubs.event_order_accepted(order2))
        order2.apply(TestStubs.event_order_working(order2))

        self.database.update_order(order2)

        # Act
        self.database.reset()
        self.database.flush()

        # Assert
        # Does not raise exception

    def test_get_strategy_ids_with_no_ids_returns_empty_set(self):
        # Arrange
        # Act
        result = self.database.get_strategy_ids()

        # Assert
        self.assertEqual(set(), result)

    def test_get_strategy_ids_with_id_returns_correct_set(self):
        # Arrange
        self.database.update_strategy(self.strategy)

        # Act
        result = self.database.get_strategy_ids()

        # Assert
        self.assertEqual({self.strategy.id}, result)

    def test_position_exists_when_no_position_returns_false(self):
        # Arrange
        # Act
        # Assert
        self.assertFalse(self.database.position_exists(PositionId("P-123456")))

    def test_position_exists_for_order_when_no_position_returns_false(self):
        # Arrange
        # Act
        # Assert
        self.assertFalse(self.database.position_exists_for_order(OrderId("O-123456")))

    def test_position_indexed_for_order_when_no_indexing_returns_false(self):
        # Arrange
        # Act
        # Assert
        self.assertFalse(self.database.position_indexed_for_order(OrderId("O-123456")))

    def test_order_exists_when_no_order_returns_false(self):
        # Arrange
        # Act
        # Assert
        self.assertFalse(self.database.order_exists(OrderId("O-123456")))

    def test_position_for_order_when_not_found_returns_none(self):
        # Arrange
        # Act
        # Assert
        self.assertIsNone(self.database.get_position_for_order(OrderId("O-123456")))

    def test_get_order_when_no_order_returns_none(self):
        # Arrange
        position_id = PositionId("P-123456")

        # Act
        result = self.database.get_position(position_id)

        # Assert
        self.assertIsNone(result)

    def test_get_position_when_no_position_returns_none(self):
        # Arrange
        order_id = OrderId("O-201908080101-000-001")

        # Act
        result = self.database.get_order(order_id)

        # Assert
        self.assertIsNone(result)
