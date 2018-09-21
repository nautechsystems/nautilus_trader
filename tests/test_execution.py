#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_execution.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest

from datetime import datetime, timezone
from decimal import Decimal

from inv_trader.model.enums import Venue, OrderSide, OrderType, OrderStatus, TimeInForce
from inv_trader.model.objects import Price, Symbol, Resolution, QuoteType, BarType, Bar
from inv_trader.model.order import OrderIdGenerator, OrderFactory
from inv_trader.model.events import OrderSubmitted, OrderAccepted, OrderRejected, OrderWorking
from inv_trader.model.events import OrderExpired, OrderModified, OrderCancelled, OrderCancelReject
from inv_trader.model.events import OrderPartiallyFilled, OrderFilled
from inv_trader.messaging import RequestWorker
from inv_trader.execution import ExecutionClient, LiveExecClient
from test_kit.stubs import TestStubs
from test_kit.mocks import MockExecClient, MockServer, MockPublisher
from test_kit.strategies import TestStrategy1

UNIX_EPOCH = TestStubs.unix_epoch()
AUDUSD_FXCM = Symbol('AUDUSD', Venue.FXCM)
GBPUSD_FXCM = Symbol('GBPUSD', Venue.FXCM)

UTF8 = 'utf8'
LOCAL_HOST = "127.0.0.1"


class ExecutionClientTests(unittest.TestCase):

    def test_can_register_strategy(self):
        # Arrange
        strategy = TestStrategy1()
        exec_client = ExecutionClient()
        exec_client.register_strategy(strategy)

        # Act
        result = strategy._exec_client

        # Assert
        self.assertEqual(exec_client, result)

    def test_can_send_submit_order_command_to_mock_exec_client(self):
        # Arrange
        strategy = TestStrategy1()
        exec_client = MockExecClient()
        exec_client.register_strategy(strategy)
        exec_client.connect()

        order_id = strategy.generate_order_id(AUDUSD_FXCM)

        order = OrderFactory.market(
            AUDUSD_FXCM,
            order_id,
            'S1_E',
            OrderSide.BUY,
            100000)

        # Act
        strategy.submit_order(order, order.id)

        # Assert
        self.assertEqual(order, strategy.order(order_id))
        self.assertEqual(OrderStatus.WORKING, order.status)
        exec_client.disconnect()

    def test_can_send_cancel_order_command_to_mock_exec_clint(self):
        # Arrange
        strategy = TestStrategy1()
        exec_client = MockExecClient()
        exec_client.register_strategy(strategy)
        exec_client.connect()

        order_id = strategy.generate_order_id(AUDUSD_FXCM)

        order = OrderFactory.market(
            AUDUSD_FXCM,
            order_id,
            'S1_E',
            OrderSide.BUY,
            100000)

        # Act
        strategy.submit_order(order, order.id)
        strategy.cancel_order(order, 'ORDER_EXPIRED')

        # Assert
        self.assertEqual(order, strategy.order(order_id))
        self.assertEqual(OrderStatus.CANCELLED, order.status)
        exec_client.disconnect()

    def test_can_send_modify_order_command_to_mock_exec_client(self):
        # Arrange
        strategy = TestStrategy1()
        exec_client = MockExecClient()
        exec_client.register_strategy(strategy)
        exec_client.connect()

        order_id = strategy.generate_order_id(AUDUSD_FXCM)

        order = OrderFactory.limit(
            AUDUSD_FXCM,
            order_id,
            'S1_E',
            OrderSide.BUY,
            100000,
            Price.create(1.00000, 5),
            TimeInForce.DAY)

        # Act
        strategy.submit_order(order, order.id)
        strategy.modify_order(order, Decimal('1.00001'))

        # Assert
        self.assertEqual(order, strategy.order(order_id))
        self.assertEqual(OrderStatus.WORKING, order.status)
        self.assertEqual(Decimal('1.00001'), order.price)
        exec_client.disconnect()


class LiveExecClientTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        print("\n")

        self.exec_client = LiveExecClient()

        context = self.exec_client.zmq_context

        self.response_list = []
        self.response_handler = self.response_list.append

        self.server1 = MockServer(
            context,
            5555,
            self.response_handler)

        self.server2 = MockServer(
            context,
            5556,
            self.response_handler)

        self.server1.start()
        self.server2.start()

    def tearDown(self):
        # Tear Down
        self.exec_client.disconnect()
        self.server1.stop()
        self.server2.stop()

    def test_can_send_submit_order_command(self):
        # Arrange
        strategy = TestStrategy1()
        self.exec_client.register_strategy(strategy)
        self.exec_client.connect()

        order_id = strategy.generate_order_id(AUDUSD_FXCM)

        order = OrderFactory.market(
            AUDUSD_FXCM,
            order_id,
            'S1_E',
            OrderSide.BUY,
            100000)

        # Act
        strategy.submit_order(order, order.id)

        # Assert
        self.assertEqual(order, strategy.order(order_id))

    def test_can_send_cancel_order_command(self):
        # Arrange
        strategy = TestStrategy1()
        self.exec_client.register_strategy(strategy)
        self.exec_client.connect()

        order_id = strategy.generate_order_id(AUDUSD_FXCM)

        order = OrderFactory.market(
            AUDUSD_FXCM,
            order_id,
            'S1_E',
            OrderSide.BUY,
            100000)

        # Act
        strategy.submit_order(order, 'some-position')
        strategy.cancel_order(order, 'ORDER_EXPIRED')

        # Assert
        self.assertEqual(order, strategy.order(order_id))

    def test_can_send_modify_order_command(self):
        # Arrange
        strategy = TestStrategy1()
        self.exec_client.register_strategy(strategy)
        self.exec_client.connect()

        order_id = strategy.generate_order_id(AUDUSD_FXCM)

        order = OrderFactory.limit(
            AUDUSD_FXCM,
            order_id,
            'S1_E',
            OrderSide.BUY,
            100000,
            Price.create(1.00000, 5),
            TimeInForce.DAY)

        # Act
        strategy.submit_order(order, 'some-position')
        strategy.modify_order(order, Price.create(1.00001, 5))

        # Assert
        self.assertEqual(order, strategy.order(order_id))

    def test_can_send_collateral_inquiry(self):
        # Arrange
        strategy = TestStrategy1()
        self.exec_client.register_strategy(strategy)
        self.exec_client.connect()

        order_id = strategy.generate_order_id(AUDUSD_FXCM)

        order = OrderFactory.limit(
            AUDUSD_FXCM,
            order_id,
            'S1_E',
            OrderSide.BUY,
            100000,
            Price.create(1.00000, 5),
            TimeInForce.DAY)

        # Act
        strategy.submit_order(order, 'some-position')
        strategy.modify_order(order, Price.create(1.00001, 5))

        # Assert
        self.assertEqual(order, strategy.order(order_id))
