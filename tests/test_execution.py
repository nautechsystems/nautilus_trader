#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_execution.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest
import pytz
import time

from datetime import datetime
from decimal import Decimal

from inv_trader.model.enums import Venue, OrderSide, OrderType, OrderStatus, TimeInForce
from inv_trader.model.objects import Symbol, Resolution, QuoteType, BarType, Bar
from inv_trader.model.events import OrderSubmitted, OrderAccepted, OrderRejected, OrderWorking
from inv_trader.model.events import OrderExpired, OrderModified, OrderCancelled, OrderCancelReject
from inv_trader.model.events import OrderPartiallyFilled, OrderFilled
from inv_trader.execution import ExecutionClient, LiveExecClient
from inv_trader.factories import OrderFactory, OrderIdGenerator
from test_kit.stubs import TestStubs
from test_kit.mocks import MockExecClient
from test_kit.objects import ObjectStorer
from test_kit.strategies import TestStrategy1

UNIX_EPOCH = TestStubs.unix_epoch()
AUDUSD_FXCM = Symbol('AUDUSD', Venue.FXCM)
GBPUSD_FXCM = Symbol('GBPUSD', Venue.FXCM)


class ExecutionClientTests(unittest.TestCase):

    def test_can_register_strategy(self):
        # Arrange
        storer = ObjectStorer()
        strategy = TestStrategy1(storer)
        exec_client = ExecutionClient()
        exec_client.register_strategy(strategy)

        # Act
        result = strategy._exec_client

        # Assert
        self.assertEqual(exec_client, result)

    def test_can_receive_bars(self):
        # Arrange
        storer = ObjectStorer()
        strategy = TestStrategy1(storer)
        exec_client = MockExecClient()
        exec_client.register_strategy(strategy)

        strategy.start()

        bar_type = BarType(GBPUSD_FXCM,
                           1,
                           Resolution.SECOND,
                           QuoteType.MID)

        bar1 = Bar(
            Decimal('1.00001'),
            Decimal('1.00004'),
            Decimal('1.00003'),
            Decimal('1.00002'),
            100000,
            datetime(1970, 1, 1, 00, 00, 0, 0, pytz.UTC))

        bar2 = Bar(
            Decimal('1.00011'),
            Decimal('1.00014'),
            Decimal('1.00013'),
            Decimal('1.00012'),
            100000,
            datetime(1970, 1, 1, 00, 00, 1, 0, pytz.UTC))

        # Act
        strategy._update_bars(bar_type, bar1)
        strategy._update_bars(bar_type, bar2)
        result = storer.get_store[-1]

        # Assert
        self.assertTrue(isinstance(result, OrderWorking))

    def test_can_send_submit_order_command_to_mock_exec_client(self):
        # Arrange
        storer = ObjectStorer()
        strategy = TestStrategy1(storer)
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
        time.sleep(1)
        strategy.submit_order(order)

        # Assert
        self.assertEqual(order, strategy.order(order_id))
        self.assertEqual(OrderStatus.WORKING, order.status)
        exec_client.disconnect()

    def test_can_send_cancel_order_command_to_mock_exec_clint(self):
        # Arrange
        storer = ObjectStorer()
        strategy = TestStrategy1(storer)
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
        time.sleep(1)
        strategy.submit_order(order)
        strategy.cancel_order(order, 'ORDER_EXPIRED')

        # Assert
        self.assertEqual(order, strategy.order(order_id))
        self.assertEqual(OrderStatus.CANCELLED, order.status)
        exec_client.disconnect()

    def test_can_send_modify_order_command_to_mock_exec_client(self):
        # Arrange
        storer = ObjectStorer()
        strategy = TestStrategy1(storer)
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
            1.00000,
            5,
            TimeInForce.DAY)

        # Act
        time.sleep(1)
        strategy.submit_order(order)
        strategy.modify_order(order, Decimal('1.00001'))

        # Assert
        self.assertEqual(order, strategy.order(order_id))
        self.assertEqual(OrderStatus.WORKING, order.status)
        self.assertEqual(Decimal('1.00001'), order.price)
        exec_client.disconnect()


class LiveExecClientTests(unittest.TestCase):

    def test_can_send_submit_order_command(self):
        # Arrange
        storer = ObjectStorer()
        strategy = TestStrategy1(storer)
        exec_client = LiveExecClient()
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
        time.sleep(1)
        strategy.submit_order(order)

        # Assert
        self.assertEqual(order, strategy.order(order_id))
        self.assertEqual(OrderStatus.INITIALIZED, order.status)
        exec_client.disconnect()

    def test_can_send_cancel_order_command(self):
        # Arrange
        storer = ObjectStorer()
        strategy = TestStrategy1(storer)
        exec_client = LiveExecClient()
        exec_client.register_strategy(strategy)
        exec_client.connect()

        order_id_generator = OrderIdGenerator(order_id_tag='')
        order_id = order_id_generator.generate(AUDUSD_FXCM)

        order = OrderFactory.market(
            AUDUSD_FXCM,
            order_id,
            'S1_E',
            OrderSide.BUY,
            100000)

        # Act
        time.sleep(1)
        strategy.submit_order(order)
        strategy.cancel_order(order, 'ORDER_EXPIRED')

        # Assert
        self.assertEqual(order, strategy.order(order_id))
        self.assertEqual(OrderStatus.INITIALIZED, order.status)
        exec_client.disconnect()

    def test_can_send_modify_order_command(self):
        # Arrange
        storer = ObjectStorer()
        strategy = TestStrategy1(storer)
        exec_client = LiveExecClient()
        exec_client.register_strategy(strategy)
        exec_client.connect()

        order_id = strategy.generate_order_id(AUDUSD_FXCM)

        order = OrderFactory.limit(
            AUDUSD_FXCM,
            order_id,
            'S1_E',
            OrderSide.BUY,
            100000,
            1.00000,
            5,
            TimeInForce.DAY)

        # Act
        time.sleep(1)
        strategy.submit_order(order)
        strategy.modify_order(order, Decimal('1.00001'))

        # Assert
        self.assertEqual(order, strategy.order(order_id))
        self.assertEqual(OrderStatus.INITIALIZED, order.status)
        exec_client.disconnect()
