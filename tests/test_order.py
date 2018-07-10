#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_order.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest
import uuid

from decimal import Decimal

from inv_trader.model.enums import Venue, OrderSide, OrderType, OrderStatus
from inv_trader.model.objects import Symbol
from inv_trader.model.events import OrderSubmitted
from inv_trader.factories import OrderFactory
from test_kit.constants import TestConstants

UNIX_EPOCH = TestConstants.unix_epoch()
AUDUSD_FXCM = Symbol('audusd', Venue.FXCM)
GBPUSD_FXCM = Symbol('gbpusd', Venue.FXCM)


class OrderTests(unittest.TestCase):

    def test_can_initialize_market_order(self):
        # Arrange
        # Act
        order = OrderFactory.market(
            AUDUSD_FXCM,
            'AUDUSD|123456|1',
            'SCALPER-01',
            OrderSide.BUY,
            100000)

        # Assert
        self.assertEqual(OrderType.MARKET, order.type)
        self.assertEqual(OrderStatus.INITIALIZED, order.status)
        self.assertFalse(order.is_complete)

    def test_can_initialize_limit_order(self):
        # Arrange
        # Act
        order = OrderFactory.limit(
            AUDUSD_FXCM,
            'AUDUSD|123456|1',
            'SCALPER-01',
            OrderSide.BUY,
            100000,
            Decimal('1.00000'))

        # Assert
        self.assertEqual(OrderType.LIMIT, order.type)
        self.assertEqual(OrderStatus.INITIALIZED, order.status)
        self.assertFalse(order.is_complete)

    def test_can_initialize_stop_market_order(self):
        # Arrange
        # Act
        order = OrderFactory.stop_market(
            AUDUSD_FXCM,
            'AUDUSD|123456|1',
            'SCALPER-01',
            OrderSide.BUY,
            100000,
            Decimal('1.00000'))

        # Assert
        self.assertEqual(OrderType.STOP_MARKET, order.type)
        self.assertEqual(OrderStatus.INITIALIZED, order.status)
        self.assertFalse(order.is_complete)

    def test_can_initialize_stop_limit_order(self):
        # Arrange
        # Act
        order = OrderFactory.stop_limit(
            AUDUSD_FXCM,
            'AUDUSD|123456|1',
            'SCALPER-01',
            OrderSide.BUY,
            100000,
            Decimal('1.00000'))

        # Assert
        self.assertEqual(OrderType.STOP_LIMIT, order.type)
        self.assertEqual(OrderStatus.INITIALIZED, order.status)
        self.assertFalse(order.is_complete)

    def test_can_initialize_market_if_touched_order(self):
        # Arrange
        # Act
        order = OrderFactory.market_if_touched(
            AUDUSD_FXCM,
            'AUDUSD|123456|1',
            'SCALPER-01',
            OrderSide.BUY,
            100000,
            Decimal('1.00000'))

        # Assert
        self.assertEqual(OrderType.MIT, order.type)
        self.assertEqual(OrderStatus.INITIALIZED, order.status)
        self.assertFalse(order.is_complete)

    def test_can_initialize_fill_or_kill_order(self):
        # Arrange
        # Act
        order = OrderFactory.fill_or_kill(
            AUDUSD_FXCM,
            'AUDUSD|123456|1',
            'SCALPER-01',
            OrderSide.BUY,
            100000)

        # Assert
        self.assertEqual(OrderType.FOC, order.type)
        self.assertEqual(OrderStatus.INITIALIZED, order.status)
        self.assertFalse(order.is_complete)

    def test_can_initialize_immediate_or_cancel_order(self):
        # Arrange
        # Act
        order = OrderFactory.immediate_or_cancel(
            AUDUSD_FXCM,
            'AUDUSD|123456|1',
            'SCALPER-01',
            OrderSide.BUY,
            100000)

        # Assert
        self.assertEqual(OrderType.IOC, order.type)
        self.assertEqual(OrderStatus.INITIALIZED, order.status)
        self.assertFalse(order.is_complete)

    def test_can_apply_order_submitted_event_to_order(self):
        # Arrange
        order = OrderFactory.market(
            AUDUSD_FXCM,
            'AUDUSD|123456|1',
            'SCALPER-01',
            OrderSide.BUY,
            100000)

        event = OrderSubmitted(order.symbol, order.id, UNIX_EPOCH, uuid.uuid4(), UNIX_EPOCH)

        # Act
        order.apply(event)

        # Assert
        self.assertEqual(OrderStatus.SUBMITTED, order.status)

