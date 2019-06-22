#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_execution.py" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest
import time

from inv_trader.model.enums import Venue, OrderSide
from inv_trader.model.objects import Quantity, Symbol, Price
from inv_trader.network.execution import LiveExecClient
from test_kit.stubs import TestStubs
from test_kit.mocks import MockServer
from test_kit.strategies import TestStrategy1

UNIX_EPOCH = TestStubs.unix_epoch()
AUDUSD_FXCM = Symbol('AUDUSD', Venue.FXCM)
GBPUSD_FXCM = Symbol('GBPUSD', Venue.FXCM)

UTF8 = 'utf8'
LOCAL_HOST = "127.0.0.1"


class LiveExecClientTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.bar_type = TestStubs.bartype_audusd_1min_bid()
        self.exec_client = LiveExecClient()

        context = self.exec_client.zmq_context

        self.response_list = []
        self.response_handler = self.response_list.append

        self.server1 = MockServer(context, 5555, self.response_handler)
        self.server2 = MockServer(context, 5556, self.response_handler)

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
        strategy = TestStrategy1(self.bar_type, id_tag_strategy='001')
        self.exec_client.register_strategy(strategy)
        order = strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        # Act
        strategy.submit_order(order, strategy.position_id_generator.generate())

        time.sleep(0.1)
        # Assert
        self.assertEqual(order, strategy.order(order.id))
        self.assertEqual(1, len(self.response_list))

    def test_can_send_submit_atomic_order_no_take_profit_command(self):
        # Arrange
        strategy = TestStrategy1(self.bar_type, id_tag_strategy='002')
        self.exec_client.register_strategy(strategy)
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
        self.assertEqual(1, len(self.response_list))

    def test_can_send_submit_atomic_order_with_take_profit_command(self):
        # Arrange
        strategy = TestStrategy1(self.bar_type, id_tag_strategy='003')
        self.exec_client.register_strategy(strategy)
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
        self.assertEqual(1, len(self.response_list))

    def test_can_send_cancel_order_command(self):
        # Arrange
        strategy = TestStrategy1(self.bar_type, id_tag_strategy='004')
        self.exec_client.register_strategy(strategy)
        order = strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        # Act
        strategy.submit_order(order, strategy.position_id_generator.generate())
        time.sleep(1)
        strategy.cancel_order(order, 'ORDER_EXPIRED')

        # Assert
        time.sleep(1)
        self.assertEqual(order, strategy.order(order.id))
        self.assertEqual(2, len(self.response_list))

    def test_can_send_modify_order_command(self):
        # Arrange
        strategy = TestStrategy1(self.bar_type, id_tag_strategy='005')
        self.exec_client.register_strategy(strategy)
        order = strategy.order_factory.limit(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price('1.00000'))

        # Act
        strategy.submit_order(order, strategy.position_id_generator.generate())
        time.sleep(1)
        strategy.modify_order(order, Price('1.00001'))

        # Assert
        time.sleep(1)
        self.assertEqual(order, strategy.order(order.id))
        self.assertEqual(2, len(self.response_list))

    def test_can_send_collateral_inquiry(self):
        # Arrange
        strategy = TestStrategy1(self.bar_type, id_tag_strategy='006')
        self.exec_client.register_strategy(strategy)

        # Act
        strategy.collateral_inquiry()

        # Assert
        time.sleep(1)
        self.assertEqual(1, len(self.response_list))
