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
        guid_factory = TestGuidFactory()
        logger = TestLogger()

        self.trader_id = TraderId('TESTER', '000')

        self.order_factory = OrderFactory(
            id_tag_trader=self.trader_id.order_id_tag,
            id_tag_strategy=IdTag('001'),
            clock=clock)

        self.account = Account()
        self.database = InMemoryExecutionDatabase(trader_id=self.trader_id, logger=logger)

    def test_can_add_strategy(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag='001')

        # Act
        self.database.add_strategy(strategy)

        # Assert
        self.assertTrue(strategy.id in self.database.get_strategy_ids())

    def test_can_add_order(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        strategy_id = StrategyId('SCALPER', '001')
        position_id = PositionId('AUDUSD-1-123456')

        # Act
        self.database.add_order(order, strategy_id, position_id)

        print(self.database.get_order_ids())
        # Assert
        self.assertTrue(order.id in self.database.get_order_ids())
        self.assertEqual(order, self.database.get_orders()[order.id])

    def test_can_add_position(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag='001')
        order = strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        position_id = strategy.position_id_generator.generate()

        order_filled = OrderFilled(
            order.id,
            self.account.id,
            ExecutionId('E123456'),
            ExecutionTicket('T123456'),
            order.symbol,
            order.side,
            order.quantity,
            Price('1.00001'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        position = Position(position_id, order_filled)

        self.database.add_strategy(strategy)

        # Act
        self.database.add_position(position, strategy.id)

        # Assert
        self.assertTrue(position.id in self.database.get_position_ids())
        self.assertTrue(position.id in self.database.get_positions_open(strategy.id))

    def test_can_delete_strategy(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag='001')
        self.database.add_strategy(strategy)

        # Act
        self.database.delete_strategy(strategy)

        # Assert
        self.assertTrue(strategy.id not in self.database.get_strategy_ids())

    def test_can_add_order_event_with_working_order(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag='001')
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        position_id = PositionId('AUDUSD-1-123456')

        self.database.add_strategy(strategy)
        self.database.add_order(order, strategy.id, position_id)

        order_working = OrderWorking(
            order.id,
            OrderId('SOME_BROKER_ID_1'),
            self.account.id,
            order.symbol,
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

        order.apply(order_working)

        # Act
        self.database.update_order(order)

        # Assert
        self.assertTrue(self.database.order_exists(order.id))
        self.assertTrue(order.id in self.database.get_order_ids())
        self.assertTrue(order.id in self.database.get_orders())
        self.assertTrue(order.id in self.database.get_orders_working(strategy.id))
        self.assertTrue(order.id in self.database.get_orders_working())
        self.assertTrue(order.id not in self.database.get_orders_completed(strategy.id))
        self.assertTrue(order.id not in self.database.get_orders_completed())

    def test_can_add_order_event_with_completed_order(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag='001')
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        position_id = PositionId('AUDUSD-1-123456')

        order_filled = OrderFilled(
            order.id,
            self.account.id,
            ExecutionId('E123456'),
            ExecutionTicket('T123456'),
            order.symbol,
            order.side,
            order.quantity,
            Price('1.00001'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        order.apply(order_filled)

        self.database.add_strategy(strategy)
        self.database.add_order(order, strategy.id, position_id)

        # Act
        self.database.update_order(order)

        # Assert
        self.assertTrue(self.database.order_exists(order.id))
        self.assertTrue(order.id in self.database.get_order_ids())
        self.assertTrue(order.id in self.database.get_orders())
        self.assertTrue(order.id in self.database.get_orders_completed(strategy.id))
        self.assertTrue(order.id in self.database.get_orders_completed())
        self.assertTrue(order.id not in self.database.get_orders_working(strategy.id))
        self.assertTrue(order.id not in self.database.get_orders_working())

    def test_can_add_position_event_with_open_position(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag='001')
        self.database.add_strategy(strategy)

        position = TestStubs.position()

        # Act
        self.database.add_position(position, strategy.id)

        # Assert
        self.assertTrue(self.database.position_exists(position.id))
        self.assertTrue(position.id in self.database.get_position_ids())
        self.assertTrue(position.id in self.database.get_positions())
        self.assertTrue(position.id in self.database.get_positions_open(strategy.id))
        self.assertTrue(position.id in self.database.get_positions_open())
        self.assertTrue(position.id not in self.database.get_positions_closed(strategy.id))
        self.assertTrue(position.id not in self.database.get_positions_closed())

    def test_can_add_position_event_with_closed_position(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag='001')
        self.database.add_strategy(strategy)

        position = TestStubs.position()

        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.SELL,
            Quantity(100000))

        order_filled = OrderFilled(
            order.id,
            self.account.id,
            ExecutionId('E123456'),
            ExecutionTicket('T123456'),
            order.symbol,
            order.side,
            order.quantity,
            Price('1.00001'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        position.apply(order_filled)

        self.database.add_order(order, strategy.id, position.id)
        self.database.add_position(position, strategy.id)

        # Act
        self.database.update_position(position, order_filled)

        # Assert
        self.assertTrue(self.database.position_exists(position.id))
        self.assertTrue(position.id in self.database.get_position_ids())
        self.assertTrue(position.id in self.database.get_positions())
        self.assertTrue(position.id in self.database.get_positions_closed(strategy.id))
        self.assertTrue(position.id in self.database.get_positions_closed())
        self.assertTrue(position.id not in self.database.get_positions_open(strategy.id))
        self.assertTrue(position.id not in self.database.get_positions_open())

    def test_can_add_account_event(self):
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

        order_filled = OrderFilled(
            order.id,
            self.account.id,
            ExecutionId('E-' + order.id.value),
            ExecutionTicket('ET-' + order.id.value),
            order.symbol,
            order.side,
            order.quantity,
            order.price,
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

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
        self.assertEqual(1, self.exec_db.positions_count())
        self.assertEqual(1, self.exec_db.positions_open_count())
        self.assertEqual(0, self.exec_db.positions_closed_count())
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

        order_filled1 = OrderFilled(
            order1.id,
            self.account.id,
            ExecutionId('E-' + order1.id.value),
            ExecutionTicket('ET-' + order1.id.value),
            order1.symbol,
            order1.side,
            order1.quantity,
            order1.price,
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        order_filled2 = OrderFilled(
            order2.id,
            self.account.id,
            ExecutionId('E-' + order1.id.value),
            ExecutionTicket('ET-' + order1.id.value),
            order2.symbol,
            order2.side,
            order2.quantity,
            order2.price,
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        self.exec_engine.handle_event(order_filled1)
        self.exec_engine.handle_event(order_filled2)

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
        self.assertEqual(1, self.exec_db.positions_count())
        self.assertEqual(1, self.exec_db.positions_open_count())
        self.assertEqual(0, self.exec_db.positions_closed_count())

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

        order_filled1 = OrderFilled(
            order1.id,
            self.account.id,
            ExecutionId('E-' + order1.id.value),
            ExecutionTicket('ET-' + order1.id.value),
            order1.symbol,
            order1.side,
            order1.quantity,
            order1.price,
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        order_filled2 = OrderFilled(
            order2.id,
            self.account.id,
            ExecutionId('E-' + order1.id.value),
            ExecutionTicket('ET-' + order1.id.value),
            order2.symbol,
            order2.side,
            order2.quantity,
            order2.price,
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        self.exec_engine.handle_event(order_filled1)
        self.exec_engine.handle_event(order_filled2)

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
        self.assertEqual(1, self.exec_db.positions_count())
        self.assertEqual(0, self.exec_db.positions_open_count())
        self.assertEqual(1, self.exec_db.positions_closed_count())

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

        order1_filled = OrderFilled(
            order1.id,
            self.account.id,
            ExecutionId('E-' + order1.id.value),
            ExecutionTicket('ET-' + order1.id.value),
            order1.symbol,
            order1.side,
            order1.quantity,
            order1.price,
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        order2_filled = OrderFilled(
            order2.id,
            self.account.id,
            ExecutionId('E-' + order2.id.value),
            ExecutionTicket('ET-' + order2.id.value),
            order2.symbol,
            order2.side,
            order2.quantity,
            order2.price,
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

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
        self.assertEqual(2, self.exec_db.positions_count())
        self.assertEqual(2, self.exec_db.positions_open_count())
        self.assertEqual(0, self.exec_db.positions_closed_count())

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

        order1_filled = OrderFilled(
            order1.id,
            self.account.id,
            ExecutionId('E1'),
            ExecutionTicket('T1'),
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price('1.00000'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        order2_filled = OrderFilled(
            order2.id,
            self.account.id,
            ExecutionId('E2'),
            ExecutionTicket('T2'),
            AUDUSD_FXCM,
            OrderSide.SELL,
            Quantity(100000),
            Price('1.00000'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

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
        self.assertEqual(0, len(self.exec_db.get_positions_open()))
        self.assertEqual(1, len(self.exec_db.get_positions_open()))
        self.assertTrue(position_id1 not in self.exec_db.get_positions_open(strategy1.id))
        self.assertTrue(position_id2 in self.exec_db.get_positions_open(strategy2.id))
        self.assertTrue(position_id1 not in self.exec_db.get_positions_open())
        self.assertTrue(position_id2 in self.exec_db.get_positions_open())
        self.assertTrue(position_id1 in self.exec_db.get_positions_closed(strategy1.id))
        self.assertTrue(position_id2 not in self.exec_db.get_positions_closed(strategy2.id))
        self.assertTrue(position_id1 in self.exec_db.get_positions_closed())
        self.assertTrue(position_id2 not in self.exec_db.get_positions_closed())
        self.assertEqual(2, self.exec_db.positions_count())
        self.assertEqual(1, self.exec_db.positions_open_count())
        self.assertEqual(1, self.exec_db.positions_closed_count())
