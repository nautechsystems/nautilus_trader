#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_position.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest
import uuid

from decimal import Decimal

from inv_trader.model.enums import Venue, OrderSide, MarketPosition
from inv_trader.model.objects import Symbol
from inv_trader.model.position import Position
from inv_trader.model.events import OrderPartiallyFilled, OrderFilled
from inv_trader.factories import OrderFactory
from test_kit.stubs import TestStubs

UNIX_EPOCH = TestStubs.unix_epoch()
AUDUSD_FXCM = Symbol('audusd', Venue.FXCM)
GBPUSD_FXCM = Symbol('gbpusd', Venue.FXCM)


class PositionTests(unittest.TestCase):

    def test_initialized_position_returns_expected_attributes(self):
        # Arrange
        order = OrderFactory.market(
            AUDUSD_FXCM,
            'AUDUSD|123456|1',
            'SCALPER-01',
            OrderSide.BUY,
            100000)

        # Act
        position = Position(
            order.symbol,
            order.id,
            'P123456',
            UNIX_EPOCH)

        # Assert
        self.assertEqual(0, position.quantity)
        self.assertEqual(MarketPosition.FLAT, position.market_position)
        self.assertEqual(0, position.event_count)
        self.assertEqual(0, len(position.execution_ids))
        self.assertEqual(0, len(position.execution_tickets))

    def test_position_filled_with_buy_order_returns_expected_attributes(self):
        # Arrange
        order = OrderFactory.market(
            AUDUSD_FXCM,
            'AUDUSD|123456|1',
            'SCALPER-01',
            OrderSide.BUY,
            100000)

        position = Position(
            order.symbol,
            order.id,
            'P123456',
            UNIX_EPOCH)

        order_filled = OrderFilled(
            order.symbol,
            order.id,
            'E123456',
            'T123456',
            order.side,
            order.quantity,
            Decimal('1.00001'),
            UNIX_EPOCH,
            uuid.uuid4(),
            UNIX_EPOCH)

        # Act
        position.apply(order_filled)

        # Assert
        self.assertEqual(100000, position.quantity)
        self.assertEqual(MarketPosition.LONG, position.market_position)
        self.assertEqual(UNIX_EPOCH, position.entry_time)
        self.assertEqual(Decimal('1.00001'), position.average_entry_price)
        self.assertEqual(1, position.event_count)
        self.assertEqual(1, len(position.execution_ids))
        self.assertEqual(1, len(position.execution_tickets))

    def test_position_filled_with_sell_order_returns_expected_attributes(self):
        # Arrange
        order = OrderFactory.market(
            AUDUSD_FXCM,
            'AUDUSD|123456|1',
            'SCALPER-01',
            OrderSide.SELL,
            100000)

        position = Position(
            order.symbol,
            order.id,
            'P123456',
            UNIX_EPOCH)

        order_filled = OrderFilled(
            order.symbol,
            order.id,
            'E123456',
            'T123456',
            order.side,
            order.quantity,
            Decimal('1.00001'),
            UNIX_EPOCH,
            uuid.uuid4(),
            UNIX_EPOCH)

        # Act
        position.apply(order_filled)

        # Assert
        self.assertEqual(100000, position.quantity)
        self.assertEqual(MarketPosition.SHORT, position.market_position)
        self.assertEqual(UNIX_EPOCH, position.entry_time)
        self.assertEqual(Decimal('1.00001'), position.average_entry_price)
        self.assertEqual(1, position.event_count)
        self.assertEqual(1, len(position.execution_ids))
        self.assertEqual(1, len(position.execution_tickets))

    def test_position_partial_fills_with_buy_order_returns_expected_attributes(self):
        # Arrange
        order = OrderFactory.market(
            AUDUSD_FXCM,
            'AUDUSD|123456|1',
            'SCALPER-01',
            OrderSide.BUY,
            100000)

        position = Position(
            order.symbol,
            order.id,
            'P123456',
            UNIX_EPOCH)

        order_partially_filled = OrderPartiallyFilled(
            order.symbol,
            order.id,
            'E123456',
            'T123456',
            order.side,
            50000,
            50000,
            Decimal('1.00001'),
            UNIX_EPOCH,
            uuid.uuid4(),
            UNIX_EPOCH)

        # Act
        position.apply(order_partially_filled)
        position.apply(order_partially_filled)

        # Assert
        self.assertEqual(100000, position.quantity)
        self.assertEqual(MarketPosition.LONG, position.market_position)
        self.assertEqual(UNIX_EPOCH, position.entry_time)
        self.assertEqual(Decimal('1.00001'), position.average_entry_price)
        self.assertEqual(2, position.event_count)
        self.assertEqual(2, len(position.execution_ids))
        self.assertEqual(2, len(position.execution_tickets))

    def test_position_partial_fills_with_sell_order_returns_expected_attributes(self):
        # Arrange
        order = OrderFactory.market(
            AUDUSD_FXCM,
            'AUDUSD|123456|1',
            'SCALPER-01',
            OrderSide.SELL,
            100000)

        position = Position(
            order.symbol,
            order.id,
            'P123456',
            UNIX_EPOCH)

        order_partially_filled = OrderPartiallyFilled(
            order.symbol,
            order.id,
            'E123456',
            'T123456',
            order.side,
            50000,
            50000,
            Decimal('1.00001'),
            UNIX_EPOCH,
            uuid.uuid4(),
            UNIX_EPOCH)

        # Act
        position.apply(order_partially_filled)
        position.apply(order_partially_filled)

        # Assert
        self.assertEqual(100000, position.quantity)
        self.assertEqual(MarketPosition.SHORT, position.market_position)
        self.assertEqual(UNIX_EPOCH, position.entry_time)
        self.assertEqual(Decimal('1.00001'), position.average_entry_price)
        self.assertEqual(2, position.event_count)
        self.assertEqual(2, len(position.execution_ids))
        self.assertEqual(2, len(position.execution_tickets))

    def test_position_filled_with_buy_order_then_sell_order_returns_expected_attributes(self):
        # Arrange
        order = OrderFactory.market(
            AUDUSD_FXCM,
            'AUDUSD|123456|1',
            'SCALPER-01',
            OrderSide.BUY,
            100000)

        position = Position(
            order.symbol,
            order.id,
            'P123456',
            UNIX_EPOCH)

        order_filled1 = OrderFilled(
            order.symbol,
            order.id,
            'E123456',
            'T123456',
            OrderSide.BUY,
            order.quantity,
            Decimal('1.00001'),
            UNIX_EPOCH,
            uuid.uuid4(),
            UNIX_EPOCH)

        order_filled2 = OrderFilled(
            order.symbol,
            order.id,
            'E123456',
            'T123456',
            OrderSide.SELL,
            order.quantity,
            Decimal('1.00001'),
            UNIX_EPOCH,
            uuid.uuid4(),
            UNIX_EPOCH)

        # Act
        position.apply(order_filled1)
        position.apply(order_filled2)

        # Assert
        self.assertEqual(0, position.quantity)
        self.assertEqual(MarketPosition.FLAT, position.market_position)
        self.assertEqual(UNIX_EPOCH, position.entry_time)
        self.assertEqual(Decimal('1.00001'), position.average_entry_price)
        self.assertEqual(2, position.event_count)
        self.assertEqual(2, len(position.execution_ids))
        self.assertEqual(2, len(position.execution_tickets))
        self.assertEqual(UNIX_EPOCH, position.exit_time)
        self.assertEqual(Decimal('1.00001'), position.average_exit_price)

    def test_position_filled_with_sell_order_then_buy_order_returns_expected_attributes(self):
        # Arrange
        order = OrderFactory.market(
            AUDUSD_FXCM,
            'AUDUSD|123456|1',
            'SCALPER-01',
            OrderSide.SELL,
            100000)

        position = Position(
            order.symbol,
            order.id,
            'P123456',
            UNIX_EPOCH)

        order_filled1 = OrderFilled(
            order.symbol,
            order.id,
            'E123456',
            'T123456',
            OrderSide.SELL,
            order.quantity,
            Decimal('1.00001'),
            UNIX_EPOCH,
            uuid.uuid4(),
            UNIX_EPOCH)

        order_filled2 = OrderFilled(
            order.symbol,
            order.id,
            'E123456',
            'T123456',
            OrderSide.BUY,
            order.quantity,
            Decimal('1.00001'),
            UNIX_EPOCH,
            uuid.uuid4(),
            UNIX_EPOCH)

        # Act
        position.apply(order_filled1)
        position.apply(order_filled2)

        # Assert
        self.assertEqual(0, position.quantity)
        self.assertEqual(MarketPosition.FLAT, position.market_position)
        self.assertEqual(UNIX_EPOCH, position.entry_time)
        self.assertEqual(Decimal('1.00001'), position.average_entry_price)
        self.assertEqual(2, position.event_count)
        self.assertEqual(2, len(position.execution_ids))
        self.assertEqual(2, len(position.execution_tickets))
        self.assertEqual(UNIX_EPOCH, position.exit_time)
        self.assertEqual(Decimal('1.00001'), position.average_exit_price)
