# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

import unittest

from nautilus_trader.backtest.logging import TestLogger
from nautilus_trader.common.account import Account
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.uuid import TestUUIDFactory
from nautilus_trader.core.uuid import uuid4
from nautilus_trader.execution.cache import ExecutionCache
from nautilus_trader.execution.database import BypassExecutionDatabase
from nautilus_trader.model.enums import Currency
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.objects import Decimal
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.position import Position
from nautilus_trader.trading.strategy import TradingStrategy
from tests.test_kit.stubs import TestStubs
from tests.test_kit.stubs import UNIX_EPOCH

AUDUSD_FXCM = TestStubs.symbol_audusd_fxcm()
GBPUSD_FXCM = TestStubs.symbol_gbpusd_fxcm()


class ExecutionCacheTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        clock = TestClock()
        logger = TestLogger(clock)

        self.trader_id = TraderId("TESTER", "000")
        self.account_id = TestStubs.account_id()

        self.strategy = TradingStrategy(order_id_tag="001")
        self.strategy.register_trader(
            TraderId("TESTER", "000"),
            clock=clock,
            uuid_factory=TestUUIDFactory(),
            logger=logger,
        )

        exec_db = BypassExecutionDatabase(trader_id=self.trader_id, logger=logger)
        self.cache = ExecutionCache(database=exec_db, logger=logger)

    def test_add_order(self):
        # Arrange
        order = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        position_id = PositionId('P-1')

        # Act
        self.cache.add_order(order, position_id, self.strategy.id)

        # Assert
        self.assertTrue(order.cl_ord_id in self.cache.order_ids())
        self.assertTrue(order.cl_ord_id in self.cache.order_ids(symbol=order.symbol))
        self.assertTrue(order.cl_ord_id in self.cache.order_ids(strategy_id=self.strategy.id))
        self.assertTrue(order.cl_ord_id in self.cache.order_ids(symbol=order.symbol, strategy_id=self.strategy.id))
        self.assertTrue(order in self.cache.orders())

    def test_add_position(self):
        # Arrange
        order = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        position_id = PositionId('P-1')
        self.cache.add_order(order, position_id, self.strategy.id)

        order_filled = TestStubs.event_order_filled(
            order,
            position_id=PositionId('P-1'),
            fill_price=Price("1.00000"),
        )

        position = Position(order_filled)

        # Act
        self.cache.add_position(position, self.strategy.id)

        # Assert
        self.assertTrue(self.cache.position_exists_for_order(order.cl_ord_id))
        self.assertTrue(self.cache.position_exists(position.id))
        self.assertTrue(position.id in self.cache.position_ids())
        self.assertTrue(position in self.cache.positions())
        self.assertTrue(position in self.cache.positions_open())
        self.assertTrue(position in self.cache.positions_open(symbol=position.symbol))
        self.assertTrue(position in self.cache.positions_open(strategy_id=self.strategy.id))
        self.assertTrue(position in self.cache.positions_open(symbol=position.symbol, strategy_id=self.strategy.id))
        self.assertTrue(position not in self.cache.positions_closed())
        self.assertTrue(position not in self.cache.positions_closed(symbol=position.symbol))
        self.assertTrue(position not in self.cache.positions_closed(strategy_id=self.strategy.id))
        self.assertTrue(position not in self.cache.positions_closed(symbol=position.symbol, strategy_id=self.strategy.id))

    def test_update_order_for_working_order(self):
        # Arrange
        order = self.strategy.order_factory.stop(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price("1.00000"))

        position_id = PositionId('P-1')
        self.cache.add_order(order, position_id, self.strategy.id)

        order.apply(TestStubs.event_order_submitted(order))
        self.cache.update_order(order)

        order.apply(TestStubs.event_order_accepted(order))
        self.cache.update_order(order)

        order.apply(TestStubs.event_order_working(order))

        # Act
        self.cache.update_order(order)

        # Assert
        self.assertTrue(self.cache.order_exists(order.cl_ord_id))
        self.assertTrue(order.cl_ord_id in self.cache.order_ids())
        self.assertTrue(order in self.cache.orders())
        self.assertTrue(order in self.cache.orders_working())
        self.assertTrue(order in self.cache.orders_working(symbol=order.symbol))
        self.assertTrue(order in self.cache.orders_working(strategy_id=self.strategy.id))
        self.assertTrue(order in self.cache.orders_working(symbol=order.symbol, strategy_id=self.strategy.id))
        self.assertTrue(order not in self.cache.orders_completed())
        self.assertTrue(order not in self.cache.orders_completed(symbol=order.symbol))
        self.assertTrue(order not in self.cache.orders_completed(strategy_id=self.strategy.id))
        self.assertTrue(order not in self.cache.orders_completed(symbol=order.symbol, strategy_id=self.strategy.id))

    def test_update_order_for_completed_order(self):
        # Arrange
        order = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        position_id = PositionId('P-1')
        self.cache.add_order(order, position_id, self.strategy.id)
        order.apply(TestStubs.event_order_submitted(order))
        self.cache.update_order(order)

        order.apply(TestStubs.event_order_accepted(order))
        self.cache.update_order(order)

        order.apply(TestStubs.event_order_filled(order, fill_price=Price("1.00001")))

        # Act
        self.cache.update_order(order)

        # Assert
        self.assertTrue(self.cache.order_exists(order.cl_ord_id))
        self.assertTrue(order.cl_ord_id in self.cache.order_ids())
        self.assertTrue(order in self.cache.orders())
        self.assertTrue(order in self.cache.orders_completed())
        self.assertTrue(order in self.cache.orders_completed(symbol=order.symbol))
        self.assertTrue(order in self.cache.orders_completed(strategy_id=self.strategy.id))
        self.assertTrue(order in self.cache.orders_completed(symbol=order.symbol, strategy_id=self.strategy.id))
        self.assertTrue(order not in self.cache.orders_working())
        self.assertTrue(order not in self.cache.orders_working(symbol=order.symbol))
        self.assertTrue(order not in self.cache.orders_working(strategy_id=self.strategy.id))
        self.assertTrue(order not in self.cache.orders_working(symbol=order.symbol, strategy_id=self.strategy.id))

    def test_update_position_for_open_position(self):
        # Arrange
        order1 = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        position_id = PositionId('P-1')
        self.cache.add_order(order1, position_id, self.strategy.id)
        order1.apply(TestStubs.event_order_submitted(order1))
        self.cache.update_order(order1)

        order1.apply(TestStubs.event_order_accepted(order1))
        self.cache.update_order(order1)
        order1_filled = TestStubs.event_order_filled(
            order1,
            position_id=PositionId('P-1'),
            fill_price=Price("1.00001"),
        )

        position = Position(order1_filled)

        # Act
        self.cache.add_position(position, self.strategy.id)

        # Assert
        self.assertTrue(self.cache.position_exists(position.id))
        self.assertTrue(position.id in self.cache.position_ids())
        self.assertTrue(position in self.cache.positions())
        self.assertTrue(position in self.cache.positions_open())
        self.assertTrue(position in self.cache.positions_open(symbol=position.symbol))
        self.assertTrue(position in self.cache.positions_open(strategy_id=self.strategy.id))
        self.assertTrue(position in self.cache.positions_open(symbol=position.symbol, strategy_id=self.strategy.id))
        self.assertTrue(position not in self.cache.positions_closed())
        self.assertTrue(position not in self.cache.positions_closed(symbol=position.symbol))
        self.assertTrue(position not in self.cache.positions_closed(strategy_id=self.strategy.id))
        self.assertTrue(position not in self.cache.positions_closed(symbol=position.symbol, strategy_id=self.strategy.id))
        self.assertEqual(position, self.cache.position(position_id))

    def test_update_position_for_closed_position(self):
        # Arrange
        order1 = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        position_id = PositionId('P-1')
        self.cache.add_order(order1, position_id, self.strategy.id)
        order1.apply(TestStubs.event_order_submitted(order1))
        self.cache.update_order(order1)

        order1.apply(TestStubs.event_order_accepted(order1))
        self.cache.update_order(order1)
        order1_filled = TestStubs.event_order_filled(
            order1,
            position_id=PositionId('P-1'),
            fill_price=Price("1.00001"),
        )

        position = Position(order1_filled)
        self.cache.add_position(position, self.strategy.id)

        order2 = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.SELL,
            Quantity(100000))
        order2.apply(TestStubs.event_order_submitted(order2))
        self.cache.update_order(order2)

        order2.apply(TestStubs.event_order_accepted(order2))
        self.cache.update_order(order2)
        order2_filled = TestStubs.event_order_filled(
            order2,
            position_id=PositionId('P-1'),
            fill_price=Price("1.00001"),
        )
        position.apply(order2_filled)

        # Act
        self.cache.update_position(position)

        # Assert
        self.assertTrue(self.cache.position_exists(position.id))
        self.assertTrue(position.id in self.cache.position_ids())
        self.assertTrue(position in self.cache.positions())
        self.assertTrue(position in self.cache.positions_closed())
        self.assertTrue(position in self.cache.positions_closed(symbol=position.symbol))
        self.assertTrue(position in self.cache.positions_closed(strategy_id=self.strategy.id))
        self.assertTrue(position in self.cache.positions_closed(symbol=position.symbol, strategy_id=self.strategy.id))
        self.assertTrue(position not in self.cache.positions_open())
        self.assertTrue(position not in self.cache.positions_open(symbol=position.symbol))
        self.assertTrue(position not in self.cache.positions_open(strategy_id=self.strategy.id))
        self.assertTrue(position not in self.cache.positions_open(symbol=position.symbol, strategy_id=self.strategy.id))
        self.assertEqual(position, self.cache.position(position_id))

    def test_add_account(self):
        # Arrange
        event = AccountState(
            AccountId.py_from_string("SIMULATED-123456-SIMULATED"),
            Currency.USD,
            Money("1000000", Currency.USD),
            Money("1000000", Currency.USD),
            Money("0", Currency.USD),
            Money("0", Currency.USD),
            Money("0", Currency.USD),
            Decimal(),
            "N",
            uuid4(),
            UNIX_EPOCH)

        account = Account(event)

        # Act
        self.cache.add_account(account)

        # Assert
        self.assertTrue(True)  # Did not raise exception

    def test_update_account(self):
        # Arrange
        event = AccountState(
            AccountId.py_from_string("SIMULATED-123456-SIMULATED"),
            Currency.USD,
            Money(1000000, Currency.USD),
            Money(1000000, Currency.USD),
            Money(0, Currency.USD),
            Money(0, Currency.USD),
            Money(0, Currency.USD),
            Decimal(),
            "N",
            uuid4(),
            UNIX_EPOCH)

        account = Account(event)
        self.cache.add_account(account)

        # Act
        self.cache.update_account(account)

        # Assert
        self.assertTrue(True)  # Did not raise exception

    def test_delete_strategy(self):
        # Arrange
        self.cache.update_strategy(self.strategy)

        # Act
        self.cache.delete_strategy(self.strategy)

        # Assert
        self.assertTrue(self.strategy.id not in self.cache.strategy_ids())

    def test_check_residuals(self):
        # Arrange
        order1 = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        position1_id = PositionId('P-1')
        self.cache.add_order(order1, position1_id, self.strategy.id)

        order1.apply(TestStubs.event_order_submitted(order1))
        self.cache.update_order(order1)

        order1.apply(TestStubs.event_order_accepted(order1))
        self.cache.update_order(order1)

        order1_filled = TestStubs.event_order_filled(
            order1,
            position_id=position1_id,
            fill_price=Price("1.00000"),
        )
        position1 = Position(order1_filled)
        self.cache.update_order(order1)
        self.cache.add_position(position1, self.strategy.id)

        order2 = self.strategy.order_factory.stop(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price("1.0000"))
        position2_id = PositionId('P-2')
        self.cache.add_order(order2, position2_id, self.strategy.id)

        order2.apply(TestStubs.event_order_submitted(order2))
        self.cache.update_order(order2)

        order2.apply(TestStubs.event_order_accepted(order2))
        self.cache.update_order(order2)

        order2.apply(TestStubs.event_order_working(order2))
        self.cache.update_order(order2)

        # Act
        self.cache.check_residuals()

        # Does not raise exception

    def test_reset(self):
        # Arrange
        order1 = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        position1_id = PositionId('P-1')
        self.cache.add_order(order1, position1_id, self.strategy.id)

        order1.apply(TestStubs.event_order_submitted(order1))
        self.cache.update_order(order1)

        order1.apply(TestStubs.event_order_accepted(order1))
        self.cache.update_order(order1)

        order1_filled = TestStubs.event_order_filled(
            order1,
            position_id=position1_id,
            fill_price=Price("1.00000"),
        )
        position1 = Position(order1_filled)
        self.cache.update_order(order1)
        self.cache.add_position(position1, self.strategy.id)

        order2 = self.strategy.order_factory.stop(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price("1.00000"))

        position2_id = PositionId('P-2')
        self.cache.add_order(order2, position2_id, self.strategy.id)

        order2.apply(TestStubs.event_order_submitted(order2))
        self.cache.update_order(order2)

        order2.apply(TestStubs.event_order_accepted(order2))
        self.cache.update_order(order2)

        order2.apply(TestStubs.event_order_working(order2))
        self.cache.update_order(order2)

        self.cache.update_order(order2)

        # Act
        self.cache.reset()

        # Assert
        self.assertEqual(0, len(self.cache.strategy_ids()))
        self.assertEqual(0, self.cache.orders_total_count())
        self.assertEqual(0, self.cache.positions_total_count())

    def test_flush_db(self):
        # Arrange
        order1 = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))
        position1_id = PositionId('P-1')
        self.cache.add_order(order1, position1_id, self.strategy.id)

        order1.apply(TestStubs.event_order_submitted(order1))
        self.cache.update_order(order1)

        order1.apply(TestStubs.event_order_accepted(order1))
        self.cache.update_order(order1)

        order1_filled = TestStubs.event_order_filled(
            order1,
            position_id=position1_id,
            fill_price=Price("1.00000"),
        )
        position1 = Position(order1_filled)
        self.cache.update_order(order1)
        self.cache.add_position(position1, self.strategy.id)

        order2 = self.strategy.order_factory.stop(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price("1.00000"))

        position2_id = PositionId('P-2')
        self.cache.add_order(order2, position2_id, self.strategy.id)
        order2.apply(TestStubs.event_order_submitted(order2))
        self.cache.update_order(order2)

        order2.apply(TestStubs.event_order_accepted(order2))
        self.cache.update_order(order2)

        order2.apply(TestStubs.event_order_working(order2))
        self.cache.update_order(order2)

        # Act
        self.cache.reset()
        self.cache.flush_db()

        # Assert
        # Does not raise exception

    def test_get_strategy_ids_with_no_ids_returns_empty_set(self):
        # Arrange
        # Act
        result = self.cache.strategy_ids()

        # Assert
        self.assertEqual(set(), result)

    def test_get_strategy_ids_with_id_returns_correct_set(self):
        # Arrange
        self.cache.update_strategy(self.strategy)

        # Act
        result = self.cache.strategy_ids()

        # Assert
        self.assertEqual({self.strategy.id}, result)

    def test_position_exists_when_no_position_returns_false(self):
        # Arrange
        # Act
        # Assert
        self.assertFalse(self.cache.position_exists(PositionId("P-123456")))

    def test_order_exists_when_no_order_returns_false(self):
        # Arrange
        # Act
        # Assert
        self.assertFalse(self.cache.order_exists(ClientOrderId("O-123456")))

    def test_position_indexed_for_order_when_no_indexing_returns_false(self):
        # Arrange
        # Act
        # Assert
        self.assertFalse(self.cache.position_indexed_for_order(ClientOrderId("O-123456")))

    def test_get_order_when_no_order_returns_none(self):
        # Arrange
        position_id = PositionId("P-123456")

        # Act
        result = self.cache.position(position_id)

        # Assert
        self.assertIsNone(result)

    def test_get_position_when_no_position_returns_none(self):
        # Arrange
        order_id = ClientOrderId("O-201908080101-000-001")

        # Act
        result = self.cache.order(order_id)

        # Assert
        self.assertIsNone(result)
