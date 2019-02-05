#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_execution.py" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest

from inv_trader.model.enums import Venue, OrderSide, OrderStatus, TimeInForce
from inv_trader.model.identifiers import Label, PositionId
from inv_trader.model.objects import Symbol, Price
from inv_trader.model.order import OrderFactory
from inv_trader.execution import LiveExecClient
from test_kit.stubs import TestStubs
from test_kit.mocks import MockExecClient, MockServer
from test_kit.strategies import TestStrategy1

UNIX_EPOCH = TestStubs.unix_epoch()
AUDUSD_FXCM = Symbol('AUDUSD', Venue.FXCM)
GBPUSD_FXCM = Symbol('GBPUSD', Venue.FXCM)

UTF8 = 'utf8'
LOCAL_HOST = "127.0.0.1"


class ExecutionClientTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.bar_type = TestStubs.bartype_gbpusd_1min_bid()
        self.strategy = TestStrategy1(self.bar_type)
        self.exec_client = MockExecClient()
        self.exec_client.connect()

    def tearDown(self):
        # Tear Down
        self.exec_client.disconnect()

    def test_can_register_strategy(self):
        # Arrange
        self.exec_client.register_strategy(self.strategy)

        # Act
        result = self.strategy._exec_client

        # Assert
        self.assertEqual(self.exec_client, result)

    def test_can_send_submit_order_command_to_mock_exec_client(self):
        # Arrange
        self.exec_client.register_strategy(self.strategy)

        order = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            Label('S1_E'),
            OrderSide.BUY,
            100000)

        order_id = order.id

        # Act
        self.strategy.submit_order(order, PositionId(str(order.id)))

        # Assert
        self.assertEqual(order, self.strategy.order(order_id))
        self.assertEqual(4, order.status)  # OrderStatus.WORKING

    def test_can_send_cancel_order_command_to_mock_exec_clint(self):
        # Arrange
        self.exec_client.register_strategy(self.strategy)
        self.exec_client.connect()

        order = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            Label('S1_E'),
            OrderSide.BUY,
            100000)

        order_id = order.id

        # Act
        self.strategy.submit_order(order, PositionId(str(order_id)))
        self.strategy.cancel_order(order, 'ORDER_EXPIRED')

        # Assert
        self.assertEqual(order, self.strategy.order(order_id))
        self.assertEqual(OrderStatus.CANCELLED, order.status)

    def test_can_send_modify_order_command_to_mock_exec_client(self):
        # Arrange
        self.exec_client.register_strategy(self.strategy)
        self.exec_client.connect()

        order = self.strategy.order_factory.limit(
            AUDUSD_FXCM,
            Label('S1_E'),
            OrderSide.BUY,
            100000,
            Price('1.00000'),
            TimeInForce.DAY,
            expire_time=None)

        order_id = order.id

        # Act
        self.strategy.submit_order(order, PositionId(str(order.id)))
        self.strategy.modify_order(order, Price('1.00001'))

        # Assert
        self.assertEqual(order, self.strategy.order(order_id))
        self.assertEqual(OrderStatus.WORKING, order.status)
        self.assertEqual(Price('1.00001'), order.price)


class LiveExecClientTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        print("\n")

        self.bar_type = TestStubs.bartype_audusd_1min_bid()
        self.strategy = TestStrategy1(bar_type=self.bar_type)
        self.exec_client = LiveExecClient()
        self.exec_client.register_strategy(self.strategy)

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
        self.exec_client.connect()

    def tearDown(self):
        # Tear Down
        self.exec_client.disconnect()
        self.server1.stop()
        self.server2.stop()

    def test_can_send_submit_order_command(self):
        # Arrange
        order = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            Label('S1_E'),
            OrderSide.BUY,
            100000)

        order_id = order.id

        # Act
        self.strategy.submit_order(order, PositionId(str(order.id)))

        # Assert
        self.assertEqual(order, self.strategy.order(order_id))
        self.assertEqual(1, len(self.response_list))

    def test_can_send_cancel_order_command(self):
        # Arrange
        order = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            Label('S1_E'),
            OrderSide.BUY,
            100000)

        order_id = order.id

        # Act
        self.strategy.submit_order(order, PositionId('some-position'))
        self.strategy.cancel_order(order, 'ORDER_EXPIRED')

        # Assert
        self.assertEqual(order, self.strategy.order(order_id))
        self.assertEqual(2, len(self.response_list))

    def test_can_send_modify_order_command(self):
        # Arrange
        order = self.strategy.order_factory.limit(
            AUDUSD_FXCM,
            Label('S1_E'),
            OrderSide.BUY,
            100000,
            Price('1.00000'),
            TimeInForce.DAY,
            expire_time=None)

        order_id = order.id

        # Act
        self.strategy.submit_order(order, PositionId('some-position'))
        self.strategy.modify_order(order, Price('1.00001'))

        # Assert
        self.assertEqual(order, self.strategy.order(order_id))
        self.assertEqual(2, len(self.response_list))

    def test_can_send_collateral_inquiry(self):
        # Arrange
        # Act
        self.strategy.collateral_inquiry()

        # Assert
        self.assertEqual(1, len(self.response_list))
