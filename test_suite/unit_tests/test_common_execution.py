# -------------------------------------------------------------------------------------------------
# <copyright file="test_common_execution.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest
import uuid

from decimal import Decimal

from nautilus_trader.core.types import GUID, ValidString
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.events import OrderFilled, OrderWorking
from nautilus_trader.model.identifiers import (
    AccountId,
    TraderId,
    StrategyId,
    IdTag,
    OrderId,
    PositionId,
    ExecutionId,
    ExecutionTicket)
from nautilus_trader.model.objects import Quantity, Price, Money
from nautilus_trader.model.order import OrderFactory
from nautilus_trader.model.position import Position
from nautilus_trader.model.commands import SubmitOrder
from nautilus_trader.model.events import AccountStateEvent
from nautilus_trader.model.enums import Currency
from nautilus_trader.common.account import Account
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.guid import TestGuidFactory
from nautilus_trader.common.logger import TestLogger
from nautilus_trader.common.portfolio import Portfolio
from nautilus_trader.common.execution import InMemoryExecutionDatabase, ExecutionEngine
from nautilus_trader.trade.strategy import TradingStrategy
from test_kit.stubs import TestStubs
from test_kit.mocks import MockExecutionClient

UNIX_EPOCH = TestStubs.unix_epoch()
AUDUSD_FXCM = TestStubs.symbol_audusd_fxcm()
GBPUSD_FXCM = TestStubs.symbol_gbpusd_fxcm()


class InMemoryExecutionDatabaseTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        clock = TestClock()
        logger = TestLogger()

        self.trader_id = TraderId('TESTER', '000')

        self.strategy = TradingStrategy(order_id_tag='001')
        self.strategy.change_clock(clock)
        self.strategy.change_logger(logger)

        self.account = Account()
        self.database = InMemoryExecutionDatabase(trader_id=self.trader_id, logger=logger)

    def test_can_add_strategy(self):
        # Arrange
        # Act
        self.database.add_strategy(self.strategy)

        # Assert
        self.assertTrue(self.strategy.id in self.database.get_strategy_ids())

    def test_can_add_order(self):
        # Arrange
        self.database.add_strategy(self.strategy)
        order = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        position_id = self.strategy.position_id_generator.generate()

        # Act
        self.database.add_order(order, self.strategy.id, position_id)

        # Assert
        self.assertTrue(order.id in self.database.get_order_ids())
        self.assertEqual(order, self.database.get_orders()[order.id])

    def test_can_add_position(self):
        # Arrange
        self.database.add_strategy(self.strategy)
        order = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        position_id = self.strategy.position_id_generator.generate()
        self.database.add_order(order, self.strategy.id, position_id)

        order_filled = TestStubs.event_order_filled(order, fill_price=Price('1.00000'))
        position = Position(position_id, order_filled)

        # Act
        self.database.add_position(position, self.strategy.id)

        # Assert
        self.assertTrue(self.database.position_exists_for_order(order.id))
        self.assertTrue(self.database.position_exists(position.id))
        self.assertTrue(position.id in self.database.get_position_ids())
        self.assertTrue(position.id in self.database.get_positions())
        self.assertTrue(position.id in self.database.get_positions_open(self.strategy.id))
        self.assertTrue(position.id in self.database.get_positions_open())
        self.assertTrue(position.id not in self.database.get_positions_closed(self.strategy.id))
        self.assertTrue(position.id not in self.database.get_positions_closed())

    def test_can_update_order_for_working_order(self):
        # Arrange
        self.database.add_strategy(self.strategy)
        order = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        position_id = self.strategy.position_id_generator.generate()
        self.database.add_order(order, self.strategy.id, position_id)

        order_working = TestStubs.event_order_working(order)
        order.apply(order_working)

        # Act
        self.database.update_order(order)

        # Assert
        self.assertTrue(self.database.order_exists(order.id))
        self.assertTrue(order.id in self.database.get_order_ids())
        self.assertTrue(order.id in self.database.get_orders())
        self.assertTrue(order.id in self.database.get_orders_working(self.strategy.id))
        self.assertTrue(order.id in self.database.get_orders_working())
        self.assertTrue(order.id not in self.database.get_orders_completed(self.strategy.id))
        self.assertTrue(order.id not in self.database.get_orders_completed())

    def test_can_update_order_for_completed_order(self):
        # Arrange
        self.database.add_strategy(self.strategy)
        order = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        position_id = self.strategy.position_id_generator.generate()
        self.database.add_order(order, self.strategy.id, position_id)

        order_filled = TestStubs.event_order_filled(order, fill_price=Price('1.00001'))
        order.apply(order_filled)

        # Act
        self.database.update_order(order)

        # Assert
        self.assertTrue(self.database.order_exists(order.id))
        self.assertTrue(order.id in self.database.get_order_ids())
        self.assertTrue(order.id in self.database.get_orders())
        self.assertTrue(order.id in self.database.get_orders_completed(self.strategy.id))
        self.assertTrue(order.id in self.database.get_orders_completed())
        self.assertTrue(order.id not in self.database.get_orders_working(self.strategy.id))
        self.assertTrue(order.id not in self.database.get_orders_working())

    def test_can_update_position_for_closed_position(self):
        # Arrange
        self.database.add_strategy(self.strategy)
        order1 = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        position_id = self.strategy.position_id_generator.generate()
        self.database.add_order(order1, self.strategy.id, position_id)

        order1_filled = TestStubs.event_order_filled(order1, fill_price=Price('1.00001'))
        order1.apply(order1_filled)
        position = Position(position_id, order1.last_event)
        self.database.add_position(position, self.strategy.id)

        order2 = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.SELL,
            Quantity(100000))
        order2_filled = TestStubs.event_order_filled(order2, fill_price=Price('1.00001'))
        position.apply(order2_filled)

        # Act
        self.database.update_position(position)

        # Assert
        self.assertTrue(self.database.position_exists(position.id))
        self.assertTrue(position.id in self.database.get_position_ids())
        self.assertTrue(position.id in self.database.get_positions())
        self.assertTrue(position.id in self.database.get_positions_closed(self.strategy.id))
        self.assertTrue(position.id in self.database.get_positions_closed())
        self.assertTrue(position.id not in self.database.get_positions_open(self.strategy.id))
        self.assertTrue(position.id not in self.database.get_positions_open())
        self.assertEqual(position, self.database.get_position_for_order(order1.id))

    def test_can_update_account(self):
        # Arrange
        account = Account()
        event = AccountStateEvent(
            AccountId('SIMULATED', '123456'),
            Currency.USD,
            Money(1000000),
            Money(1000000),
            Money.zero(),
            Money.zero(),
            Money.zero(),
            Decimal(0),
            ValidString(),
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        account.apply(event)

        # Act
        self.database.update_account(account)

        # Assert
        self.assertTrue(True)  # Did not raise exception

    def test_can_delete_strategy(self):
        # Arrange
        self.database.add_strategy(self.strategy)

        # Act
        self.database.delete_strategy(self.strategy)

        # Assert
        self.assertTrue(self.strategy.id not in self.database.get_strategy_ids())

    def test_can_check_residuals(self):
        # Arrange
        self.database.add_strategy(self.strategy)
        order1 = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        position1_id = self.strategy.position_id_generator.generate()
        self.database.add_order(order1, self.strategy.id, position1_id)

        order1_filled = TestStubs.event_order_filled(order1, fill_price=Price('1.00000'))
        position1 = Position(position1_id, order1_filled)
        self.database.update_order(order1)
        self.database.add_position(position1, self.strategy.id)

        order2 = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        position2_id = self.strategy.position_id_generator.generate()
        self.database.add_order(order2, self.strategy.id, position2_id)
        order2_working = TestStubs.event_order_working(order2)
        order2.apply(order2_working)

        self.database.update_order(order2)

        # Act
        self.database.check_residuals()

        # Does not raise exception

    def test_can_reset(self):
        # Arrange
        self.database.add_strategy(self.strategy)
        order1 = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        position1_id = self.strategy.position_id_generator.generate()
        self.database.add_order(order1, self.strategy.id, position1_id)

        order1_filled = TestStubs.event_order_filled(order1, fill_price=Price('1.00000'))
        position1 = Position(position1_id, order1_filled)
        self.database.update_order(order1)
        self.database.add_position(position1, self.strategy.id)

        order2 = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        position2_id = self.strategy.position_id_generator.generate()
        self.database.add_order(order2, self.strategy.id, position2_id)
        order2_working = TestStubs.event_order_working(order2)
        order2.apply(order2_working)

        self.database.update_order(order2)

        # Act
        self.database.reset()

        # Assert
        self.assertEqual(0, len(self.database.get_strategy_ids()))
        self.assertEqual(0, self.database.count_orders_total())
        self.assertEqual(0, self.database.count_positions_total())

    def test_can_flush(self):
        # Arrange
        self.database.add_strategy(self.strategy)
        order1 = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        position1_id = self.strategy.position_id_generator.generate()
        self.database.add_order(order1, self.strategy.id, position1_id)

        order1_filled = TestStubs.event_order_filled(order1, fill_price=Price('1.00000'))
        position1 = Position(position1_id, order1_filled)
        self.database.update_order(order1)
        self.database.add_position(position1, self.strategy.id)

        order2 = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        position2_id = self.strategy.position_id_generator.generate()
        self.database.add_order(order2, self.strategy.id, position2_id)
        order2_working = TestStubs.event_order_working(order2)
        order2.apply(order2_working)

        self.database.update_order(order2)

        # Act
        self.database.reset()
        self.database.flush()

        # Assert
        # Does not raise exception

    def test_get_strategy_ids_with_no_ids_returns_empty_set(self):
        # Arrange
        # Act
        result = self.database.get_strategy_ids()

        # Assert
        self.assertEqual(set(), result)

    def test_get_strategy_ids_with_id_returns_correct_set(self):
        # Arrange
        self.database.add_strategy(self.strategy)

        # Act
        result = self.database.get_strategy_ids()

        # Assert
        self.assertEqual({self.strategy.id}, result)

    def test_position_exists_when_no_position_returns_false(self):
        # Arrange
        # Act
        # Assert
        self.assertFalse(self.database.position_exists(PositionId('unknown')))

    def test_order_exists_when_no_order_returns_false(self):
        # Arrange
        # Act
        # Assert
        self.assertFalse(self.database.order_exists(OrderId('unknown')))

    def test_position_for_order_when_not_found_returns_none(self):
        # Arrange
        # Act
        # Assert
        self.assertIsNone(self.database.get_position_for_order(OrderId('unknown')))

    def test_position_indexed_for_order_when_no_indexing_returns_false(self):
        # Arrange
        # Act
        # Assert
        self.assertFalse(self.database.position_indexed_for_order(OrderId('unknown')))

    def test_get_order_when_no_order_returns_none(self):
        # Arrange
        position_id = PositionId('AUDUSD.FXCM-1-123456')

        # Act
        result = self.database.get_position(position_id)

        # Assert
        self.assertIsNone(result)

    def test_get_position_when_no_position_returns_none(self):
        # Arrange
        order_id = OrderId('O-201908080101-000-001')

        # Act
        result = self.database.get_order(order_id)

        # Assert
        self.assertIsNone(result)


class ExecutionEngineTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.clock = TestClock()
        self.guid_factory = TestGuidFactory()
        logger = TestLogger()

        self.trader_id = TraderId('TESTER', '000')

        self.order_factory = OrderFactory(
            id_tag_trader=self.trader_id.order_id_tag,
            id_tag_strategy=IdTag('001'),
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

        self.exec_client = MockExecutionClient(self.exec_engine, logger)
        self.exec_engine.register_client(self.exec_client)

    def test_can_register_strategy(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag='001')

        # Act
        self.exec_engine.register_strategy(strategy)

        # Assert
        self.assertTrue(strategy.id in self.exec_engine.registered_strategies())

    def test_can_deregister_strategy(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag='001')
        self.exec_engine.register_strategy(strategy)

        # Act
        self.exec_engine.deregister_strategy(strategy)

        # Assert
        self.assertTrue(strategy.id not in self.exec_engine.registered_strategies())

    def test_is_flat_when_strategy_registered_returns_true(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag='001')

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

    def test_can_reset_execution_engine(self):
        strategy = TradingStrategy(order_id_tag='001')

        self.exec_engine.register_strategy(strategy)  # Also registers with portfolio

        # Act
        self.exec_engine.reset()

        # Assert
        self.assertTrue(strategy.id in self.exec_engine.registered_strategies())

    def test_can_submit_order(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag='001')
        strategy.change_clock(self.clock)

        self.exec_engine.register_strategy(strategy)

        position_id = strategy.position_id_generator.generate()
        order = strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        submit_order = SubmitOrder(
            self.trader_id,
            strategy.id,
            position_id,
            self.account.id,
            order,
            self.guid_factory.generate(),
            self.clock.time_now())

        # Act
        self.exec_engine.execute_command(submit_order)

        # Assert
        self.assertIn(submit_order, self.exec_client.received_commands)
        self.assertTrue(self.exec_db.order_exists(order.id))
        self.assertEqual(position_id, self.exec_db.get_position_id(order.id))

    def test_can_handle_order_fill_event(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag='001')
        strategy.change_clock(self.clock)

        self.exec_engine.register_strategy(strategy)

        position_id = strategy.position_id_generator.generate()
        order = strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        submit_order = SubmitOrder(
            self.trader_id,
            strategy.id,
            position_id,
            self.account.id,
            order,
            self.guid_factory.generate(),
            self.clock.time_now())

        self.exec_engine.execute_command(submit_order)

        order_filled = TestStubs.event_order_filled(order)

        # Act
        self.exec_engine.handle_event(order_filled)

        # Assert
        self.assertTrue(self.exec_db.position_exists(position_id))
        self.assertTrue(self.exec_db.is_position_open(position_id))
        self.assertFalse(self.exec_db.is_position_closed(position_id))
        self.assertFalse(self.exec_engine.is_strategy_flat(strategy.id))
        self.assertFalse(self.exec_engine.is_flat())
        self.assertEqual(Position, type(self.exec_db.get_position(position_id)))
        self.assertTrue(position_id in self.exec_db.get_positions())
        self.assertTrue(position_id not in self.exec_db.get_positions_closed(strategy.id))
        self.assertTrue(position_id not in self.exec_db.get_positions_closed())
        self.assertTrue(position_id in self.exec_db.get_positions_open(strategy.id))
        self.assertTrue(position_id in self.exec_db.get_positions_open())
        self.assertEqual(1, self.exec_db.count_positions_total())
        self.assertEqual(1, self.exec_db.count_positions_open())
        self.assertEqual(0, self.exec_db.count_positions_closed())
        self.assertTrue(self.exec_db.position_exists_for_order(order.id))
        self.assertEqual(Position, type(self.exec_db.get_position_for_order(order.id)))

    def test_can_add_to_existing_position_on_order_fill(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag='001')
        strategy.change_clock(self.clock)

        self.exec_engine.register_strategy(strategy)

        position_id = strategy.position_id_generator.generate()
        order1 = strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        order2 = strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        submit_order1 = SubmitOrder(
            self.trader_id,
            strategy.id,
            position_id,
            self.account.id,
            order1,
            self.guid_factory.generate(),
            self.clock.time_now())

        submit_order2 = SubmitOrder(
            self.trader_id,
            strategy.id,
            position_id,
            self.account.id,
            order2,
            self.guid_factory.generate(),
            self.clock.time_now())

        self.exec_engine.execute_command(submit_order1)
        self.exec_engine.execute_command(submit_order2)

        order1_filled = TestStubs.event_order_filled(order1)
        order2_filled = TestStubs.event_order_filled(order2)

        # Act
        self.exec_engine.handle_event(order1_filled)
        self.exec_engine.handle_event(order2_filled)

        # Assert
        self.assertTrue(self.exec_db.position_exists(position_id))
        self.assertTrue(self.exec_db.is_position_open(position_id))
        self.assertFalse(self.exec_db.is_position_closed(position_id))
        self.assertFalse(self.exec_engine.is_strategy_flat(strategy.id))
        self.assertFalse(self.exec_engine.is_flat())
        self.assertEqual(Position, type(self.exec_db.get_position(position_id)))
        self.assertEqual(0, len(self.exec_db.get_positions_closed(strategy.id)))
        self.assertEqual(0, len(self.exec_db.get_positions_closed()))
        self.assertEqual(1, len(self.exec_db.get_positions_open(strategy.id)))
        self.assertEqual(1, len(self.exec_db.get_positions_open()))
        self.assertEqual(1, self.exec_db.count_positions_total())
        self.assertEqual(1, self.exec_db.count_positions_open())
        self.assertEqual(0, self.exec_db.count_positions_closed())

    def test_can_close_position_on_order_fill(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag='001')
        strategy.change_clock(self.clock)

        self.exec_engine.register_strategy(strategy)

        position_id = strategy.position_id_generator.generate()

        order1 = strategy.order_factory.stop_market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price('1.00000'))

        order2 = strategy.order_factory.stop_market(
            AUDUSD_FXCM,
            OrderSide.SELL,
            Quantity(100000),
            Price('1.00000'))

        submit_order1 = SubmitOrder(
            self.trader_id,
            strategy.id,
            position_id,
            self.account.id,
            order1,
            self.guid_factory.generate(),
            self.clock.time_now())

        submit_order2 = SubmitOrder(
            self.trader_id,
            strategy.id,
            position_id,
            self.account.id,
            order2,
            self.guid_factory.generate(),
            self.clock.time_now())

        self.exec_engine.execute_command(submit_order1)
        self.exec_engine.execute_command(submit_order2)

        order1_filled = TestStubs.event_order_filled(order1)
        order2_filled = TestStubs.event_order_filled(order2)

        # Act
        self.exec_engine.handle_event(order1_filled)
        self.exec_engine.handle_event(order2_filled)

        # Assert
        self.assertTrue(self.exec_db.position_exists(position_id))
        self.assertFalse(self.exec_db.is_position_open(position_id))
        self.assertTrue(self.exec_db.is_position_closed(position_id))
        self.assertTrue(self.exec_engine.is_strategy_flat(strategy.id))
        self.assertTrue(self.exec_engine.is_flat())
        self.assertEqual(position_id, self.exec_db.get_position(position_id).id)
        self.assertTrue(position_id in self.exec_db.get_positions(strategy.id))
        self.assertTrue(position_id in self.exec_db.get_positions())
        self.assertEqual(0, len(self.exec_db.get_positions_open(strategy.id)))
        self.assertEqual(0, len(self.exec_db.get_positions_open()))
        self.assertTrue(position_id in self.exec_db.get_positions_closed(strategy.id))
        self.assertTrue(position_id in self.exec_db.get_positions_closed())
        self.assertTrue(position_id not in self.exec_db.get_positions_open(strategy.id))
        self.assertTrue(position_id not in self.exec_db.get_positions_open())
        self.assertEqual(1, self.exec_db.count_positions_total())
        self.assertEqual(0, self.exec_db.count_positions_open())
        self.assertEqual(1, self.exec_db.count_positions_closed())

    def test_multiple_strategy_positions_opened(self):
        # Arrange
        strategy1 = TradingStrategy(order_id_tag='001')
        strategy2 = TradingStrategy(order_id_tag='002')
        position_id1 = strategy1.position_id_generator.generate()
        position_id2 = strategy2.position_id_generator.generate()

        self.exec_engine.register_strategy(strategy1)
        self.exec_engine.register_strategy(strategy2)

        order1 = strategy1.order_factory.stop_market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price('1.00000'))

        order2 = strategy2.order_factory.stop_market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price('1.00000'))

        submit_order1 = SubmitOrder(
            self.trader_id,
            strategy1.id,
            position_id1,
            self.account.id,
            order1,
            self.guid_factory.generate(),
            self.clock.time_now())

        submit_order2 = SubmitOrder(
            self.trader_id,
            strategy2.id,
            position_id2,
            self.account.id,
            order2,
            self.guid_factory.generate(),
            self.clock.time_now())

        order1_filled = TestStubs.event_order_filled(order1)
        order2_filled = TestStubs.event_order_filled(order2)

        # Act
        self.exec_engine.execute_command(submit_order1)
        self.exec_engine.execute_command(submit_order2)
        self.exec_engine.handle_event(order1_filled)
        self.exec_engine.handle_event(order2_filled)

        # Assert
        self.assertTrue(self.exec_db.position_exists(position_id1))
        self.assertTrue(self.exec_db.position_exists(position_id2))
        self.assertTrue(self.exec_db.is_position_open(position_id1))
        self.assertTrue(self.exec_db.is_position_open(position_id2))
        self.assertFalse(self.exec_db.is_position_closed(position_id1))
        self.assertFalse(self.exec_db.is_position_closed(position_id2))
        self.assertFalse(self.exec_engine.is_strategy_flat(strategy1.id))
        self.assertFalse(self.exec_engine.is_strategy_flat(strategy2.id))
        self.assertFalse(self.exec_engine.is_flat())
        self.assertEqual(Position, type(self.exec_db.get_position(position_id1)))
        self.assertEqual(Position, type(self.exec_db.get_position(position_id2)))
        self.assertTrue(position_id1 in self.exec_db.get_positions(strategy1.id))
        self.assertTrue(position_id2 in self.exec_db.get_positions(strategy2.id))
        self.assertTrue(position_id1 in self.exec_db.get_positions())
        self.assertTrue(position_id2 in self.exec_db.get_positions())
        self.assertEqual(1, len(self.exec_db.get_positions_open(strategy1.id)))
        self.assertEqual(1, len(self.exec_db.get_positions_open(strategy2.id)))
        self.assertEqual(2, len(self.exec_db.get_positions_open()))
        self.assertEqual(1, len(self.exec_db.get_positions_open(strategy1.id)))
        self.assertEqual(1, len(self.exec_db.get_positions_open(strategy2.id)))
        self.assertTrue(position_id1 in self.exec_db.get_positions_open(strategy1.id))
        self.assertTrue(position_id2 in self.exec_db.get_positions_open(strategy2.id))
        self.assertTrue(position_id1 in self.exec_db.get_positions_open())
        self.assertTrue(position_id2 in self.exec_db.get_positions_open())
        self.assertTrue(position_id1 not in self.exec_db.get_positions_closed(strategy1.id))
        self.assertTrue(position_id2 not in self.exec_db.get_positions_closed(strategy2.id))
        self.assertTrue(position_id1 not in self.exec_db.get_positions_closed())
        self.assertTrue(position_id2 not in self.exec_db.get_positions_closed())
        self.assertEqual(2, self.exec_db.count_positions_total())
        self.assertEqual(2, self.exec_db.count_positions_open())
        self.assertEqual(0, self.exec_db.count_positions_closed())

    def test_multiple_strategy_positions_one_active_one_closed(self):
        # Arrange
        strategy1 = TradingStrategy(order_id_tag='001')
        strategy2 = TradingStrategy(order_id_tag='002')
        position_id1 = strategy1.position_id_generator.generate()
        position_id2 = strategy2.position_id_generator.generate()

        self.exec_engine.register_strategy(strategy1)
        self.exec_engine.register_strategy(strategy2)

        order1 = strategy1.order_factory.stop_market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price('1.00000'))

        order2 = strategy1.order_factory.stop_market(
            AUDUSD_FXCM,
            OrderSide.SELL,
            Quantity(100000),
            Price('1.00000'))

        order3 = strategy2.order_factory.stop_market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price('1.00000'))

        submit_order1 = SubmitOrder(
            self.trader_id,
            strategy1.id,
            position_id1,
            self.account.id,
            order1,
            self.guid_factory.generate(),
            self.clock.time_now())

        submit_order2 = SubmitOrder(
            self.trader_id,
            strategy1.id,
            position_id1,
            self.account.id,
            order2,
            self.guid_factory.generate(),
            self.clock.time_now())

        submit_order3 = SubmitOrder(
            self.trader_id,
            strategy2.id,
            position_id2,
            self.account.id,
            order3,
            self.guid_factory.generate(),
            self.clock.time_now())

        order1_filled = TestStubs.event_order_filled(order1)
        order2_filled = TestStubs.event_order_filled(order2)

        order3_filled = OrderFilled(
            order3.id,
            self.account.id,
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
        self.exec_engine.handle_event(order1_filled)
        self.exec_engine.handle_event(order2_filled)
        self.exec_engine.handle_event(order3_filled)

        # Assert
        # Already tested .is_position_active and .is_position_closed above
        self.assertTrue(self.exec_db.position_exists(position_id1))
        self.assertTrue(self.exec_db.position_exists(position_id2))
        self.assertTrue(self.exec_engine.is_strategy_flat(strategy1.id))
        self.assertFalse(self.exec_engine.is_strategy_flat(strategy2.id))
        self.assertFalse(self.exec_engine.is_flat())
        self.assertTrue(position_id1 in self.exec_db.get_positions(strategy1.id))
        self.assertTrue(position_id2 in self.exec_db.get_positions(strategy2.id))
        self.assertTrue(position_id1 in self.exec_db.get_positions())
        self.assertTrue(position_id2 in self.exec_db.get_positions())
        self.assertEqual(0, len(self.exec_db.get_positions_open(strategy1.id)))
        self.assertEqual(1, len(self.exec_db.get_positions_open(strategy2.id)))
        self.assertEqual(1, len(self.exec_db.get_positions_open()))
        self.assertEqual(1, len(self.exec_db.get_positions_closed()))
        self.assertEqual(2, len(self.exec_db.get_positions()))
        self.assertTrue(position_id1 not in self.exec_db.get_positions_open(strategy1.id))
        self.assertTrue(position_id2 in self.exec_db.get_positions_open(strategy2.id))
        self.assertTrue(position_id1 not in self.exec_db.get_positions_open())
        self.assertTrue(position_id2 in self.exec_db.get_positions_open())
        self.assertTrue(position_id1 in self.exec_db.get_positions_closed(strategy1.id))
        self.assertTrue(position_id2 not in self.exec_db.get_positions_closed(strategy2.id))
        self.assertTrue(position_id1 in self.exec_db.get_positions_closed())
        self.assertTrue(position_id2 not in self.exec_db.get_positions_closed())
        self.assertEqual(2, self.exec_db.count_positions_total())
        self.assertEqual(1, self.exec_db.count_positions_open())
        self.assertEqual(1, self.exec_db.count_positions_closed())
