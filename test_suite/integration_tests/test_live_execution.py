# -------------------------------------------------------------------------------------------------
# <copyright file="test_live_execution.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import uuid
import unittest

from decimal import Decimal
from redis import Redis

from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.core.types import GUID, ValidString
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.events import OrderFilled, OrderWorking
from nautilus_trader.model.objects import Quantity, Price
from nautilus_trader.model.order import OrderFactory
from nautilus_trader.model.position import Position
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.guid import TestGuidFactory
from nautilus_trader.common.logger import TestLogger
from nautilus_trader.live.execution import RedisExecutionDatabase
from test_kit.stubs import TestStubs
from nautilus_trader.common.account import Account
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.guid import LiveGuidFactory
from nautilus_trader.common.portfolio import Portfolio
from nautilus_trader.common.execution import InMemoryExecutionDatabase
from nautilus_trader.network.responses import MessageReceived
from nautilus_trader.serialization.serializers import MsgPackCommandSerializer, MsgPackEventSerializer
from nautilus_trader.live.execution import LiveExecutionEngine, LiveExecClient
from nautilus_trader.live.logger import LiveLogger
from nautilus_trader.model.events import OrderFilled, OrderWorking
from nautilus_trader.model.identifiers import (

    AccountId,
    TraderId,
    StrategyId,
    IdTag,
    OrderId,
    PositionId,
    ExecutionId,
    ExecutionTicket)
from nautilus_trader.model.objects import Quantity, Price, Money
from nautilus_trader.model.order import OrderFactory
from nautilus_trader.model.position import Position
from nautilus_trader.model.commands import SubmitOrder
from nautilus_trader.model.events import AccountStateEvent
from nautilus_trader.model.enums import Currency
from nautilus_trader.common.account import Account
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.guid import TestGuidFactory
from nautilus_trader.common.logger import TestLogger
from nautilus_trader.common.portfolio import Portfolio
from nautilus_trader.common.execution import InMemoryExecutionDatabase, ExecutionEngine
from nautilus_trader.trade.strategy import TradingStrategy
from test_kit.strategies import EmptyStrategy

UNIX_EPOCH = TestStubs.unix_epoch()
AUDUSD_FXCM = TestStubs.symbol_audusd_fxcm()

# Requirements:
#    - A Redis instance listening on the default port 6379


class RedisExecutionDatabaseTests(unittest.TestCase):

    # These tests require a Redis instance listening on the default port 6379

    def setUp(self):
        # Fixture Setup
        clock = TestClock()
        logger = TestLogger()

        self.trader_id = TraderId('TESTER', '000')

        self.strategy = TradingStrategy(order_id_tag='001')
        self.strategy.change_clock(clock)
        self.strategy.change_logger(logger)

        self.account = Account()
        self.database = RedisExecutionDatabase(
            trader_id=self.trader_id,
            host='localhost',
            port=6379,
            command_serializer=MsgPackCommandSerializer(),
            event_serializer=MsgPackEventSerializer(),
            logger=logger)

        self.test_redis = Redis(host='localhost', port=6379, db=0)

    def tearDown(self):
        # Tear down
        self.test_redis.flushall()  # Comment this line out to preserve data between tests
        pass

    def test_redis_functions(self):
        #print(self.test_redis.sinter(keys=('a', 'b')))
        #print(self.test_redis.scard(name='*'))
        # a = {'a': 1, 'b': 2}
        # x = {'a'}
        # print(list(a.keys()))
        # print(x.intersection(list(a.keys())))
        # print(set(a.keys()))
        pass

    def test_keys(self):
        # Arrange
        # Act
        # Assert
        self.assertEqual('Trader-TESTER-000', self.database.key_trader)
        self.assertEqual('Trader-TESTER-000:Accounts:', self.database.key_accounts)
        self.assertEqual('Trader-TESTER-000:Orders:', self.database.key_orders)
        self.assertEqual('Trader-TESTER-000:Positions:', self.database.key_positions)
        self.assertEqual('Trader-TESTER-000:Strategies:', self.database.key_strategies)
        self.assertEqual('Trader-TESTER-000:Index:OrderPosition', self.database.key_index_order_position)
        self.assertEqual('Trader-TESTER-000:Index:OrderStrategy', self.database.key_index_order_strategy)
        self.assertEqual('Trader-TESTER-000:Index:PositionStrategy', self.database.key_index_position_strategy)
        self.assertEqual('Trader-TESTER-000:Index:PositionOrders:', self.database.key_index_position_orders)
        self.assertEqual('Trader-TESTER-000:Index:StrategyOrders:', self.database.key_index_strategy_orders)
        self.assertEqual('Trader-TESTER-000:Index:StrategyPositions:', self.database.key_index_strategy_positions)
        self.assertEqual('Trader-TESTER-000:Index:Orders:Working', self.database.key_index_orders_working)
        self.assertEqual('Trader-TESTER-000:Index:Orders:Completed', self.database.key_index_orders_completed)
        self.assertEqual('Trader-TESTER-000:Index:Positions:Open', self.database.key_index_positions_open)
        self.assertEqual('Trader-TESTER-000:Index:Positions:Closed', self.database.key_index_positions_closed)

    def test_can_add_strategy(self):
        # Arrange
        # Act
        self.database.add_strategy(self.strategy)

        # Assert
        self.assertTrue(self.strategy.id in self.database.get_strategy_ids())

    def test_can_add_order(self):
        # Arrange
        self.database.add_strategy(self.strategy)
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
        self.database.add_strategy(self.strategy)
        order = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        position_id = self.strategy.position_id_generator.generate()
        self.database.add_order(order, self.strategy.id, position_id)

        order_filled = TestStubs.event_order_filled(order, fill_price=Price('1.00000'))
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

    def test_can_update_order_for_working_order(self):
        # Arrange
        self.database.add_strategy(self.strategy)
        order = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        position_id = self.strategy.position_id_generator.generate()
        self.database.add_order(order, self.strategy.id, position_id)

        order_working = TestStubs.event_order_working(order)
        order.apply(order_working)

        # Act
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
        self.database.add_strategy(self.strategy)
        order = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        position_id = self.strategy.position_id_generator.generate()
        self.database.add_order(order, self.strategy.id, position_id)

        order_filled = TestStubs.event_order_filled(order, fill_price=Price('1.00001'))
        order.apply(order_filled)

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
        self.database.add_strategy(self.strategy)
        order1 = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        position_id = self.strategy.position_id_generator.generate()
        self.database.add_order(order1, self.strategy.id, position_id)

        order1_filled = TestStubs.event_order_filled(order1, fill_price=Price('1.00001'))
        order1.apply(order1_filled)
        position = Position(position_id, order1.last_event)
        self.database.add_position(position, self.strategy.id)

        order2 = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.SELL,
            Quantity(100000))
        order2_filled = TestStubs.event_order_filled(order2, fill_price=Price('1.00001'))
        position.apply(order2_filled)

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

    def test_can_update_account(self):
        # Arrange
        account = Account()
        event = AccountStateEvent(
            AccountId('SIMULATED', '123456'),
            Currency.USD,
            Money(1000000),
            Money(1000000),
            Money.zero(),
            Money.zero(),
            Money.zero(),
            Decimal(0),
            ValidString(),
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        account.apply(event)

        # Act
        self.database.update_account(account)

        # Assert
        self.assertTrue(True)  # Did not raise exception

    def test_can_delete_strategy(self):
        # Arrange
        self.database.add_strategy(self.strategy)

        # Act
        self.database.delete_strategy(self.strategy)

        # Assert
        self.assertTrue(self.strategy.id not in self.database.get_strategy_ids())

    def test_can_check_residuals(self):
        # Arrange
        self.database.add_strategy(self.strategy)
        order1 = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        position1_id = self.strategy.position_id_generator.generate()
        self.database.add_order(order1, self.strategy.id, position1_id)

        order1_filled = TestStubs.event_order_filled(order1, fill_price=Price('1.00000'))
        position1 = Position(position1_id, order1_filled)
        self.database.update_order(order1)
        self.database.add_position(position1, self.strategy.id)

        order2 = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        position2_id = self.strategy.position_id_generator.generate()
        self.database.add_order(order2, self.strategy.id, position2_id)
        order2_working = TestStubs.event_order_working(order2)
        order2.apply(order2_working)

        self.database.update_order(order2)

        # Act
        self.database.check_residuals()

        # Does not raise exception

    def test_can_reset(self):
        # Arrange
        self.database.add_strategy(self.strategy)
        order1 = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        position1_id = self.strategy.position_id_generator.generate()
        self.database.add_order(order1, self.strategy.id, position1_id)

        order1_filled = TestStubs.event_order_filled(order1, fill_price=Price('1.00000'))
        position1 = Position(position1_id, order1_filled)
        self.database.update_order(order1)
        self.database.add_position(position1, self.strategy.id)

        order2 = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        position2_id = self.strategy.position_id_generator.generate()
        self.database.add_order(order2, self.strategy.id, position2_id)
        order2_working = TestStubs.event_order_working(order2)
        order2.apply(order2_working)

        self.database.update_order(order2)

        # Act
        self.database.reset()

        # Assert
        self.assertEqual(0, len(self.database.get_orders()))
        self.assertEqual(0, len(self.database.get_positions()))

    def test_can_flush(self):
        # Arrange
        self.database.add_strategy(self.strategy)
        order1 = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        position1_id = self.strategy.position_id_generator.generate()
        self.database.add_order(order1, self.strategy.id, position1_id)

        order1_filled = TestStubs.event_order_filled(order1, fill_price=Price('1.00000'))
        position1 = Position(position1_id, order1_filled)
        self.database.update_order(order1)
        self.database.add_position(position1, self.strategy.id)

        order2 = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        position2_id = self.strategy.position_id_generator.generate()
        self.database.add_order(order2, self.strategy.id, position2_id)
        order2_working = TestStubs.event_order_working(order2)
        order2.apply(order2_working)

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
        self.database.add_strategy(self.strategy)

        # Act
        result = self.database.get_strategy_ids()

        # Assert
        self.assertEqual({self.strategy.id}, result)

    def test_position_exists_when_no_position_returns_false(self):
        # Arrange
        # Act
        # Assert
        self.assertFalse(self.database.position_exists(PositionId('unknown')))

    def test_position_exists_for_order_when_no_position_returns_false(self):
        # Arrange
        # Act
        # Assert
        self.assertFalse(self.database.position_exists_for_order(OrderId('unknown')))

    def test_order_exists_when_no_order_returns_false(self):
        # Arrange
        # Act
        # Assert
        self.assertFalse(self.database.order_exists(OrderId('unknown')))

    def test_position_for_order_when_not_found_returns_none(self):
        # Arrange
        # Act
        # Assert
        self.assertIsNone(self.database.get_position_for_order(OrderId('unknown')))

    def test_get_order_when_no_order_returns_none(self):
        # Arrange
        position_id = PositionId('AUDUSD.FXCM-1-123456')

        # Act
        result = self.database.get_position(position_id)

        # Assert
        self.assertIsNone(result)

    def test_get_position_when_no_position_returns_none(self):
        # Arrange
        order_id = OrderId('O-201908080101-000-001')

        # Act
        result = self.database.get_order(order_id)

        # Assert
        self.assertIsNone(result)
