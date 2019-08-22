# -------------------------------------------------------------------------------------------------
# <copyright file="test_live_execution.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import uuid
import unittest
from redis import Redis

from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.core.types import GUID
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.events import OrderFilled, OrderWorking
from nautilus_trader.model.identifiers import Symbol, Venue, IdTag, StrategyId, PositionId, OrderId, ExecutionId, ExecutionTicket
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
from test_kit.strategies import EmptyStrategy

UNIX_EPOCH = TestStubs.unix_epoch()
AUDUSD_FXCM = Symbol('AUDUSD', Venue('FXCM'))

# Requirements:
#    - A Redis instance listening on the default port 6379


class RedisExecutionDatabaseTests(unittest.TestCase):

    # These tests require a Redis instance listening on the default port 6379

    def setUp(self):
        # Fixture Setup
        clock = LiveClock()
        guid_factory = LiveGuidFactory()
        logger = LiveLogger()

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

        self.redis = Redis(host='localhost', port=6379, db=0)

    def test_can_add_strategy(self):
        # Arrange
        strategy = EmptyStrategy('000')

        # Act
        self.database.add_strategy(strategy)

        # Assert
        self.assertTrue(self.redis.hexists(name='Trader-TESTER-000:Strategies:EmptyStrategy-000:Config', key='some_value'))

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
        # self.assertTrue(order.id in self.database.get_order_ids())
        # self.assertEqual(order, self.database.get_orders_all()[order.id])

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
        self.database.add_order_event(order_working, strategy.id, order.is_working, order.is_complete)

        # Assert
        # self.assertTrue(self.exec_db.order_exists(order.id))
        # self.assertTrue(order.id in self.exec_db.get_order_ids())
        # self.assertTrue(order.id in self.exec_db.get_orders_all())
        # self.assertTrue(order.id in self.exec_db.get_orders_working(strategy.id))
        # self.assertTrue(order.id in self.exec_db.get_orders_working_all()[strategy.id])
        # self.assertTrue(order.id not in self.exec_db.get_orders_completed(strategy.id))
        # self.assertTrue(order.id not in self.exec_db.get_orders_completed_all()[strategy.id])