# -------------------------------------------------------------------------------------------------
# <copyright file="test_common_portfolio.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest
import uuid

from nautilus_trader.backtest.execution import BacktestExecClient
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.common.account import Account
from nautilus_trader.common.brokerage import CommissionCalculator
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.guid import TestGuidFactory
from nautilus_trader.common.logger import TestLogger
from nautilus_trader.common.execution import InMemoryExecutionEngine
from nautilus_trader.core.types import GUID
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.identifiers import IdTag, OrderId, PositionId, ExecutionId, ExecutionTicket
from nautilus_trader.model.objects import Quantity, Venue, Symbol, Price, Money
from nautilus_trader.model.order import OrderFactory
from nautilus_trader.model.position import Position
from nautilus_trader.trade.portfolio import Portfolio
from nautilus_trader.trade.strategy import TradingStrategy

from test_kit.stubs import TestStubs

UNIX_EPOCH = TestStubs.unix_epoch()
AUDUSD_FXCM = Symbol('AUDUSD', Venue('FXCM'))
GBPUSD_FXCM = Symbol('GBPUSD', Venue('FXCM'))


class PortfolioTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.order_factory = OrderFactory(
            id_tag_trader=IdTag('001'),
            id_tag_strategy=IdTag('001'),
            clock=TestClock())
        self.portfolio = Portfolio()

        self.instruments = [TestStubs.instrument_usdjpy()]
        self.account = Account()

        self.portfolio = Portfolio(
            clock=TestClock(),
            guid_factory=TestGuidFactory(),
            logger=TestLogger())

        self.exec_engine = InMemoryExecutionEngine()
        self.exec_client = BacktestExecClient(
            instruments=self.instruments,
            frozen_account=False,
            starting_capital=Money(1000000),
            fill_model=FillModel(),
            commission_calculator=CommissionCalculator(),
            account=self.account,
            portfolio=self.portfolio,
            clock=TestClock(),
            guid_factory=TestGuidFactory(),
            logger=TestLogger())
        self.portfolio.register_execution_client(self.exec_client)
        print('\n')

    def test_can_register_strategy(self):
        # Arrange
        strategy = TradingStrategy(id_tag_strategy='001')

        # Act
        self.portfolio.register_strategy(strategy)

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

    def test_position_exists_when_no_position_returns_false(self):
        # Arrange
        # Act
        # Assert
        self.assertFalse(self.portfolio.is_position_exists(PositionId('unknown')))

    def test_position_for_order_has_position_when_no_position_returns_false(self):
        # Arrange
        # Act
        # Assert
        self.assertFalse(self.portfolio.is_position_for_order(OrderId('unknown')))

    def test_is_flat_when_no_registered_strategies_returns_false(self):
        # Arrange
        # Act
        # Assert
        self.assertTrue(self.portfolio.is_flat())

    def test_get_position_for_order_when_no_position_returns_none(self):
        # Arrange
        order_id = OrderId('AUDUSD.FXCM-1-123456')

        # Act
        result = self.portfolio.get_position_for_order(order_id)

        # Assert
        self.assertIsNone(result)

    def test_get_position_when_no_position_returns_none(self):
        # Arrange
        position_id = PositionId('AUDUSD.FXCM-1-123456')

        # Act
        result = self.portfolio.get_position(position_id)

        # Assert
        self.assertIsNone(result)

    def test_can_reset_portfolio(self):
        strategy = TradingStrategy(id_tag_strategy='001')
        order_id = OrderId('AUDUSD.FXCM-1-123456')
        position_id = PositionId('AUDUSD.FXCM-1-123456')

        self.exec_engine.register_strategy(strategy)  # Also registers with portfolio
        self.portfolio.register_order(order_id, position_id)
        event = OrderFilled(
            order_id,
            ExecutionId('E123456'),
            ExecutionTicket('T123456'),
            AUDUSD_FXCM,
            OrderSide.SELL,
            Quantity(100000),
            Price('1.00000'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        self.portfolio.handle_order_fill(event, strategy.id)

        # Act
        self.portfolio.reset()

        # Assert
        self.assertTrue(strategy.id in self.portfolio.registered_strategies())
        self.assertFalse(self.portfolio.is_position_for_order(order_id))
        self.assertEqual({}, self.portfolio.get_positions_all())

    def test_opens_new_position_on_order_fill(self):
        # Arrange
        strategy = TradingStrategy(id_tag_strategy='001')
        order_id = OrderId('AUDUSD.FXCM-1-123456')
        position_id = PositionId('AUDUSD.FXCM-1-123456')

        self.exec_client.register_strategy(strategy)  # Also registers with portfolio
        self.portfolio.register_order(order_id, position_id)
        event = OrderFilled(
            order_id,
            ExecutionId('E123456'),
            ExecutionTicket('T123456'),
            AUDUSD_FXCM,
            OrderSide.SELL,
            Quantity(100000),
            Price('1.00000'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        self.portfolio.handle_order_fill(event, strategy.id)

        # Assert
        self.assertTrue(self.portfolio.is_position_exists(position_id))
        self.assertTrue(self.portfolio.is_position_active(position_id))
        self.assertFalse(self.portfolio.is_position_closed(position_id))
        self.assertFalse(self.portfolio.is_strategy_flat(strategy.id))
        self.assertFalse(self.portfolio.is_flat())
        self.assertEqual(Position, type(self.portfolio.get_position(position_id)))
        self.assertTrue(position_id in self.portfolio.get_positions_all())
        self.assertTrue(position_id not in self.portfolio.get_positions_closed(strategy.id))
        self.assertTrue(position_id not in self.portfolio.get_positions_closed_all()[strategy.id])
        self.assertTrue(position_id in self.portfolio.get_positions_active(strategy.id))
        self.assertTrue(position_id in self.portfolio.get_positions_active_all()[strategy.id])
        self.assertEqual(1, self.portfolio.positions_count())
        self.assertEqual(1, self.portfolio.positions_active_count())
        self.assertEqual(0, self.portfolio.positions_closed_count())
        self.assertEqual(1, len(self.portfolio.position_opened_events))
        self.assertEqual(0, len(self.portfolio.position_closed_events))
        self.assertTrue(self.portfolio.is_position_for_order(order_id))
        self.assertEqual(Position, type(self.portfolio.get_position_for_order(order_id)))

    def test_adds_to_existing_position_on_order_fill(self):
        # Arrange
        strategy = TradingStrategy(id_tag_strategy='001')
        order_id = OrderId('AUDUSD.FXCM-1-123456')
        position_id = PositionId('AUDUSD.FXCM-1-123456')

        self.exec_client.register_strategy(strategy)  # Also registers with portfolio
        self.portfolio.register_order(order_id, position_id)

        event = OrderFilled(
            order_id,
            ExecutionId('E123456'),
            ExecutionTicket('T123456'),
            AUDUSD_FXCM,
            OrderSide.SELL,
            Quantity(100000),
            Price('1.00000'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        self.portfolio.handle_order_fill(event, strategy.id)
        self.portfolio.handle_order_fill(event, strategy.id)

        # Assert
        self.assertTrue(self.portfolio.is_position_exists(position_id))
        self.assertTrue(self.portfolio.is_position_active(position_id))
        self.assertFalse(self.portfolio.is_position_closed(position_id))
        self.assertFalse(self.portfolio.is_strategy_flat(strategy.id))
        self.assertFalse(self.portfolio.is_flat())
        self.assertEqual(Position, type(self.portfolio.get_position(position_id)))
        self.assertEqual(0, len(self.portfolio.get_positions_closed(strategy.id)))
        self.assertEqual(0, len(self.portfolio.get_positions_closed_all()[strategy.id]))
        self.assertEqual(1, len(self.portfolio.get_positions_active(strategy.id)))
        self.assertEqual(1, len(self.portfolio.get_positions_active_all()[strategy.id]))
        self.assertEqual(1, self.portfolio.positions_count())
        self.assertEqual(1, self.portfolio.positions_active_count())
        self.assertEqual(0, self.portfolio.positions_closed_count())
        self.assertEqual(1, len(self.portfolio.position_opened_events))
        self.assertEqual(0, len(self.portfolio.position_closed_events))

    def test_closes_position_on_order_fill(self):
        # Arrange
        strategy = TradingStrategy(id_tag_strategy='001')
        order_id1 = OrderId('AUDUSD.FXCM-1-123456-1')
        order_id2 = OrderId('AUDUSD.FXCM-1-123456-2')
        position_id = PositionId('AUDUSD.FXCM-1-123456')

        self.exec_client.register_strategy(strategy)  # Also registers with portfolio
        self.portfolio.register_order(order_id1, position_id)
        self.portfolio.register_order(order_id2, position_id)

        buy = OrderFilled(
            order_id1,
            ExecutionId('E123456'),
            ExecutionTicket('T123456'),
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price('1.00000'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        sell = OrderFilled(
            order_id2,
            ExecutionId('E1234567'),
            ExecutionTicket('T1234567'),
            AUDUSD_FXCM,
            OrderSide.SELL,
            Quantity(100000),
            Price('1.00000'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        self.portfolio.handle_order_fill(buy, strategy.id)
        self.portfolio.handle_order_fill(sell, strategy.id)

        # Assert
        self.assertTrue(self.portfolio.is_position_exists(position_id))
        self.assertFalse(self.portfolio.is_position_active(position_id))
        self.assertTrue(self.portfolio.is_position_closed(position_id))
        self.assertTrue(self.portfolio.is_strategy_flat(strategy.id))
        self.assertTrue(self.portfolio.is_flat())
        self.assertEqual(position_id, self.portfolio.get_position(position_id).id)
        self.assertTrue(position_id in self.portfolio.get_positions(strategy.id))
        self.assertTrue(position_id in self.portfolio.get_positions_all())
        self.assertEqual(0, len(self.portfolio.get_positions_active(strategy.id)))
        self.assertEqual(0, len(self.portfolio.get_positions_active_all()[strategy.id]))
        self.assertTrue(position_id in self.portfolio.get_positions_closed(strategy.id))
        self.assertTrue(position_id in self.portfolio.get_positions_closed_all()[strategy.id])
        self.assertTrue(position_id not in self.portfolio.get_positions_active(strategy.id))
        self.assertTrue(position_id not in self.portfolio.get_positions_active_all()[strategy.id])
        self.assertEqual(1, self.portfolio.positions_count())
        self.assertEqual(0, self.portfolio.positions_active_count())
        self.assertEqual(1, self.portfolio.positions_closed_count())
        self.assertEqual(1, len(self.portfolio.position_opened_events))
        self.assertEqual(1, len(self.portfolio.position_closed_events))

    def test_multiple_strategy_positions_opened(self):
        # Arrange
        strategy1 = TradingStrategy(id_tag_strategy='001')
        strategy2 = TradingStrategy(id_tag_strategy='002')
        order_id1 = OrderId('AUDUSD.FXCM-1-1')
        order_id2 = OrderId('AUDUSD.FXCM-1-2')
        position_id1 = PositionId('AUDUSD.FXCM-1-1')
        position_id2 = PositionId('AUDUSD.FXCM-1-2')

        self.exec_client.register_strategy(strategy1)  # Also registers with portfolio
        self.exec_client.register_strategy(strategy2)  # Also registers with portfolio
        self.portfolio.register_order(order_id1, position_id1)
        self.portfolio.register_order(order_id2, position_id2)

        buy1 = OrderFilled(
            order_id1,
            ExecutionId('E1'),
            ExecutionTicket('T1'),
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price('1.00000'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        buy2 = OrderFilled(
            order_id2,
            ExecutionId('E2'),
            ExecutionTicket('T2'),
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price('1.00000'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        self.portfolio.handle_order_fill(buy1, strategy1.id)
        self.portfolio.handle_order_fill(buy2, strategy2.id)

        # Assert
        self.assertTrue(self.portfolio.is_position_exists(position_id1))
        self.assertTrue(self.portfolio.is_position_exists(position_id2))
        self.assertTrue(self.portfolio.is_position_active(position_id1))
        self.assertTrue(self.portfolio.is_position_active(position_id2))
        self.assertFalse(self.portfolio.is_position_closed(position_id1))
        self.assertFalse(self.portfolio.is_position_closed(position_id2))
        self.assertFalse(self.portfolio.is_strategy_flat(strategy1.id))
        self.assertFalse(self.portfolio.is_strategy_flat(strategy2.id))
        self.assertFalse(self.portfolio.is_flat())
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
        self.assertEqual(2, self.portfolio.positions_count())
        self.assertEqual(2, self.portfolio.positions_active_count())
        self.assertEqual(0, self.portfolio.positions_closed_count())
        self.assertEqual(2, len(self.portfolio.position_opened_events))
        self.assertEqual(0, len(self.portfolio.position_closed_events))

    def test_multiple_strategy_positions_one_active_one_closed(self):
        # Arrange
        strategy1 = TradingStrategy(id_tag_strategy='001')
        strategy2 = TradingStrategy(id_tag_strategy='002')
        order_id1 = OrderId('AUDUSD.FXCM-1-1')
        order_id2 = OrderId('AUDUSD.FXCM-1-2')
        order_id3 = OrderId('AUDUSD.FXCM-1-3')
        position_id1 = PositionId('AUDUSD.FXCM-1-1')
        position_id2 = PositionId('AUDUSD.FXCM-1-2')

        self.exec_client.register_strategy(strategy1)  # Also registers with portfolio
        self.exec_client.register_strategy(strategy2)  # Also registers with portfolio
        self.portfolio.register_order(order_id1, position_id1)
        self.portfolio.register_order(order_id2, position_id2)
        self.portfolio.register_order(order_id3, position_id1)

        buy1 = OrderFilled(
            order_id1,
            ExecutionId('E1'),
            ExecutionTicket('T1'),
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price('1.00000'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        buy2 = OrderFilled(
            order_id2,
            ExecutionId('E2'),
            ExecutionTicket('T2'),
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price('1.00000'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        sell1 = OrderFilled(
            order_id3,
            ExecutionId('E3'),
            ExecutionTicket('T3'),
            AUDUSD_FXCM,
            OrderSide.SELL,
            Quantity(100000),
            Price('1.00000'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        self.portfolio.handle_order_fill(buy1, strategy1.id)
        self.portfolio.handle_order_fill(buy2, strategy2.id)
        self.portfolio.handle_order_fill(sell1, strategy1.id)

        # Assert
        # Already tested .is_position_active and .is_position_closed above
        self.assertTrue(self.portfolio.is_position_exists(position_id1))
        self.assertTrue(self.portfolio.is_position_exists(position_id2))
        self.assertTrue(self.portfolio.is_strategy_flat(strategy1.id))
        self.assertFalse(self.portfolio.is_strategy_flat(strategy2.id))
        self.assertFalse(self.portfolio.is_flat())
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
        self.assertEqual(2, self.portfolio.positions_count())
        self.assertEqual(1, self.portfolio.positions_active_count())
        self.assertEqual(1, self.portfolio.positions_closed_count())
        self.assertEqual(2, len(self.portfolio.position_opened_events))
        self.assertEqual(1, len(self.portfolio.position_closed_events))
