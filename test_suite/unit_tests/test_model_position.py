# -------------------------------------------------------------------------------------------------
# <copyright file="test_model_position.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest
import uuid

from decimal import Decimal

from nautilus_trader.core.types import GUID
from nautilus_trader.common.clock import TestClock
from nautilus_trader.model.enums import OrderSide, MarketPosition
from nautilus_trader.model.objects import Quantity, Venue, Symbol, Price
from nautilus_trader.model.identifiers import IdTag, OrderId, PositionId, ExecutionId, ExecutionTicket
from nautilus_trader.model.order import OrderFactory
from nautilus_trader.model.position import Position
from nautilus_trader.model.events import OrderPartiallyFilled, OrderFilled
from test_kit.stubs import TestStubs

UNIX_EPOCH = TestStubs.unix_epoch()
AUDUSD_FXCM = Symbol('AUDUSD', Venue('FXCM'))
GBPUSD_FXCM = Symbol('GBPUSD', Venue('FXCM'))


class PositionTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.order_factory = OrderFactory(
            id_tag_trader=IdTag('001'),
            id_tag_strategy=IdTag('001'),
            clock=TestClock())
        print('\n')

    def test_position_filled_with_buy_order_returns_expected_attributes(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        order_filled = OrderFilled(
            order.id,
            ExecutionId('E123456'),
            ExecutionTicket('T123456'),
            order.symbol,
            order.side,
            order.quantity,
            Price('1.00001'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        position = Position(PositionId('P123456'), order_filled)

        # Assert
        self.assertEqual(OrderId('O-19700101-000000-001-001-1'), position.from_order_id)
        self.assertEqual(Quantity(100000), position.quantity)
        self.assertEqual(MarketPosition.LONG, position.market_position)
        self.assertEqual(UNIX_EPOCH, position.entry_time)
        self.assertEqual(OrderSide.BUY, position.entry_direction)
        self.assertEqual(Price('1.00001'), position.average_entry_price)
        self.assertEqual(1, position.event_count())
        self.assertEqual([order.id], position.get_order_ids())
        self.assertEqual([ExecutionId('E123456')], position.get_execution_ids())
        self.assertEqual([ExecutionTicket('T123456')], position.get_execution_tickets())
        self.assertEqual(ExecutionId('E123456'), position.last_execution_id)
        self.assertEqual(ExecutionTicket('T123456'), position.last_execution_ticket)
        self.assertFalse(position.is_flat)
        self.assertTrue(position.is_long)
        self.assertFalse(position.is_short)
        self.assertTrue(position.is_entered)
        self.assertFalse(position.is_exited)
        self.assertEqual(Decimal(0), position.points_realized)
        self.assertEqual(0.0, position.return_realized)
        self.assertEqual(0.0004899452906101942, position.return_unrealized(Price('1.00050')))

    def test_position_filled_with_sell_order_returns_expected_attributes(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.SELL,
            Quantity(100000))

        order_filled = OrderFilled(
            order.id,
            ExecutionId('E123456'),
            ExecutionTicket('T123456'),
            order.symbol,
            order.side,
            order.quantity,
            Price('1.00001'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        position = Position(PositionId('P123456'), order_filled)

        # Assert
        self.assertEqual(Quantity(100000), position.quantity)
        self.assertEqual(MarketPosition.SHORT, position.market_position)
        self.assertEqual(UNIX_EPOCH, position.entry_time)
        self.assertEqual(OrderSide.SELL, position.entry_direction)
        self.assertEqual(Price('1.00001'), position.average_entry_price)
        self.assertEqual(1, position.event_count())
        self.assertEqual(ExecutionId('E123456'), position.last_execution_id)
        self.assertEqual(ExecutionTicket('T123456'), position.last_execution_ticket)
        self.assertFalse(position.is_flat)
        self.assertFalse(position.is_long)
        self.assertTrue(position.is_short)
        self.assertTrue(position.is_entered)
        self.assertFalse(position.is_exited)
        self.assertEqual(Decimal(0), position.points_realized)
        self.assertEqual(0.0, position.return_realized)
        self.assertEqual(-0.0004899452906101942, position.return_unrealized(Price('1.00050')))

    def test_position_partial_fills_with_buy_order_returns_expected_attributes(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        order_partially_filled = OrderPartiallyFilled(
            order.id,
            ExecutionId('E123456'),
            ExecutionTicket('T123456'),
            order.symbol,
            order.side,
            Quantity(50000),
            Quantity(50000),
            Price('1.00001'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        position = Position(PositionId('P123456'), order_partially_filled)

        # Act
        position.apply(order_partially_filled)

        # Assert
        self.assertEqual(Quantity(100000), position.quantity)
        self.assertEqual(MarketPosition.LONG, position.market_position)
        self.assertEqual(UNIX_EPOCH, position.entry_time)
        self.assertEqual(OrderSide.BUY, position.entry_direction)
        self.assertEqual(Price('1.00001'), position.average_entry_price)
        self.assertEqual(2, position.event_count())
        self.assertEqual(ExecutionId('E123456'), position.last_execution_id)
        self.assertEqual(ExecutionTicket('T123456'), position.last_execution_ticket)
        self.assertFalse(position.is_flat)
        self.assertTrue(position.is_long)
        self.assertFalse(position.is_short)
        self.assertTrue(position.is_entered)
        self.assertFalse(position.is_exited)
        self.assertEqual(Decimal(0), position.points_realized)
        self.assertEqual(0.0, position.return_realized)
        self.assertEqual(Decimal('0.00049'), position.points_unrealized(Price('1.00050')))
        self.assertEqual(0.0004899452906101942, position.return_unrealized(Price('1.00050')))

    def test_position_partial_fills_with_sell_order_returns_expected_attributes(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.SELL,
            Quantity(100000))

        order_partially_filled = OrderPartiallyFilled(
            order.id,
            ExecutionId('E123456'),
            ExecutionTicket('T123456'),
            order.symbol,
            order.side,
            Quantity(50000),
            Quantity(50000),
            Price('1.00001'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        position = Position(PositionId('P123456'), order_partially_filled)

        # Act
        position.apply(order_partially_filled)

        # Assert
        self.assertEqual(Quantity(100000), position.quantity)
        self.assertEqual(MarketPosition.SHORT, position.market_position)
        self.assertEqual(UNIX_EPOCH, position.entry_time)
        self.assertEqual(OrderSide.SELL, position.entry_direction)
        self.assertEqual(Price('1.00001'), position.average_entry_price)
        self.assertEqual(2, position.event_count())
        self.assertEqual(ExecutionId('E123456'), position.last_execution_id)
        self.assertEqual(ExecutionTicket('T123456'), position.last_execution_ticket)
        self.assertFalse(position.is_flat)
        self.assertFalse(position.is_long)
        self.assertTrue(position.is_short)
        self.assertTrue(position.is_entered)
        self.assertFalse(position.is_exited)
        self.assertEqual(Decimal(0), position.points_realized)
        self.assertEqual(0.0, position.return_realized)
        self.assertEqual(Decimal('-0.00049'), position.points_unrealized(Price('1.00050')))
        self.assertEqual(-0.0004899452906101942, position.return_unrealized(Price('1.00050')))

    def test_position_filled_with_buy_order_then_sell_order_returns_expected_attributes(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        order_filled1 = OrderFilled(
            order.id,
            ExecutionId('E123456'),
            ExecutionTicket('T123456'),
            order.symbol,
            OrderSide.BUY,
            order.quantity,
            Price('1.00001'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        position = Position(PositionId('P123456'), order_filled1)

        order_filled2 = OrderFilled(
            order.id,
            ExecutionId('E123456'),
            ExecutionTicket('T123456'),
            order.symbol,
            OrderSide.SELL,
            order.quantity,
            Price('1.00001'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        position.apply(order_filled2)

        # Assert
        self.assertEqual(Quantity.zero(), position.quantity)
        self.assertEqual(MarketPosition.FLAT, position.market_position)
        self.assertEqual(UNIX_EPOCH, position.entry_time)
        self.assertEqual(OrderSide.BUY, position.entry_direction)
        self.assertEqual(Price('1.00001'), position.average_entry_price)
        self.assertEqual(2, position.event_count())
        self.assertEqual(ExecutionId('E123456'), position.last_execution_id)
        self.assertEqual(ExecutionTicket('T123456'), position.last_execution_ticket)
        self.assertEqual(UNIX_EPOCH, position.exit_time)
        self.assertEqual(Price('1.00001'), position.average_exit_price)
        self.assertTrue(position.is_flat)
        self.assertFalse(position.is_long)
        self.assertFalse(position.is_short)
        self.assertTrue(position.is_entered)
        self.assertTrue(position.is_exited)
        self.assertEqual(Decimal(0), position.points_realized)  # No change in price
        self.assertEqual(Decimal(0), position.points_unrealized(Price('1.00050')))
        self.assertEqual(0.0, position.return_realized)  # No change in price
        self.assertEqual(0.0, position.return_unrealized(Price('1.00050')))

    def test_position_filled_with_sell_order_then_buy_order_returns_expected_attributes(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.SELL,
            Quantity(100000))

        order_filled1 = OrderFilled(
            order.id,
            ExecutionId('E123456'),
            ExecutionTicket('T123456'),
            order.symbol,
            OrderSide.SELL,
            order.quantity,
            Price('1.00000'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        position = Position(PositionId('P123456'), order_filled1)

        order_filled2 = OrderFilled(
            order.id,
            ExecutionId('E123456'),
            ExecutionTicket('T123456'),
            order.symbol,
            OrderSide.BUY,
            order.quantity,
            Price('1.00001'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        position.apply(order_filled2)

        # Assert
        self.assertEqual(Quantity.zero(), position.quantity)
        self.assertEqual(MarketPosition.FLAT, position.market_position)
        self.assertEqual(UNIX_EPOCH, position.entry_time)
        self.assertEqual(OrderSide.SELL, position.entry_direction)
        self.assertEqual(Price('1.00000'), position.average_entry_price)
        self.assertEqual(2, position.event_count())
        self.assertEqual([order.id], position.get_order_ids())
        self.assertEqual(ExecutionId('E123456'), position.last_execution_id)
        self.assertEqual(ExecutionTicket('T123456'), position.last_execution_ticket)
        self.assertEqual(UNIX_EPOCH, position.exit_time)
        self.assertEqual(Price('1.00001'), position.average_exit_price)
        self.assertTrue(position.is_flat)
        self.assertFalse(position.is_long)
        self.assertFalse(position.is_short)
        self.assertTrue(position.is_entered)
        self.assertTrue(position.is_exited)
        self.assertEqual(Decimal('-0.00001'), position.points_realized)
        self.assertEqual(Decimal(0), position.points_unrealized(Price('1.00050')))  # No more quantity in market
        self.assertEqual(-1.0013580322265625e-05, position.return_realized)
        self.assertEqual(0.0, position.return_unrealized(Price('1.00050')))  # No more quantity in market

    def test_position_filled_with_no_pnl_returns_expected_attributes(self):
        # Arrange
        order1 = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        order2 = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.SELL,
            Quantity(100000))

        order1_filled = OrderFilled(
            order1.id,
            ExecutionId('E123456'),
            ExecutionTicket('T123456'),
            order1.symbol,
            order1.side,
            order1.quantity,
            Price('1.00000'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        position = Position(PositionId('P123456'), order1_filled)

        order2_filled = OrderFilled(
            order2.id,
            ExecutionId('E123456'),
            ExecutionTicket('T123456'),
            order2.symbol,
            order2.side,
            order2.quantity,
            Price('1.00000'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        position.apply(order2_filled)

        # Assert
        self.assertEqual(Quantity.zero(), position.quantity)
        self.assertEqual(MarketPosition.FLAT, position.market_position)
        self.assertEqual(UNIX_EPOCH, position.entry_time)
        self.assertEqual(OrderSide.BUY, position.entry_direction)
        self.assertEqual(Price('1.00000'), position.average_entry_price)
        self.assertEqual(2, position.event_count())
        self.assertEqual([order1.id, order2.id], position.get_order_ids())
        self.assertEqual(ExecutionId('E123456'), position.last_execution_id)
        self.assertEqual(ExecutionTicket('T123456'), position.last_execution_ticket)
        self.assertEqual(UNIX_EPOCH, position.exit_time)
        self.assertEqual(Price('1.00000'), position.average_exit_price)
        self.assertTrue(position.is_flat)
        self.assertFalse(position.is_long)
        self.assertFalse(position.is_short)
        self.assertTrue(position.is_entered)
        self.assertTrue(position.is_exited)
        self.assertEqual(Decimal('0'), position.points_realized)
        self.assertEqual(Decimal(0), position.points_unrealized(Price('1.00050')))  # No more quantity in market
        self.assertEqual(0, position.return_realized)
        self.assertEqual(0.0, position.return_unrealized(Price('1.00050')))  # No more quantity in market
