# -------------------------------------------------------------------------------------------------
# <copyright file="test_live_execution.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest
import time
import zmq

from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.objects import Quantity, Venue, Symbol, Price
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.common.account import Account
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.guid import LiveGuidFactory
from nautilus_trader.common.portfolio import Portfolio
from nautilus_trader.common.execution import InMemoryExecutionDatabase
from nautilus_trader.network.responses import MessageReceived
from nautilus_trader.serialization.serializers import MsgPackCommandSerializer, MsgPackResponseSerializer
from nautilus_trader.live.execution import LiveExecutionEngine, LiveExecClient
from nautilus_trader.live.logger import LiveLogger
from test_kit.stubs import TestStubs
from test_kit.mocks import MockCommandRouter, MockPublisher
from test_kit.strategies import TestStrategy1

UNIX_EPOCH = TestStubs.unix_epoch()
AUDUSD_FXCM = Symbol('AUDUSD', Venue('FXCM'))
GBPUSD_FXCM = Symbol('GBPUSD', Venue('FXCM'))

UTF8 = 'utf8'
LOCAL_HOST = "127.0.0.1"


class LiveExecClientTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        zmq_context = zmq.Context()
        commands_port = 56555
        events_port = 56556

        trader_id = TraderId('000')
        self.bar_type = TestStubs.bartype_audusd_1min_bid()

        self.clock = LiveClock()
        self.guid_factory = LiveGuidFactory()
        self.logger = LiveLogger()
        self.account = Account()

        self.portfolio = Portfolio(
            clock=self.clock,
            guid_factory=self.guid_factory,
            logger=self.logger)

        self.exec_db = InMemoryExecutionDatabase(trader_id=trader_id, logger=self.logger)
        self.exec_engine = LiveExecutionEngine(
            database=self.exec_db,
            account=self.account,
            portfolio=self.portfolio,
            clock=self.clock,
            guid_factory=self.guid_factory,
            logger=self.logger)

        self.exec_client = LiveExecClient(
            exec_engine=self.exec_engine,
            zmq_context=zmq_context,
            commands_port=commands_port,
            events_port=events_port)

        self.exec_engine.register_client(self.exec_client)

        self.response_list = []
        self.response_handler = self.response_list.append

        self.command_router = MockCommandRouter(
            zmq_context,
            commands_port,
            MsgPackCommandSerializer(),
            MsgPackResponseSerializer())
        self.command_router.start()
        self.event_publisher = MockPublisher(zmq_context, events_port)

        self.exec_client.connect()

    def tearDown(self):
        # Tear Down
        self.exec_client.disconnect()
        self.command_router.stop()
        self.event_publisher.stop()

    def test_can_send_submit_order_command(self):
        # Arrange
        strategy = TestStrategy1(self.bar_type, id_tag_strategy='001')
        self.exec_engine.register_strategy(strategy)
        order = strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        # Act
        strategy.submit_order(order, strategy.position_id_generator.generate())

        time.sleep(0.1)
        # Assert
        self.assertEqual(order, strategy.order(order.id))
        self.assertEqual(1, len(self.command_router.responses_sent))
        self.assertEqual(MessageReceived, type(self.command_router.responses_sent[0]))

    def test_can_send_submit_atomic_order_no_take_profit_command(self):
        # Arrange
        strategy = TestStrategy1(self.bar_type, id_tag_strategy='002')
        self.exec_engine.register_strategy(strategy)
        atomic_order = strategy.order_factory.atomic_market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price('0.99900'))

        # Act
        strategy.submit_atomic_order(atomic_order, strategy.position_id_generator.generate())

        time.sleep(0.1)
        # Assert
        self.assertEqual(atomic_order.entry, strategy.order(atomic_order.entry.id))
        self.assertEqual(atomic_order.stop_loss, strategy.order(atomic_order.stop_loss.id))
        self.assertEqual(1, len(self.command_router.responses_sent))
        self.assertEqual(MessageReceived, type(self.command_router.responses_sent[0]))

    def test_can_send_submit_atomic_order_with_take_profit_command(self):
        # Arrange
        strategy = TestStrategy1(self.bar_type, id_tag_strategy='003')
        self.exec_engine.register_strategy(strategy)
        atomic_order = strategy.order_factory.atomic_limit(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price('1.00010'),
            Price('1.00000'),
            Price('0.99900'))

        # Act
        strategy.submit_atomic_order(atomic_order, strategy.position_id_generator.generate())

        time.sleep(0.1)
        # Assert
        self.assertEqual(atomic_order.entry, strategy.order(atomic_order.entry.id))
        self.assertEqual(atomic_order.stop_loss, strategy.order(atomic_order.stop_loss.id))
        self.assertEqual(atomic_order.take_profit, strategy.order(atomic_order.take_profit.id))
        self.assertEqual(1, len(self.command_router.responses_sent))
        self.assertEqual(MessageReceived, type(self.command_router.responses_sent[0]))

    def test_can_send_cancel_order_command(self):
        # Arrange
        strategy = TestStrategy1(self.bar_type, id_tag_strategy='004')
        self.exec_engine.register_strategy(strategy)
        order = strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        # Act
        strategy.submit_order(order, strategy.position_id_generator.generate())
        time.sleep(0.1)
        strategy.cancel_order(order, 'ORDER_EXPIRED')

        # Assert
        time.sleep(0.1)
        self.assertEqual(order, strategy.order(order.id))
        self.assertEqual(2, len(self.command_router.responses_sent))
        self.assertEqual(MessageReceived, type(self.command_router.responses_sent[0]))
        self.assertEqual(MessageReceived, type(self.command_router.responses_sent[1]))

    def test_can_send_modify_order_command(self):
        # Arrange
        strategy = TestStrategy1(self.bar_type, id_tag_strategy='005')
        self.exec_engine.register_strategy(strategy)
        order = strategy.order_factory.limit(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price('1.00000'))

        # Act
        strategy.submit_order(order, strategy.position_id_generator.generate())
        time.sleep(0.1)
        strategy.modify_order(order, Price('1.00001'))

        # Assert
        time.sleep(0.1)
        self.assertEqual(order, strategy.order(order.id))
        self.assertEqual(2, len(self.command_router.responses_sent))
        self.assertEqual(MessageReceived, type(self.command_router.responses_sent[0]))
        self.assertEqual(MessageReceived, type(self.command_router.responses_sent[1]))

    def test_can_send_account_inquiry_command(self):
        # Arrange
        strategy = TestStrategy1(self.bar_type, id_tag_strategy='006')
        self.exec_engine.register_strategy(strategy)

        # Act
        strategy.account_inquiry()

        # Assert
        time.sleep(0.1)
        self.assertEqual(1, len(self.command_router.responses_sent))
        self.assertEqual(MessageReceived, type(self.command_router.responses_sent[0]))
