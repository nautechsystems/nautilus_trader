# -------------------------------------------------------------------------------------------------
# <copyright file="test_common_execution.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest
import uuid

from nautilus_trader.core.types import GUID
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.identifiers import (
    TraderId,
    StrategyId,
    IdTag,
    OrderId,
    PositionId,
    ExecutionId,
    ExecutionTicket)
from nautilus_trader.model.objects import Quantity, Venue, Symbol, Price, Money
from nautilus_trader.model.order import OrderFactory
from nautilus_trader.model.position import Position
from nautilus_trader.model.commands import SubmitOrder
from nautilus_trader.common.account import Account
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.guid import TestGuidFactory
from nautilus_trader.common.logger import TestLogger
from nautilus_trader.common.portfolio import Portfolio
from nautilus_trader.common.execution import InMemoryExecutionDatabase, ExecutionEngine
from nautilus_trader.trade.strategy import TradingStrategy
from test_kit.dummies import DummyExecutionClient
from test_kit.stubs import TestStubs

UNIX_EPOCH = TestStubs.unix_epoch()
AUDUSD_FXCM = Symbol('AUDUSD', Venue('FXCM'))
GBPUSD_FXCM = Symbol('GBPUSD', Venue('FXCM'))


class InMemoryExecutionDatabaseTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        clock = TestClock()
        guid_factory = TestGuidFactory()
        logger = TestLogger()

        self.trader_id = TraderId('000')
        self.id_tag_trader = IdTag('000')
        self.id_tag_strategy = IdTag('001')

        self.order_factory = OrderFactory(
            id_tag_trader=self.id_tag_trader,
            id_tag_strategy=self.id_tag_strategy,
            clock=clock)

        self.account = Account()
        self.portfolio = Portfolio(
            clock=clock,
            guid_factory=guid_factory,
            logger=logger)

        self.exec_db = InMemoryExecutionDatabase(trader_id=self.trader_id, logger=logger)

    def test_can_add_strategy(self):
        # Arrange
        strategy = TradingStrategy(id_tag_strategy='001')

        # Act
        self.exec_db.add_strategy(strategy)

        # Assert
        self.assertTrue(strategy.id in self.exec_db.get_strategy_ids())

    def test_can_add_order(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        strategy_id = StrategyId('001')
        position_id = PositionId('AUDUSD-1-123456')

        # Act
        self.exec_db.add_order(order, strategy_id, position_id)

        # Assert
        self.assertTrue(order.id in self.exec_db.get_order_ids())
        self.assertEqual(order, self.exec_db.get_orders_all()[order.id])

    def test_can_add_position(self):
        # Arrange
        strategy = TradingStrategy(id_tag_strategy='001')
        position_id = PositionId('AUDUSD-1-123456')
        position = Position(
            symbol=AUDUSD_FXCM,
            position_id=position_id,
            timestamp=UNIX_EPOCH)

        self.exec_db.add_strategy(strategy)

        # Act
        self.exec_db.add_position(position, strategy.id)

        # Assert
        self.assertTrue(position.id in self.exec_db.get_position_ids())
        self.assertTrue(position.id in self.exec_db.get_positions_active(strategy.id))
        self.assertTrue(position.id in self.exec_db.get_positions_active_all()[strategy.id])

    def test_can_delete_strategy(self):
        # Arrange
        strategy = TradingStrategy(id_tag_strategy='001')
        self.exec_db.add_strategy(strategy)

        # Act
        self.exec_db.delete_strategy(strategy)

        # Assert
        self.assertTrue(strategy.id not in self.exec_db.get_strategy_ids())

    def test_can_make_order_active(self):
        # Arrange
        strategy = TradingStrategy(id_tag_strategy='001')
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        position_id = PositionId('AUDUSD-1-123456')

        self.exec_db.add_strategy(strategy)
        self.exec_db.add_order(order, strategy.id, position_id)

        # Act
        self.exec_db.order_active(order, strategy.id)

        # Assert
        self.assertTrue(self.exec_db.order_exists(order.id))
        self.assertTrue(order.id in self.exec_db.get_order_ids())
        self.assertTrue(order.id in self.exec_db.get_orders_all())
        self.assertTrue(order.id in self.exec_db.get_orders_active(strategy.id))
        self.assertTrue(order.id in self.exec_db.get_orders_active_all()[strategy.id])
        self.assertTrue(order.id not in self.exec_db.get_orders_completed(strategy.id))
        self.assertTrue(order.id not in self.exec_db.get_orders_completed_all()[strategy.id])

    def test_can_make_order_completed(self):
        # Arrange
        strategy = TradingStrategy(id_tag_strategy='001')
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        position_id = PositionId('AUDUSD-1-123456')

        self.exec_db.add_strategy(strategy)
        self.exec_db.add_order(order, strategy.id, position_id)
        self.exec_db.order_active(order, strategy.id)

        # Act
        self.exec_db.order_completed(order, strategy.id)

        # Assert
        self.assertTrue(self.exec_db.order_exists(order.id))
        self.assertTrue(order.id in self.exec_db.get_order_ids())
        self.assertTrue(order.id in self.exec_db.get_orders_all())
        self.assertTrue(order.id in self.exec_db.get_orders_completed(strategy.id))
        self.assertTrue(order.id in self.exec_db.get_orders_completed_all()[strategy.id])
        self.assertTrue(order.id not in self.exec_db.get_orders_active(strategy.id))
        self.assertTrue(order.id not in self.exec_db.get_orders_active_all()[strategy.id])

    def test_can_make_position_active(self):
        # Arrange
        strategy = TradingStrategy(id_tag_strategy='001')
        position_id = PositionId('AUDUSD-1-123456')
        position = Position(
            symbol=AUDUSD_FXCM,
            position_id=position_id,
            timestamp=UNIX_EPOCH)

        self.exec_db.add_strategy(strategy)

        # Act
        self.exec_db.add_position(position, strategy.id)

        # Assert
        self.assertTrue(self.exec_db.position_exists(position_id))
        self.assertTrue(position.id in self.exec_db.get_position_ids())
        self.assertTrue(position.id in self.exec_db.get_positions_all())
        self.assertTrue(position.id in self.exec_db.get_positions_active(strategy.id))
        self.assertTrue(position.id in self.exec_db.get_positions_active_all()[strategy.id])
        self.assertTrue(position.id not in self.exec_db.get_positions_closed(strategy.id))
        self.assertTrue(position.id not in self.exec_db.get_positions_closed_all()[strategy.id])

    def test_can_make_position_closed(self):
        # Arrange
        strategy = TradingStrategy(id_tag_strategy='001')
        position_id = PositionId('AUDUSD-1-123456')
        position = Position(
            symbol=AUDUSD_FXCM,
            position_id=position_id,
            timestamp=UNIX_EPOCH)

        self.exec_db.add_strategy(strategy)
        self.exec_db.add_position(position, strategy.id)

        # Act
        self.exec_db.position_closed(position, strategy.id)

        # Assert
        self.assertTrue(self.exec_db.position_exists(position_id))
        self.assertTrue(position.id in self.exec_db.get_position_ids())
        self.assertTrue(position.id in self.exec_db.get_positions_all())
        self.assertTrue(position.id in self.exec_db.get_positions_closed(strategy.id))
        self.assertTrue(position.id in self.exec_db.get_positions_closed_all()[strategy.id])
        self.assertTrue(position.id not in self.exec_db.get_positions_active(strategy.id))
        self.assertTrue(position.id not in self.exec_db.get_positions_active_all()[strategy.id])

    def test_position_exists_when_no_position_returns_false(self):
        # Arrange
        # Act
        # Assert
        self.assertFalse(self.exec_db.position_exists(PositionId('unknown')))

    def test_order_exists_when_no_order_returns_false(self):
        # Arrange
        # Act
        # Assert
        self.assertFalse(self.exec_db.order_exists(OrderId('unknown')))

    def test_position_for_order_when_not_found_returns_none(self):
        # Arrange
        # Act
        # Assert
        self.assertIsNone(self.exec_db.get_position_for_order(OrderId('unknown')))

    def test_get_order_when_no_order_returns_none(self):
        # Arrange
        position_id = PositionId('AUDUSD.FXCM-1-123456')

        # Act
        result = self.exec_db.get_position(position_id)

        # Assert
        self.assertIsNone(result)

    def test_get_position_when_no_position_returns_none(self):
        # Arrange
        order_id = OrderId('O-201908080101-000-001')

        # Act
        result = self.exec_db.get_order(order_id)

        # Assert
        self.assertIsNone(result)


class ExecutionEngineTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.clock = TestClock()
        self.guid_factory = TestGuidFactory()
        logger = TestLogger()

        self.trader_id = TraderId('000')
        id_tag_trader = IdTag('000')
        id_tag_strategy = IdTag('001')

        self.order_factory = OrderFactory(
            id_tag_trader=id_tag_trader,
            id_tag_strategy=id_tag_strategy,
            clock=self.clock)

        self.account = Account()
        self.portfolio = Portfolio(
            clock=self.clock,
            guid_factory=self.guid_factory,
            logger=logger)

        self.exec_db = InMemoryExecutionDatabase(trader_id=self.trader_id, logger=logger)
        self.exec_engine = ExecutionEngine(
            database=self.exec_db,
            account=self.account,
            portfolio=self.portfolio,
            clock=self.clock,
            guid_factory=self.guid_factory,
            logger=logger)

        self.exec_client = DummyExecutionClient(self.exec_engine, logger)
        self.exec_engine.register_client(self.exec_client)

    def test_can_register_strategy(self):
        # Arrange
        strategy = TradingStrategy(id_tag_strategy='001')

        # Act
        self.exec_engine.register_strategy(strategy)

        # Assert
        self.assertTrue(strategy.id in self.exec_engine.registered_strategies())

    def test_can_deregister_strategy(self):
        # Arrange
        strategy = TradingStrategy(id_tag_strategy='001')
        self.exec_engine.register_strategy(strategy)

        # Act
        self.exec_engine.deregister_strategy(strategy)

        # Assert
        self.assertTrue(strategy.id not in self.exec_engine.registered_strategies())

    def test_is_flat_when_strategy_registered_returns_true(self):
        # Arrange
        strategy = TradingStrategy(id_tag_strategy='001')

        # Act
        self.exec_engine.register_strategy(strategy)

        # Assert
        self.assertTrue(self.exec_engine.is_strategy_flat(strategy.id))
        self.assertTrue(self.exec_engine.is_flat())

    def test_is_flat_when_no_registered_strategies_returns_true(self):
        # Arrange
        # Act
        # Assert
        self.assertTrue(self.exec_engine.is_flat())

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


        self.exec_engine.register_order(order_id, position_id)
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
        position_id1 = strategy1.position_id_generator.generate()
        position_id2 = strategy2.position_id_generator.generate()

        self.exec_engine.register_strategy(strategy1)
        self.exec_engine.register_strategy(strategy2)

        order1 = strategy1.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        order2 = strategy1.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.SELL,
            Quantity(100000))

        order3 = strategy2.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        submit_order1 = SubmitOrder(
            self.trader_id,
            strategy1.id,
            position_id1,
            order1,
            self.guid_factory.generate(),
            self.clock.time_now())

        submit_order2 = SubmitOrder(
            self.trader_id,
            strategy1.id,
            position_id1,
            order2,
            self.guid_factory.generate(),
            self.clock.time_now())

        submit_order3 = SubmitOrder(
            self.trader_id,
            strategy2.id,
            position_id2,
            order3,
            self.guid_factory.generate(),
            self.clock.time_now())

        buy1 = OrderFilled(
            order1.id,
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
            order2.id,
            ExecutionId('E2'),
            ExecutionTicket('T2'),
            AUDUSD_FXCM,
            OrderSide.SELL,
            Quantity(100000),
            Price('1.00000'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        sell1 = OrderFilled(
            order3.id,
            ExecutionId('E3'),
            ExecutionTicket('T3'),
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price('1.00000'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        self.exec_engine.execute_command(submit_order1)
        self.exec_engine.execute_command(submit_order2)
        self.exec_engine.execute_command(submit_order3)
        self.exec_engine.handle_event(buy1)
        self.exec_engine.handle_event(buy2)
        self.exec_engine.handle_event(sell1)

        # Assert
        # Already tested .is_position_active and .is_position_closed above
        self.assertTrue(self.exec_db.position_exists(position_id1))
        self.assertTrue(self.exec_db.position_exists(position_id2))
        self.assertTrue(self.exec_engine.is_strategy_flat(strategy1.id))
        self.assertFalse(self.exec_engine.is_strategy_flat(strategy2.id))
        self.assertFalse(self.exec_engine.is_flat())
        self.assertTrue(position_id1 in self.exec_db.get_positions(strategy1.id))
        self.assertTrue(position_id2 in self.exec_db.get_positions(strategy2.id))
        self.assertTrue(position_id1 in self.exec_db.get_positions_all())
        self.assertTrue(position_id2 in self.exec_db.get_positions_all())
        self.assertEqual(0, len(self.exec_db.get_positions_active(strategy1.id)))
        self.assertEqual(1, len(self.exec_db.get_positions_active(strategy2.id)))
        self.assertEqual(0, len(self.exec_db.get_positions_active_all()[strategy1.id]))
        self.assertEqual(1, len(self.exec_db.get_positions_active_all()[strategy2.id]))
        self.assertTrue(position_id1 not in self.exec_db.get_positions_active(strategy1.id))
        self.assertTrue(position_id2 in self.exec_db.get_positions_active(strategy2.id))
        self.assertTrue(position_id1 not in self.exec_db.get_positions_active_all()[strategy1.id])
        self.assertTrue(position_id2 in self.exec_db.get_positions_active_all()[strategy2.id])
        self.assertTrue(position_id1 in self.exec_db.get_positions_closed(strategy1.id))
        self.assertTrue(position_id2 not in self.exec_db.get_positions_closed(strategy2.id))
        self.assertTrue(position_id1 in self.exec_db.get_positions_closed_all()[strategy1.id])
        self.assertTrue(position_id2 not in self.exec_db.get_positions_closed_all()[strategy2.id])
        self.assertEqual(2, self.exec_db.positions_count())
        self.assertEqual(1, self.exec_db.positions_active_count())
        self.assertEqual(1, self.exec_db.positions_closed_count())
