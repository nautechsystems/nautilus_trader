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

import redis

from nautilus_trader.backtest.loaders import InstrumentLoader
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.logging import TestLogger
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.position import Position
from nautilus_trader.redis.execution import RedisExecutionDatabase
from nautilus_trader.serialization.serializers import MsgPackCommandSerializer
from nautilus_trader.serialization.serializers import MsgPackEventSerializer
from nautilus_trader.trading.account import Account
from nautilus_trader.trading.strategy import TradingStrategy
from tests.test_kit.stubs import TestStubs


AUDUSD_FXCM = InstrumentLoader.default_fx_ccy(TestStubs.symbol_audusd_fxcm())

# Requirements:
#    - A Redis instance listening on the default port 6379


class RedisExecutionDatabaseTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        clock = TestClock()
        logger = TestLogger(clock)

        self.trader_id = TraderId("TESTER", "000")

        self.strategy = TradingStrategy(order_id_tag="001")
        self.strategy.register_trader(
            TraderId("TESTER", "000"),
            clock,
            logger,
        )

        config = {
            'host': 'localhost',
            'port': 6379,
        }

        self.database = RedisExecutionDatabase(
            trader_id=self.trader_id,
            logger=logger,
            command_serializer=MsgPackCommandSerializer(),
            event_serializer=MsgPackEventSerializer(),
            config=config,
        )

        self.test_redis = redis.Redis(host="localhost", port=6379, db=0)

    def tearDown(self):
        # Tests will start failing if redis is not flushed on tear down
        self.test_redis.flushall()  # Comment this line out to preserve data between tests

    def test_add_account(self):
        # Arrange
        event = TestStubs.event_account_state()
        account = Account(event)

        # Act
        self.database.add_account(account)

        # Assert
        self.assertEqual(account, self.database.load_account(account.id))

    def test_add_order(self):
        # Arrange
        order = self.strategy.order_factory.market(
            AUDUSD_FXCM.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

        # Act
        self.database.add_order(order)

        # Assert
        self.assertEqual(order, self.database.load_order(order.cl_ord_id))

    def test_add_position(self):
        # Arrange
        order = self.strategy.order_factory.market(
            AUDUSD_FXCM.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

        self.database.add_order(order)

        position_id = PositionId('P-1')
        order_filled = TestStubs.event_order_filled(
            order,
            instrument=AUDUSD_FXCM,
            position_id=position_id,
            fill_price=Price("1.00000"),
        )

        position = Position(order_filled)

        # Act
        self.database.add_position(position)

        # Assert
        self.assertEqual(position, self.database.load_position(position.id))

    def test_update_account(self):
        # Arrange
        event = TestStubs.event_account_state()
        account = Account(event)
        self.database.add_account(account)

        # Act
        self.database.update_account(account)

        # Assert
        self.assertEqual(account, self.database.load_account(account.id))

    def test_update_order_for_working_order(self):
        # Arrange
        order = self.strategy.order_factory.stop_market(
            AUDUSD_FXCM.symbol,
            OrderSide.BUY,
            Quantity(100000),
            Price("1.00000"),
        )

        self.database.add_order(order)

        order.apply(TestStubs.event_order_submitted(order))
        self.database.update_order(order)

        order.apply(TestStubs.event_order_accepted(order))
        self.database.update_order(order)

        # Act
        order.apply(TestStubs.event_order_working(order))
        self.database.update_order(order)

        # Assert
        self.assertEqual(order, self.database.load_order(order.cl_ord_id))

    def test_update_order_for_completed_order(self):
        # Arrange
        order = self.strategy.order_factory.market(
            AUDUSD_FXCM.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

        self.database.add_order(order)

        order.apply(TestStubs.event_order_submitted(order))
        self.database.update_order(order)

        order.apply(TestStubs.event_order_accepted(order))
        self.database.update_order(order)

        order.apply(TestStubs.event_order_filled(
            order,
            instrument=AUDUSD_FXCM,
            fill_price=Price("1.00001"),
        ))

        # Act
        self.database.update_order(order)

        # Assert
        self.assertEqual(order, self.database.load_order(order.cl_ord_id))

    def test_update_position_for_closed_position(self):
        # Arrange
        order1 = self.strategy.order_factory.market(
            AUDUSD_FXCM.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

        position_id = PositionId('P-1')
        self.database.add_order(order1)

        order1.apply(TestStubs.event_order_submitted(order1))
        self.database.update_order(order1)

        order1.apply(TestStubs.event_order_accepted(order1))
        self.database.update_order(order1)

        order1.apply(TestStubs.event_order_filled(
            order1,
            instrument=AUDUSD_FXCM,
            position_id=position_id,
            fill_price=Price("1.00001"),
        ))
        self.database.update_order(order1)

        # Act
        position = Position(order1.last_event)
        self.database.add_position(position)

        order2 = self.strategy.order_factory.market(
            AUDUSD_FXCM.symbol,
            OrderSide.SELL,
            Quantity(100000),
        )

        self.database.add_order(order2)

        order2.apply(TestStubs.event_order_submitted(order2))
        self.database.update_order(order2)

        order2.apply(TestStubs.event_order_accepted(order2))
        self.database.update_order(order2)

        filled = TestStubs.event_order_filled(
            order2,
            instrument=AUDUSD_FXCM,
            position_id=position_id,
            fill_price=Price("1.00001"),
        )

        order2.apply(filled)
        self.database.update_order(order2)

        position.apply(filled)

        # Act
        self.database.update_position(position)

        # Assert
        self.assertEqual(position, self.database.load_position(position.id))

    def test_load_account_when_no_account_in_database_returns_none(self):
        # Arrange
        event = TestStubs.event_account_state()
        account = Account(event)

        # Act
        result = self.database.load_account(account.id)

        # Assert
        self.assertIsNone(result)

    def test_load_account_when_account_in_database_returns_account(self):
        # Arrange
        event = TestStubs.event_account_state()
        account = Account(event)
        self.database.add_account(account)

        # Act
        result = self.database.load_account(account.id)

        # Assert
        self.assertEqual(account, result)

    def test_load_order_when_no_order_in_database_returns_none(self):
        # Arrange
        order = self.strategy.order_factory.market(
            AUDUSD_FXCM.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

        # Act
        result = self.database.load_order(order.cl_ord_id)

        # Assert
        self.assertIsNone(result)

    def test_load_order_when_market_order_in_database_returns_order(self):
        # Arrange
        order = self.strategy.order_factory.market(
            AUDUSD_FXCM.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

        self.database.add_order(order)

        # Act
        result = self.database.load_order(order.cl_ord_id)

        # Assert
        self.assertEqual(order, result)

    def test_load_order_when_limit_order_in_database_returns_order(self):
        # Arrange
        order = self.strategy.order_factory.limit(
            AUDUSD_FXCM.symbol,
            OrderSide.BUY,
            Quantity(100000),
            Price("1.00000"),
        )

        self.database.add_order(order)

        # Act
        result = self.database.load_order(order.cl_ord_id)

        # Assert
        self.assertEqual(order, result)

    def test_load_order_when_stop_market_order_in_database_returns_order(self):
        # Arrange
        order = self.strategy.order_factory.stop_market(
            AUDUSD_FXCM.symbol,
            OrderSide.BUY,
            Quantity(100000),
            Price("1.00000"),
        )

        self.database.add_order(order)

        # Act
        result = self.database.load_order(order.cl_ord_id)

        # Assert
        self.assertEqual(order, result)

    def test_load_position_when_no_position_in_database_returns_none(self):
        # Arrange
        position_id = PositionId('P-1')

        # Act
        result = self.database.load_position(position_id)

        # Assert
        self.assertIsNone(result)

    def test_load_order_when_position_in_database_returns_position(self):
        # Arrange
        order = self.strategy.order_factory.market(
            AUDUSD_FXCM.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

        self.database.add_order(order)

        position_id = PositionId('P-1')
        order_filled = TestStubs.event_order_filled(
            order,
            instrument=AUDUSD_FXCM,
            position_id=position_id,
            fill_price=Price("1.00000"),
        )

        position = Position(order_filled)

        self.database.add_position(position)

        # Act
        result = self.database.load_position(position_id)
        # Assert
        self.assertEqual(position, result)

    def test_load_accounts_when_no_accounts_returns_empty_dict(self):
        # Arrange
        # Act
        result = self.database.load_accounts()

        # Assert
        self.assertEqual({}, result)

    def test_load_accounts_cache_when_one_account_in_database(self):
        # Arrange
        event = TestStubs.event_account_state()
        account = Account(event)
        self.database.add_account(account)

        # Act
        # Assert
        self.assertEqual(account, self.database.load_account(account.id))

    def test_load_orders_cache_when_no_orders(self):
        # Arrange
        # Act
        self.database.load_orders()

        # Assert
        self.assertEqual({}, self.database.load_orders())

    def test_load_orders_cache_when_one_order_in_database(self):
        # Arrange
        order = self.strategy.order_factory.market(
            AUDUSD_FXCM.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

        self.database.add_order(order)

        # Act
        result = self.database.load_orders()

        # Assert
        self.assertEqual({order.cl_ord_id: order}, result)

    def test_load_positions_cache_when_no_positions(self):
        # Arrange
        # Act
        self.database.load_positions()

        # Assert
        self.assertEqual({}, self.database.load_positions())

    def test_load_positions_cache_when_one_position_in_database(self):
        # Arrange
        order1 = self.strategy.order_factory.stop_market(
            AUDUSD_FXCM.symbol,
            OrderSide.BUY,
            Quantity(100000),
            Price("1.00000"),
        )

        self.database.add_order(order1)

        position_id = PositionId('P-1')
        order1.apply(TestStubs.event_order_submitted(order1))
        order1.apply(TestStubs.event_order_accepted(order1))
        order1.apply(TestStubs.event_order_working(order1))
        order1.apply(TestStubs.event_order_filled(
            order1,
            instrument=AUDUSD_FXCM,
            position_id=position_id,
            fill_price=Price("1.00001"),
        ))

        position = Position(order1.last_event)
        self.database.add_position(position)

        # Act
        result = self.database.load_positions()

        # Assert
        self.assertEqual({position.id: position}, result)

    def test_delete_strategy(self):
        # Arrange
        # Act
        self.database.delete_strategy(self.strategy.id)
        result = self.database.load_strategy(self.strategy.id)

        # Assert
        self.assertEqual({}, result)

    def test_flush(self):
        # Arrange
        order1 = self.strategy.order_factory.market(
            AUDUSD_FXCM.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

        self.database.add_order(order1)

        position1_id = PositionId('P-1')
        filled = TestStubs.event_order_filled(
            order1,
            instrument=AUDUSD_FXCM,
            position_id=position1_id,
            fill_price=Price("1.00000"),
        )

        position1 = Position(filled)
        self.database.update_order(order1)
        self.database.add_position(position1)

        order2 = self.strategy.order_factory.stop_market(
            AUDUSD_FXCM.symbol,
            OrderSide.BUY,
            Quantity(100000),
            Price("1.00000"),
        )

        self.database.add_order(order2)

        order2.apply(TestStubs.event_order_submitted(order2))
        order2.apply(TestStubs.event_order_accepted(order2))
        order2.apply(TestStubs.event_order_working(order2))

        self.database.update_order(order2)

        # Act
        self.database.flush()

        # Assert
        self.assertIsNone(self.database.load_order(order1.cl_ord_id))
        self.assertIsNone(self.database.load_order(order2.cl_ord_id))
        self.assertIsNone(self.database.load_position(position1.id))
