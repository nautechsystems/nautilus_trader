#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_portfolio.py" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest
import uuid

from inv_trader.common.clock import TestClock
from inv_trader.model.enums import Venue, OrderSide
from inv_trader.model.objects import ValidString, Quantity, Symbol, Price
from inv_trader.model.order import OrderFactory
from inv_trader.model.events import OrderFilled
from inv_trader.model.identifiers import GUID, OrderId, PositionId, ExecutionId, ExecutionTicket
from inv_trader.model.position import Position
from inv_trader.strategy import TradeStrategy
from inv_trader.portfolio.portfolio import Portfolio
from test_kit.mocks import MockExecClient
from test_kit.stubs import TestStubs

UNIX_EPOCH = TestStubs.unix_epoch()
AUDUSD_FXCM = Symbol('AUDUSD', Venue.FXCM)
GBPUSD_FXCM = Symbol('GBPUSD', Venue.FXCM)


class PortfolioTestsTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.order_factory = OrderFactory(
            id_tag_trader=ValidString('001'),
            id_tag_strategy=ValidString('001'),
            clock=TestClock())
        self.portfolio = Portfolio()
        self.portfolio.register_execution_client(MockExecClient())
        print('\n')

    def test_can_register_strategy(self):
        # Arrange
        strategy = TradeStrategy()

        # Act
        self.portfolio.register_strategy(strategy.id)

        # Assert
        self.assertTrue(strategy.id in self.portfolio.registered_strategies())

    def test_can_register_order(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        position_id = PositionId('AUDUSD-1-123456')

        # Act
        self.portfolio.register_order(order.id, position_id)

        # Assert
        self.assertTrue(order.id in self.portfolio.registered_order_ids())
        self.assertTrue(position_id in self.portfolio.registered_position_ids())

    def test_position_exists_when_no_position(self):
        # Arrange
        # Act
        # Assert
        self.assertFalse(self.portfolio.position_exists(PositionId('unknown')))

    def test_opens_new_position_on_order_fill(self):
        # Arrange
        strategy = TradeStrategy()
        order_id = OrderId('AUDUSD.FXCM-1-123456')
        position_id = PositionId('AUDUSD.FXCM-1-123456')

        self.portfolio.register_strategy(strategy.id)
        self.portfolio.register_order(order_id, position_id)
        event = OrderFilled(
            AUDUSD_FXCM,
            order_id,
            ExecutionId('E123456'),
            ExecutionTicket('T123456'),
            OrderSide.SELL,
            Quantity(100000),
            Price('1.00000'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        self.portfolio.handle_event(event, strategy.id)

        # Assert
        self.assertTrue(self.portfolio.position_exists(position_id))
        self.assertEqual(Position, type(self.portfolio.get_position(position_id)))
        self.assertTrue(position_id in self.portfolio.get_positions_all())
        self.assertTrue(position_id not in self.portfolio.get_positions_closed(strategy.id))
        self.assertTrue(position_id not in self.portfolio.get_positions_closed_all()[strategy.id])
        self.assertTrue(position_id in self.portfolio.get_positions_active(strategy.id))
        self.assertTrue(position_id in self.portfolio.get_positions_active_all()[strategy.id])
        self.assertEqual(1, self.portfolio.positions_count)
        self.assertEqual(1, self.portfolio.positions_active_count)
        self.assertEqual(0, self.portfolio.positions_closed_count)
        self.assertEqual(1, len(self.portfolio.position_opened_events))
        self.assertEqual(0, len(self.portfolio.position_closed_events))

    def test_adds_to_existing_position_on_order_fill(self):
        # Arrange
        strategy = TradeStrategy()
        order_id = OrderId('AUDUSD.FXCM-1-123456')
        position_id = PositionId('AUDUSD.FXCM-1-123456')

        self.portfolio.register_strategy(strategy.id)
        self.portfolio.register_order(order_id, position_id)

        event = OrderFilled(
            AUDUSD_FXCM,
            order_id,
            ExecutionId('E123456'),
            ExecutionTicket('T123456'),
            OrderSide.SELL,
            Quantity(100000),
            Price('1.00000'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        self.portfolio.handle_event(event, strategy.id)
        self.portfolio.handle_event(event, strategy.id)

        # Assert
        self.assertTrue(self.portfolio.position_exists(position_id))
        self.assertEqual(Position, type(self.portfolio.get_position(position_id)))
        self.assertEqual(0, len(self.portfolio.get_positions_closed(strategy.id)))
        self.assertEqual(0, len(self.portfolio.get_positions_closed_all()[strategy.id]))
        self.assertEqual(1, len(self.portfolio.get_positions_active(strategy.id)))
        self.assertEqual(1, len(self.portfolio.get_positions_active_all()[strategy.id]))
        self.assertEqual(1, self.portfolio.positions_count)
        self.assertEqual(1, self.portfolio.positions_active_count)
        self.assertEqual(0, self.portfolio.positions_closed_count)
        self.assertEqual(1, len(self.portfolio.position_opened_events))
        self.assertEqual(0, len(self.portfolio.position_closed_events))

    def test_closes_position_on_order_fill(self):
        # Arrange
        strategy = TradeStrategy()
        order_id = OrderId('AUDUSD.FXCM-1-123456')
        position_id = PositionId('AUDUSD.FXCM-1-123456')

        self.portfolio.register_strategy(strategy.id)
        self.portfolio.register_order(order_id, position_id)

        buy = OrderFilled(
            AUDUSD_FXCM,
            order_id,
            ExecutionId('E123456'),
            ExecutionTicket('T123456'),
            OrderSide.BUY,
            Quantity(100000),
            Price('1.00000'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        sell = OrderFilled(
            AUDUSD_FXCM,
            order_id,
            ExecutionId('E1234567'),
            ExecutionTicket('T1234567'),
            OrderSide.SELL,
            Quantity(100000),
            Price('1.00000'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        self.portfolio.handle_event(buy, strategy.id)
        self.portfolio.handle_event(sell, strategy.id)

        # Assert
        self.assertTrue(self.portfolio.position_exists(position_id))
        self.assertEqual(Position, type(self.portfolio.get_position(position_id)))
        self.assertTrue(position_id in self.portfolio.get_positions(strategy.id))
        self.assertTrue(position_id in self.portfolio.get_positions_all())
        self.assertEqual(0, len(self.portfolio.get_positions_active(strategy.id)))
        self.assertEqual(0, len(self.portfolio.get_positions_active_all()[strategy.id]))
        self.assertTrue(position_id in self.portfolio.get_positions_closed(strategy.id))
        self.assertTrue(position_id in self.portfolio.get_positions_closed_all()[strategy.id])
        self.assertTrue(position_id not in self.portfolio.get_positions_active(strategy.id))
        self.assertTrue(position_id not in self.portfolio.get_positions_active_all()[strategy.id])
        self.assertEqual(1, self.portfolio.positions_count)
        self.assertEqual(0, self.portfolio.positions_active_count)
        self.assertEqual(1, self.portfolio.positions_closed_count)
        self.assertEqual(1, len(self.portfolio.position_opened_events))
        self.assertEqual(1, len(self.portfolio.position_closed_events))

    def test_multiple_strategy_positions_opened(self):
        # Arrange
        strategy1 = TradeStrategy()
        strategy2 = TradeStrategy()
        order_id1 = OrderId('AUDUSD.FXCM-1-1')
        order_id2 = OrderId('AUDUSD.FXCM-1-2')
        position_id1 = PositionId('AUDUSD.FXCM-1-1')
        position_id2 = PositionId('AUDUSD.FXCM-1-2')

        self.portfolio.register_strategy(strategy1.id)
        self.portfolio.register_strategy(strategy2.id)
        self.portfolio.register_order(order_id1, position_id1)
        self.portfolio.register_order(order_id2, position_id2)

        buy1 = OrderFilled(
            AUDUSD_FXCM,
            order_id1,
            ExecutionId('E1'),
            ExecutionTicket('T1'),
            OrderSide.BUY,
            Quantity(100000),
            Price('1.00000'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        buy2 = OrderFilled(
            AUDUSD_FXCM,
            order_id2,
            ExecutionId('E2'),
            ExecutionTicket('T2'),
            OrderSide.BUY,
            Quantity(100000),
            Price('1.00000'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        self.portfolio.handle_event(buy1, strategy1.id)
        self.portfolio.handle_event(buy2, strategy2.id)

        # Assert
        self.assertTrue(self.portfolio.position_exists(position_id1))
        self.assertTrue(self.portfolio.position_exists(position_id2))
        self.assertEqual(Position, type(self.portfolio.get_position(position_id1)))
        self.assertEqual(Position, type(self.portfolio.get_position(position_id2)))
        self.assertTrue(position_id1 in self.portfolio.get_positions(strategy1.id))
        self.assertTrue(position_id2 in self.portfolio.get_positions(strategy2.id))
        self.assertTrue(position_id1 in self.portfolio.get_positions_all())
        self.assertTrue(position_id2 in self.portfolio.get_positions_all())
        self.assertEqual(1, len(self.portfolio.get_positions_active(strategy1.id)))
        self.assertEqual(1, len(self.portfolio.get_positions_active(strategy2.id)))
        self.assertEqual(2, len(self.portfolio.get_positions_active_all()))
        self.assertEqual(1, len(self.portfolio.get_positions_active_all()[strategy1.id]))
        self.assertEqual(1, len(self.portfolio.get_positions_active_all()[strategy2.id]))
        self.assertTrue(position_id1 in self.portfolio.get_positions_active(strategy1.id))
        self.assertTrue(position_id2 in self.portfolio.get_positions_active(strategy2.id))
        self.assertTrue(position_id1 in self.portfolio.get_positions_active_all()[strategy1.id])
        self.assertTrue(position_id2 in self.portfolio.get_positions_active_all()[strategy2.id])
        self.assertTrue(position_id1 not in self.portfolio.get_positions_closed(strategy1.id))
        self.assertTrue(position_id2 not in self.portfolio.get_positions_closed(strategy2.id))
        self.assertTrue(position_id1 not in self.portfolio.get_positions_closed_all()[strategy1.id])
        self.assertTrue(position_id2 not in self.portfolio.get_positions_closed_all()[strategy2.id])
        self.assertEqual(2, self.portfolio.positions_count)
        self.assertEqual(2, self.portfolio.positions_active_count)
        self.assertEqual(0, self.portfolio.positions_closed_count)
        self.assertEqual(2, len(self.portfolio.position_opened_events))
        self.assertEqual(0, len(self.portfolio.position_closed_events))

    def test_multiple_strategy_positions_one_active_one_closed(self):
        # Arrange
        strategy1 = TradeStrategy()
        strategy2 = TradeStrategy()
        order_id1 = OrderId('AUDUSD.FXCM-1-1')
        order_id2 = OrderId('AUDUSD.FXCM-1-2')
        order_id3 = OrderId('AUDUSD.FXCM-1-3')
        position_id1 = PositionId('AUDUSD.FXCM-1-1')
        position_id2 = PositionId('AUDUSD.FXCM-1-2')

        self.portfolio.register_strategy(strategy1.id)
        self.portfolio.register_strategy(strategy2.id)
        self.portfolio.register_order(order_id1, position_id1)
        self.portfolio.register_order(order_id2, position_id2)
        self.portfolio.register_order(order_id3, position_id1)

        buy1 = OrderFilled(
            AUDUSD_FXCM,
            order_id1,
            ExecutionId('E1'),
            ExecutionTicket('T1'),
            OrderSide.BUY,
            Quantity(100000),
            Price('1.00000'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        buy2 = OrderFilled(
            AUDUSD_FXCM,
            order_id2,
            ExecutionId('E2'),
            ExecutionTicket('T2'),
            OrderSide.BUY,
            Quantity(100000),
            Price('1.00000'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        sell1 = OrderFilled(
            AUDUSD_FXCM,
            order_id3,
            ExecutionId('E3'),
            ExecutionTicket('T3'),
            OrderSide.SELL,
            Quantity(100000),
            Price('1.00000'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        self.portfolio.handle_event(buy1, strategy1.id)
        self.portfolio.handle_event(buy2, strategy2.id)
        self.portfolio.handle_event(sell1, strategy1.id)

        # Assert
        self.assertTrue(position_id1 in self.portfolio.get_positions(strategy1.id))
        self.assertTrue(position_id2 in self.portfolio.get_positions(strategy2.id))
        self.assertTrue(position_id1 in self.portfolio.get_positions_all())
        self.assertTrue(position_id2 in self.portfolio.get_positions_all())
        self.assertEqual(0, len(self.portfolio.get_positions_active(strategy1.id)))
        self.assertEqual(1, len(self.portfolio.get_positions_active(strategy2.id)))
        self.assertEqual(0, len(self.portfolio.get_positions_active_all()[strategy1.id]))
        self.assertEqual(1, len(self.portfolio.get_positions_active_all()[strategy2.id]))
        self.assertTrue(position_id1 not in self.portfolio.get_positions_active(strategy1.id))
        self.assertTrue(position_id2 in self.portfolio.get_positions_active(strategy2.id))
        self.assertTrue(position_id1 not in self.portfolio.get_positions_active_all()[strategy1.id])
        self.assertTrue(position_id2 in self.portfolio.get_positions_active_all()[strategy2.id])
        self.assertTrue(position_id1 in self.portfolio.get_positions_closed(strategy1.id))
        self.assertTrue(position_id2 not in self.portfolio.get_positions_closed(strategy2.id))
        self.assertTrue(position_id1 in self.portfolio.get_positions_closed_all()[strategy1.id])
        self.assertTrue(position_id2 not in self.portfolio.get_positions_closed_all()[strategy2.id])
        self.assertEqual(2, self.portfolio.positions_count)
        self.assertEqual(1, self.portfolio.positions_active_count)
        self.assertEqual(1, self.portfolio.positions_closed_count)
        self.assertEqual(2, len(self.portfolio.position_opened_events))
        self.assertEqual(1, len(self.portfolio.position_closed_events))
