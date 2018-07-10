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

from inv_trader.model.enums import Venue, OrderSide, OrderType, OrderStatus, TimeInForce
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
        self.assertEqual(TimeInForce.DAY, order.time_in_force)
        self.assertFalse(order.is_complete)

    def test_can_initialize_limit_order_with_expire_time(self):
        # Arrange
        # Act
        order = OrderFactory.limit(
            AUDUSD_FXCM,
            'AUDUSD|123456|1',
            'SCALPER-01',
            OrderSide.BUY,
            100000,
            Decimal('1.00000'),
            TimeInForce.GTD,
            UNIX_EPOCH)

        # Assert
        self.assertEqual(OrderType.LIMIT, order.type)
        self.assertEqual(OrderStatus.INITIALIZED, order.status)
        self.assertEqual(TimeInForce.GTD, order.time_in_force)
        self.assertEqual(UNIX_EPOCH, order.expire_time)
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

    def test_can_apply_order_cancelled_event_to_order(self):
        # Arrange
        order = OrderFactory.market(
            AUDUSD_FXCM,
            'AUDUSD|123456|1',
            'SCALPER-01',
            OrderSide.BUY,
            100000)

        event = OrderCancelled(
            order.symbol,
            order.id,
            UNIX_EPOCH,
            uuid.uuid4(),
            UNIX_EPOCH)

        # Act
        order.apply(event)

        # Assert
        self.assertEqual(OrderStatus.CANCELLED, order.status)
        self.assertTrue(order.is_complete)

    def test_can_apply_order_cancel_reject_event_to_order(self):
        # Arrange
        order = OrderFactory.market(
            AUDUSD_FXCM,
            'AUDUSD|123456|1',
            'SCALPER-01',
            OrderSide.BUY,
            100000)

        event = OrderCancelReject(
            order.symbol,
            order.id,
            UNIX_EPOCH,
            'ORDER DOES NOT EXIST',
            uuid.uuid4(),
            UNIX_EPOCH)

        # Act
        order.apply(event)

        # Assert
        self.assertEqual(OrderStatus.INITIALIZED, order.status)

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

    def test_can_apply_order_filled_event_to_market_order(self):
        # Arrange
        order = OrderFactory.market(
            AUDUSD_FXCM,
            'AUDUSD|123456|1',
            'SCALPER-01',
            OrderSide.BUY,
            100000)

        event = OrderFilled(
            order.symbol,
            order.id,
            'SOME_EXEC_ID_1',
            'SOME_EXEC_TICKET_1',
            order.side,
            order.quantity,
            Decimal('1.00001'),
            UNIX_EPOCH,
            uuid.uuid4(),
            UNIX_EPOCH)

        # Act
        order.apply(event)

        # Assert
        self.assertEqual(OrderStatus.FILLED, order.status)
        self.assertEqual(100000, order.filled_quantity)
        self.assertEqual(Decimal('1.00001'), order.average_price)
        self.assertTrue(order.is_complete)

    def test_can_apply_order_filled_event_to_buy_limit_order(self):
        # Arrange
        order = OrderFactory.limit(
            AUDUSD_FXCM,
            'AUDUSD|123456|1',
            'SCALPER-01',
            OrderSide.BUY,
            100000,
            Decimal('1.00000'))

        event = OrderFilled(
            order.symbol,
            order.id,
            'SOME_EXEC_ID_1',
            'SOME_EXEC_TICKET_1',
            order.side,
            order.quantity,
            Decimal('1.00001'),
            UNIX_EPOCH,
            uuid.uuid4(),
            UNIX_EPOCH)

        # Act
        order.apply(event)

        # Assert
        self.assertEqual(OrderStatus.FILLED, order.status)
        self.assertEqual(100000, order.filled_quantity)
        self.assertEqual(Decimal('1.00000'), order.price)
        self.assertEqual(Decimal('1.00001'), order.average_price)
        self.assertEqual(Decimal('0.00001'), order.slippage)
        self.assertTrue(order.is_complete)

    def test_can_apply_order_partially_filled_event_to_buy_limit_order(self):
        # Arrange
        order = OrderFactory.limit(
            AUDUSD_FXCM,
            'AUDUSD|123456|1',
            'SCALPER-01',
            OrderSide.BUY,
            100000,
            Decimal('1.00000'))

        event = OrderPartiallyFilled(
            order.symbol,
            order.id,
            'SOME_EXEC_ID_1',
            'SOME_EXEC_TICKET_1',
            order.side,
            50000,
            50000,
            Decimal('0.99999'),
            UNIX_EPOCH,
            uuid.uuid4(),
            UNIX_EPOCH)

        # Act
        order.apply(event)

        # Assert
        self.assertEqual(OrderStatus.PARTIALLY_FILLED, order.status)
        self.assertEqual(50000, order.filled_quantity)
        self.assertEqual(Decimal('1.00000'), order.price)
        self.assertEqual(Decimal('0.99999'), order.average_price)
        self.assertEqual(Decimal('-0.00001'), order.slippage)
        self.assertFalse(order.is_complete)
