#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_order.py" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest
import uuid

from decimal import Decimal

from inv_trader.model.enums import Venue, OrderSide, OrderType, OrderStatus, TimeInForce
from inv_trader.model.objects import Symbol, Price
from inv_trader.model.identifiers import GUID, Label, OrderId, ExecutionId, ExecutionTicket
from inv_trader.model.order import Order, OrderFactory
from inv_trader.model.events import OrderSubmitted, OrderAccepted, OrderRejected, OrderWorking
from inv_trader.model.events import OrderExpired, OrderModified, OrderCancelled, OrderCancelReject
from inv_trader.model.events import OrderPartiallyFilled, OrderFilled
from test_kit.stubs import TestStubs

UNIX_EPOCH = TestStubs.unix_epoch()
AUDUSD_FXCM = Symbol('AUDUSD', Venue.FXCM)
GBPUSD_FXCM = Symbol('GBPUSD', Venue.FXCM)


class OrderTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.order_factory = OrderFactory()

    def test_market_order_with_quantity_zero_raises_exception(self):
        # Arrange
        # Act
        self.assertRaises(
            ValueError,
            Order,
            AUDUSD_FXCM,
            OrderId('AUDUSD-FXCM-123456-1'),
            Label('S1_E'),
            OrderSide.BUY,
            OrderType.MARKET,
            0,
            UNIX_EPOCH)

    def test_priced_order_with_GTD_time_in_force_and_expire_time_none_raises_exception(self):
        # Arrange
        # Act
        self.assertRaises(
            ValueError,
            Order,
            AUDUSD_FXCM,
            OrderId('AUDUSD-FXCM-123456-1'),
            Label('S1_E'),
            OrderSide.BUY,
            OrderType.LIMIT,
            100000,
            UNIX_EPOCH,
            price=Price(1.00000, 5),
            time_in_force=TimeInForce.GTD,
            expire_time=None)

    def test_market_order_with_price_input_raises_exception(self):
        # Arrange
        # Act
        self.assertRaises(
            ValueError,
            Order,
            AUDUSD_FXCM,
            OrderId('AUDUSD-FXCM-123456-1'),
            Label('S1_E'),
            OrderSide.BUY,
            OrderType.MARKET,
            100000,
            UNIX_EPOCH,
            price=Price(1.00000, 5))

    def test_stop_order_with_no_price_input_raises_exception(self):
        # Arrange
        # Act
        self.assertRaises(
            ValueError,
            Order,
            AUDUSD_FXCM,
            OrderId('AUDUSD-123456-1'),
            Label('S1_E'),
            OrderSide.BUY,
            OrderType.STOP_MARKET,
            100000,
            UNIX_EPOCH)

    def test_stop_order_with_zero_price_input_raises_exception(self):
        # Arrange
        # Act
        self.assertRaises(
            ValueError,
            Order,
            AUDUSD_FXCM,
            OrderId('AUDUSD-123456-1'),
            Label('S1_E'),
            OrderSide.BUY,
            OrderType.STOP_MARKET,
            100000,
            UNIX_EPOCH,
            price=None)

    def test_limit_order_can_create_expected_decimal_price(self):
        # Arrange
        # Act
        order1 = self.order_factory.limit(
            AUDUSD_FXCM,
            OrderId('AUDUSD-FXCM-123456-1'),
            Label('S1_E'),
            OrderSide.BUY,
            100000,
            Price(1.00000, 5))

        order2 = self.order_factory.limit(
            AUDUSD_FXCM,
            OrderId('AUDUSD-FXCM-123456-1'),
            Label('S1_E'),
            OrderSide.BUY,
            100000,
            Price(1.00000, 5))

        order3 = self.order_factory.limit(
            AUDUSD_FXCM,
            OrderId('AUDUSD-FXCM-123456-1'),
            Label('S1_E'),
            OrderSide.BUY,
            100000,
            Price(1.000001, 5))

        order4 = self.order_factory.limit(
            AUDUSD_FXCM,
            OrderId('AUDUSD-FXCM-123456-1'),
            Label('S1_E'),
            OrderSide.BUY,
            100000,
            Price(1.000005, 5))

        # Assert
        self.assertEqual(Price('1.00000'), order1.price)
        self.assertEqual(Price('1.00000'), order2.price)
        self.assertEqual(Price('1.00000'), order3.price)
        self.assertEqual(Price('1.00001'), order4.price)

    def test_can_initialize_market_order(self):
        # Arrange
        # Act
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderId('AUDUSD-FXCM-123456-1'),
            Label('SCALPER-01'),
            OrderSide.BUY,
            100000)

        # Assert
        self.assertEqual(OrderType.MARKET, order.type)
        self.assertEqual(OrderStatus.INITIALIZED, order.status)
        self.assertFalse(order.is_complete)
        print(order)

    def test_can_initialize_limit_order(self):
        # Arrange
        # Act
        order = self.order_factory.limit(
            AUDUSD_FXCM,
            OrderId('AUDUSD-FXCM-123456-1'),
            Label('SCALPER-01'),
            OrderSide.BUY,
            100000,
            Price(1.00000, 5))

        # Assert
        self.assertEqual(OrderType.LIMIT, order.type)
        self.assertEqual(OrderStatus.INITIALIZED, order.status)
        self.assertEqual(TimeInForce.DAY, order.time_in_force)
        self.assertFalse(order.is_complete)
        print(order)

    def test_can_initialize_limit_order_with_expire_time(self):
        # Arrange
        # Act
        order = self.order_factory.limit(
            AUDUSD_FXCM,
            OrderId('AUDUSD-FXCM-123456-1'),
            Label('SCALPER-01'),
            OrderSide.BUY,
            100000,
            Price(1.00000, 5),
            TimeInForce.GTD,
            UNIX_EPOCH)

        # Assert
        self.assertEqual(AUDUSD_FXCM, order.symbol)
        self.assertEqual(OrderType.LIMIT, order.type)
        self.assertEqual(Price('1.00000'), order.price)
        self.assertEqual(OrderStatus.INITIALIZED, order.status)
        self.assertEqual(TimeInForce.GTD, order.time_in_force)
        self.assertEqual(UNIX_EPOCH, order.expire_time)
        self.assertFalse(order.is_complete)
        print(order)

    def test_can_initialize_stop_market_order(self):
        # Arrange
        # Act
        order = self.order_factory.stop_market(
            AUDUSD_FXCM,
            OrderId('AUDUSD-FXCM-123456-1'),
            Label('SCALPER-01'),
            OrderSide.BUY,
            100000,
            Price(1.00000, 5))

        # Assert
        self.assertEqual(OrderType.STOP_MARKET, order.type)
        self.assertEqual(OrderStatus.INITIALIZED, order.status)
        self.assertEqual(TimeInForce.DAY, order.time_in_force)
        self.assertFalse(order.is_complete)

    def test_can_initialize_stop_limit_order(self):
        # Arrange
        # Act
        order = self.order_factory.stop_limit(
            AUDUSD_FXCM,
            OrderId('AUDUSD-FXCM-123456-1'),
            Label('SCALPER-01'),
            OrderSide.BUY,
            100000,
            Price(1.00000, 5))

        # Assert
        self.assertEqual(OrderType.STOP_LIMIT, order.type)
        self.assertEqual(OrderStatus.INITIALIZED, order.status)
        self.assertFalse(order.is_complete)

    def test_can_initialize_market_if_touched_order(self):
        # Arrange
        # Act
        order = self.order_factory.market_if_touched(
            AUDUSD_FXCM,
            OrderId('AUDUSD-FXCM-123456-1'),
            Label('SCALPER-01'),
            OrderSide.BUY,
            100000,
            Price(1.00000, 5))

        # Assert
        self.assertEqual(OrderType.MIT, order.type)
        self.assertEqual(OrderStatus.INITIALIZED, order.status)
        self.assertFalse(order.is_complete)

    def test_can_initialize_fill_or_kill_order(self):
        # Arrange
        # Act
        order = self.order_factory.fill_or_kill(
            AUDUSD_FXCM,
            OrderId('AUDUSD-FXCM-123456-1'),
            Label('SCALPER-01'),
            OrderSide.BUY,
            100000)

        # Assert
        self.assertEqual(OrderType.MARKET, order.type)
        self.assertEqual(TimeInForce.FOC, order.time_in_force)
        self.assertEqual(OrderStatus.INITIALIZED, order.status)
        self.assertFalse(order.is_complete)

    def test_can_initialize_immediate_or_cancel_order(self):
        # Arrange
        # Act
        order = self.order_factory.immediate_or_cancel(
            AUDUSD_FXCM,
            OrderId('AUDUSD-FXCM-123456-1'),
            Label('SCALPER-01'),
            OrderSide.BUY,
            100000)

        # Assert
        self.assertEqual(OrderType.MARKET, order.type)
        self.assertEqual(TimeInForce.IOC, order.time_in_force)
        self.assertEqual(OrderStatus.INITIALIZED, order.status)
        self.assertFalse(order.is_complete)

    def test_can_apply_order_submitted_event_to_order(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderId('AUDUSD-FXCM-123456-1'),
            Label('SCALPER-01'),
            OrderSide.BUY,
            100000)

        event = OrderSubmitted(
            order.symbol,
            order.id,
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        order.apply(event)

        # Assert
        self.assertEqual(OrderStatus.SUBMITTED, order.status)
        self.assertEqual(1, order.event_count)
        self.assertEqual(event, order.last_event)
        self.assertFalse(order.is_complete)

    def test_can_apply_order_accepted_event_to_order(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderId('AUDUSD-FXCM-123456-1'),
            Label('SCALPER-01'),
            OrderSide.BUY,
            100000)

        event = OrderAccepted(
            order.symbol,
            order.id,
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        order.apply(event)

        # Assert
        self.assertEqual(OrderStatus.ACCEPTED, order.status)
        self.assertFalse(order.is_complete)

    def test_can_apply_order_rejected_event_to_order(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderId('AUDUSD-FXCM-123456-1'),
            Label('SCALPER-01'),
            OrderSide.BUY,
            100000)

        event = OrderRejected(
            order.symbol,
            order.id,
            UNIX_EPOCH,
            'ORDER ID INVALID',
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        order.apply(event)

        # Assert
        self.assertEqual(OrderStatus.REJECTED, order.status)
        self.assertTrue(order.is_complete)

    def test_can_apply_order_working_event_to_order(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderId('AUDUSD-FXCM-123456-1'),
            Label('SCALPER-01'),
            OrderSide.BUY,
            100000)

        event = OrderWorking(
            order.symbol,
            order.id,
            OrderId('SOME_BROKER_ID'),
            order.label,
            order.side,
            order.type,
            order.quantity,
            Price('1'),
            order.time_in_force,
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH,
            order.expire_time)

        # Act
        order.apply(event)

        # Assert
        print(order)
        self.assertEqual(OrderStatus.WORKING, order.status)
        self.assertEqual(OrderId('SOME_BROKER_ID'), order.broker_id)
        self.assertFalse(order.is_complete)

    def test_can_apply_order_expired_event_to_order(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderId('AUDUSD-FXCM-123456-1'),
            Label('SCALPER-01'),
            OrderSide.BUY,
            100000)

        event = OrderExpired(
            order.symbol,
            order.id,
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        order.apply(event)

        # Assert
        self.assertEqual(OrderStatus.EXPIRED, order.status)
        self.assertTrue(order.is_complete)

    def test_can_apply_order_cancelled_event_to_order(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderId('AUDUSD-FXCM-123456-1'),
            Label('SCALPER-01'),
            OrderSide.BUY,
            100000)

        event = OrderCancelled(
            order.symbol,
            order.id,
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        order.apply(event)

        # Assert
        self.assertEqual(OrderStatus.CANCELLED, order.status)
        self.assertTrue(order.is_complete)

    def test_can_apply_order_cancel_reject_event_to_order(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderId('AUDUSD-FXCM-123456-1'),
            Label('SCALPER-01'),
            OrderSide.BUY,
            100000)

        event = OrderCancelReject(
            order.symbol,
            order.id,
            UNIX_EPOCH,
            'REJECT_RESPONSE',
            'ORDER DOES NOT EXIST',
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        order.apply(event)

        # Assert
        self.assertEqual(OrderStatus.INITIALIZED, order.status)

    def test_can_apply_order_modified_event_to_order(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderId('AUDUSD-FXCM-123456-1'),
            Label('SCALPER-01'),
            OrderSide.BUY,
            100000)

        order_working = OrderWorking(
            order.symbol,
            order.id,
            OrderId('SOME_BROKER_ID_1'),
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

        order_modified = OrderModified(
            order.symbol,
            order.id,
            OrderId('SOME_BROKER_ID_2'),
            Price('1.00001'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        order.apply(order_working)

        # Act
        order.apply(order_modified)

        # Assert
        self.assertEqual(OrderStatus.WORKING, order.status)
        self.assertEqual(OrderId('SOME_BROKER_ID_2'), order.broker_id)
        self.assertEqual(Price('1.00001'), order.price)
        self.assertFalse(order.is_complete)

    def test_can_apply_order_filled_event_to_market_order(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderId('AUDUSD-FXCM-123456-1'),
            Label('SCALPER-01'),
            OrderSide.BUY,
            100000)

        event = OrderFilled(
            order.symbol,
            order.id,
            ExecutionId('SOME_EXEC_ID_1'),
            ExecutionTicket('SOME_EXEC_TICKET_1'),
            order.side,
            order.quantity,
            Price('1.00001'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        order.apply(event)

        # Assert
        self.assertEqual(OrderStatus.FILLED, order.status)
        self.assertEqual(100000, order.filled_quantity)
        self.assertEqual(Price('1.00001'), order.average_price)
        self.assertTrue(order.is_complete)

    def test_can_apply_order_filled_event_to_buy_limit_order(self):
        # Arrange
        order = self.order_factory.limit(
            AUDUSD_FXCM,
            OrderId('AUDUSD-FXCM-123456-1'),
            Label('SCALPER-01'),
            OrderSide.BUY,
            100000,
            Price(1.00000, 5),
            TimeInForce.DAY,
            expire_time=None)

        event = OrderFilled(
            order.symbol,
            order.id,
            ExecutionId('SOME_EXEC_ID_1'),
            ExecutionTicket('SOME_EXEC_TICKET_1'),
            order.side,
            order.quantity,
            Price('1.00001'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        order.apply(event)

        # Assert
        self.assertEqual(OrderStatus.FILLED, order.status)
        self.assertEqual(100000, order.filled_quantity)
        self.assertEqual(Price('1.00000'), order.price)
        self.assertEqual(Price('1.00001'), order.average_price)
        self.assertEqual(Decimal('0.00001'), order.slippage)
        self.assertTrue(order.is_complete)

    def test_can_apply_order_partially_filled_event_to_buy_limit_order(self):
        # Arrange
        order = self.order_factory.limit(
            AUDUSD_FXCM,
            OrderId('AUDUSD-FXCM-123456-1'),
            Label('SCALPER-01'),
            OrderSide.BUY,
            100000,
            Price(1.00000, 5),
            TimeInForce.DAY,
            expire_time=None)

        event = OrderPartiallyFilled(
            order.symbol,
            order.id,
            ExecutionId('SOME_EXEC_ID_1'),
            ExecutionTicket('SOME_EXEC_TICKET_1'),
            order.side,
            50000,
            50000,
            Price('0.99999'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        order.apply(event)

        # Assert
        self.assertEqual(OrderStatus.PARTIALLY_FILLED, order.status)
        self.assertEqual(50000, order.filled_quantity)
        self.assertEqual(Price('1.00000'), order.price)
        self.assertEqual(Price('0.99999'), order.average_price)
        self.assertEqual(Decimal('-0.00001'), order.slippage)
        self.assertFalse(order.is_complete)

    def test_can_apply_order_overfilled_event_to_buy_limit_order(self):
        # Arrange
        order = self.order_factory.limit(
            AUDUSD_FXCM,
            OrderId('AUDUSD-FXCM-123456-1'),
            Label('SCALPER-01'),
            OrderSide.BUY,
            100000,
            Price(1.00000, 5),
            TimeInForce.DAY,
            expire_time=None)

        event = OrderFilled(
            order.symbol,
            order.id,
            ExecutionId('SOME_EXEC_ID_1'),
            ExecutionTicket('SOME_EXEC_TICKET_1'),
            order.side,
            150000,
            Price('0.99999'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        order.apply(event)

        # Assert
        self.assertEqual(OrderStatus.OVER_FILLED, order.status)
        self.assertEqual(150000, order.filled_quantity)
        self.assertFalse(order.is_complete)
