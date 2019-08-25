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
        guid_factory = TestGuidFactory()
        logger = TestLogger()

        self.trader_id = TraderId('TESTER', '000')

        self.order_factory = OrderFactory(
            id_tag_trader=self.trader_id.order_id_tag,
            id_tag_strategy=IdTag('001'),
            clock=clock)

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
        # self.test_redis.flushall()
        pass

    def test_redis_functions(self):
        print(self.test_redis.sinter(keys=('a', 'b')))
        print(self.test_redis.scard(name='*'))

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
        strategy = EmptyStrategy('000')

        # Act
        self.database.add_strategy(strategy)

        # Assert
        self.assertTrue(self.test_redis.hexists(name='Trader-TESTER-000:Strategies:EmptyStrategy-000:Config', key='some_value'))

    def test_can_add_order(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        strategy_id = StrategyId('SCALPER', '001')
        position_id = PositionId('AUDUSD-1-123456')

        # Act
        self.database.add_order(order, strategy_id, position_id)

        # Assert
        self.assertEqual(order, self.database.get_order(order.id))
        self.assertTrue(self.test_redis.exists(self.database.key_orders + order.id.value))
        self.assertTrue(self.test_redis.hexists(self.database.key_index_order_position, order.id.value))
        self.assertTrue(self.test_redis.hexists(self.database.key_index_order_strategy, order.id.value))
        self.assertTrue(self.test_redis.hexists(self.database.key_index_position_strategy, position_id.value))
        self.assertTrue(self.test_redis.exists(self.database.key_index_position_orders + position_id.value))
        self.assertTrue(self.test_redis.exists(self.database.key_index_strategy_orders + strategy_id.value))
        self.assertTrue(self.test_redis.exists(self.database.key_index_strategy_positions + strategy_id.value))

    def test_can_add_position(self):
        # Arrange
        strategy = EmptyStrategy('000')
        order = strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        position_id = strategy.position_id_generator.generate()

        order_filled = OrderFilled(
            order.id,
            self.account.id,
            ExecutionId('E123456'),
            ExecutionTicket('T123456'),
            order.symbol,
            order.side,
            order.quantity,
            Price('1.00001'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        position = Position(position_id, order_filled)

        self.database.add_strategy(strategy)

        # Act
        self.database.add_position(position, strategy.id)

        # Assert
        # self.assertTrue(position.id in self.exec_db.get_position_ids())
        # self.assertTrue(position.id in self.exec_db.get_positions_open(strategy.id))
        # self.assertTrue(position.id in self.exec_db.get_positions_open_all()[strategy.id])

    def test_can_reset_database(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        strategy_id = StrategyId('SCALPER', '001')
        position_id = PositionId('AUDUSD-1-123456')

        self.database.add_order(order, strategy_id, position_id)

        # Act
        self.database.reset()

        # Assert
        self.assertIsNone(self.database.get_order(order.id))

    def test_can_load_order_cache(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        strategy_id = StrategyId('SCALPER', '001')
        position_id = PositionId('AUDUSD-1-123456')

        self.database.add_order(order, strategy_id, position_id)
        self.database.reset()  # Clear the cached orders for the test

        # Act
        self.database.load_orders_cache()

        # Assert
        self.assertEqual(order, self.database.get_order(order.id))

    def test_can_add_order_event_with_working_order(self):
        # Arrange
        strategy = EmptyStrategy('000')
        order = strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        position_id = PositionId('AUDUSD-1-123456')

        order_working = OrderWorking(
            order.id,
            OrderId('SOME_BROKER_ID_1'),
            self.account.id,
            order.symbol,
            order.label,
            order.side,
            order.type,
            order.quantity,
            Price('1.00000'),
            order.time_in_force,
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH,
            order.expire_time)

        order.apply(order_working)

        self.database.add_strategy(strategy)
        self.database.add_order(order, strategy.id, position_id)

        # Act
        self.database.update_order(order)

        # Assert
        # self.assertTrue(self.exec_db.order_exists(order.id))
        # self.assertTrue(order.id in self.exec_db.get_order_ids())
        # self.assertTrue(order.id in self.exec_db.get_orders_all())
        # self.assertTrue(order.id in self.exec_db.get_orders_working(strategy.id))
        # self.assertTrue(order.id in self.exec_db.get_orders_working_all()[strategy.id])
        # self.assertTrue(order.id not in self.exec_db.get_orders_completed(strategy.id))
        # self.assertTrue(order.id not in self.exec_db.get_orders_completed_all()[strategy.id])

    def test_can_add_account_event(self):
        # Arrange
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

        # Act
        self.database.update_account(event)

        # Assert
        self.assertTrue(True)  # Did not raise exception