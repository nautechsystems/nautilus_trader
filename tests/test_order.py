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
from inv_trader.model.events import OrderSubmitted, OrderAccepted, OrderRejected, OrderWorking
from inv_trader.model.events import OrderExpired, OrderModified, OrderCancelled, OrderCancelReject
from inv_trader.model.events import OrderPartiallyFilled, OrderFilled
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

        event = OrderSubmitted(
            order.symbol,
            order.id,
            UNIX_EPOCH,
            uuid.uuid4(),
            UNIX_EPOCH)

        # Act
        order.apply(event)

        # Assert
        self.assertEqual(OrderStatus.SUBMITTED, order.status)
        self.assertEqual(1, order.event_count)
        self.assertEqual(event, order.events[0])
        self.assertFalse(order.is_complete)

    def test_can_apply_order_accepted_event_to_order(self):
        # Arrange
        order = OrderFactory.market(
            AUDUSD_FXCM,
            'AUDUSD|123456|1',
            'SCALPER-01',
            OrderSide.BUY,
            100000)

        event = OrderAccepted(
            order.symbol,
            order.id,
            UNIX_EPOCH,
            uuid.uuid4(),
            UNIX_EPOCH)

        # Act
        order.apply(event)

        # Assert
        self.assertEqual(OrderStatus.ACCEPTED, order.status)
        self.assertFalse(order.is_complete)

    def test_can_apply_order_rejected_event_to_order(self):
        # Arrange
        order = OrderFactory.market(
            AUDUSD_FXCM,
            'AUDUSD|123456|1',
            'SCALPER-01',
            OrderSide.BUY,
            100000)

        event = OrderRejected(
            order.symbol,
            order.id,
            UNIX_EPOCH,
            'ORDER ID INVALID',
            uuid.uuid4(),
            UNIX_EPOCH)

        # Act
        order.apply(event)

        # Assert
        self.assertEqual(OrderStatus.REJECTED, order.status)
        self.assertTrue(order.is_complete)

    def test_can_apply_order_working_event_to_order(self):
        # Arrange
        order = OrderFactory.market(
            AUDUSD_FXCM,
            'AUDUSD|123456|1',
            'SCALPER-01',
            OrderSide.BUY,
            100000)

        event = OrderWorking(
            order.symbol,
            order.id,
            'SOME_BROKER_ID',
            UNIX_EPOCH,
            uuid.uuid4(),
            UNIX_EPOCH)

        # Act
        order.apply(event)

        # Assert
        self.assertEqual(OrderStatus.WORKING, order.status)
        self.assertEqual('SOME_BROKER_ID', order.broker_id)
        self.assertFalse(order.is_complete)

    def test_can_apply_order_expired_event_to_order(self):
        # Arrange
        order = OrderFactory.market(
            AUDUSD_FXCM,
            'AUDUSD|123456|1',
            'SCALPER-01',
            OrderSide.BUY,
            100000)

        event = OrderExpired(
            order.symbol,
            order.id,
            UNIX_EPOCH,
            uuid.uuid4(),
            UNIX_EPOCH)

        # Act
        order.apply(event)

        # Assert
        self.assertEqual(OrderStatus.EXPIRED, order.status)
        self.assertTrue(order.is_complete)

    def test_can_apply_order_modified_event_to_order(self):
        # Arrange
        order = OrderFactory.market(
            AUDUSD_FXCM,
            'AUDUSD|123456|1',
            'SCALPER-01',
            OrderSide.BUY,
            100000)

        order_working = OrderWorking(
            order.symbol,
            order.id,
            'SOME_BROKER_ID_1',
            UNIX_EPOCH,
            uuid.uuid4(),
            UNIX_EPOCH)

        order_modified = OrderModified(
            order.symbol,
            order.id,
            'SOME_BROKER_ID_2',
            Decimal('1.00001'),
            UNIX_EPOCH,
            uuid.uuid4(),
            UNIX_EPOCH)

        order.apply(order_working)

        # Act
        order.apply(order_modified)

        # Assert
        self.assertEqual(OrderStatus.WORKING, order.status)
        self.assertEqual('SOME_BROKER_ID_2', order.broker_id)
        self.assertEqual(Decimal('1.00001'), order.price)
        self.assertFalse(order.is_complete)

