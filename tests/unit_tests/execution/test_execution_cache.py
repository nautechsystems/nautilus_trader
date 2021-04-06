# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

from decimal import Decimal
import unittest

from nautilus_trader.backtest.data_container import BacktestDataContainer
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.execution.cache import ExecutionCache
from nautilus_trader.execution.database import BypassExecutionDatabase
from nautilus_trader.model.bar import BarSpecification
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import OMSType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.position import Position
from nautilus_trader.trading.account import Account
from nautilus_trader.trading.strategy import TradingStrategy
from tests.test_kit.providers import TestDataProvider
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.strategies import EMACross
from tests.test_kit.stubs import TestStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
GBPUSD_SIM = TestInstrumentProvider.default_fx_ccy("GBP/USD")


class ExecutionCacheTests(unittest.TestCase):
    def setUp(self):
        # Fixture Setup
        clock = TestClock()
        logger = Logger(clock)

        self.trader_id = TraderId("TESTER", "000")
        self.account_id = TestStubs.account_id()

        self.strategy = TradingStrategy(order_id_tag="001")
        self.strategy.register_trader(
            TraderId("TESTER", "000"),
            clock,
            logger,
        )

        exec_db = BypassExecutionDatabase(trader_id=self.trader_id, logger=logger)
        self.cache = ExecutionCache(database=exec_db, logger=logger)

    def test_cache_accounts_with_no_accounts(self):
        # Arrange
        # Act
        self.cache.cache_accounts()

        # Assert
        self.assertTrue(True)  # No exception raised

    def test_cache_orders_with_no_orders(self):
        # Arrange
        # Act
        self.cache.cache_orders()

        # Assert
        self.assertTrue(True)  # No exception raised

    def test_cache_positions_with_no_positions(self):
        # Arrange
        # Act
        self.cache.cache_positions()

        # Assert
        self.assertTrue(True)  # No exception raised

    def test_build_index_with_no_objects(self):
        # Arrange
        # Act
        self.cache.build_index()

        # Assert
        self.assertTrue(True)  # No exception raised

    def test_add_account(self):
        # Arrange
        initial = TestStubs.event_account_state()
        account = Account(initial)

        # Act
        self.cache.add_account(account)

        # Assert
        self.assertEqual(account, self.cache.load_account(account.id))

    def test_load_account(self):
        # Arrange
        initial = TestStubs.event_account_state()
        account = Account(initial)

        self.cache.add_account(account)

        # Act
        result = self.cache.load_account(account.id)

        # Assert
        self.assertEqual(account, result)

    def test_account_for_venue(self):
        # Arrange
        # Act
        result = self.cache.account_for_venue(Venue("SIM"))

        # Assert
        self.assertIsNone(result)

    def test_accounts_when_no_accounts_returns_empty_list(self):
        # Arrange
        # Act
        result = self.cache.accounts()

        # Assert
        self.assertEqual([], result)

    def test_get_strategy_ids_with_no_ids_returns_empty_set(self):
        # Arrange
        # Act
        result = self.cache.strategy_ids()

        # Assert
        self.assertEqual(set(), result)

    def test_get_order_ids_with_no_ids_returns_empty_set(self):
        # Arrange
        # Act
        result = self.cache.client_order_ids()

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

    def test_position_when_no_position_returns_none(self):
        # Arrange
        position_id = PositionId("P-123456")

        # Act
        result = self.cache.position(position_id)

        # Assert
        self.assertIsNone(result)

    def test_order_when_no_order_returns_none(self):
        # Arrange
        order_id = ClientOrderId("O-201908080101-000-001")

        # Act
        result = self.cache.order(order_id)

        # Assert
        self.assertIsNone(result)

    def test_strategy_id_for_position_when_no_strategy_registered_returns_none(self):
        # Arrange
        # Act
        # Assert
        self.assertIsNone(self.cache.strategy_id_for_position(PositionId("P-123456")))

    def test_add_order(self):
        # Arrange
        order = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000),
        )

        position_id = PositionId("P-1")

        # Act
        self.cache.add_order(order, position_id)

        # Assert
        self.assertIn(order.client_order_id, self.cache.client_order_ids())
        self.assertIn(
            order.client_order_id,
            self.cache.client_order_ids(instrument_id=order.instrument_id),
        )
        self.assertIn(
            order.client_order_id,
            self.cache.client_order_ids(strategy_id=self.strategy.id),
        )
        self.assertNotIn(
            order.client_order_id,
            self.cache.client_order_ids(strategy_id=StrategyId("S", "ZX1")),
        )
        self.assertIn(
            order.client_order_id,
            self.cache.client_order_ids(
                instrument_id=order.instrument_id, strategy_id=self.strategy.id
            ),
        )
        self.assertIn(order, self.cache.orders())
        self.assertEqual(
            VenueOrderId.null(), self.cache.venue_order_id(order.client_order_id)
        )
        self.assertIsNone(self.cache.client_order_id(order.venue_order_id))

    def test_load_order(self):
        # Arrange
        order = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000),
        )

        position_id = PositionId("P-1")
        self.cache.add_order(order, position_id)

        # Act
        result = self.cache.load_order(order.client_order_id)

        # Assert
        self.assertEqual(order, result)

    def test_add_position(self):
        # Arrange
        order = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000),
        )

        position_id = PositionId("P-1")
        self.cache.add_order(order, position_id)

        fill = TestStubs.event_order_filled(
            order,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-1"),
            last_px=Price("1.00000"),
        )

        position = Position(fill=fill)

        # Act
        self.cache.add_position(position)

        # Assert
        self.assertTrue(self.cache.position_exists(position.id))
        self.assertIn(position.id, self.cache.position_ids())
        self.assertIn(position, self.cache.positions())
        self.assertIn(position, self.cache.positions_open())
        self.assertIn(
            position, self.cache.positions_open(instrument_id=position.instrument_id)
        )
        self.assertIn(position, self.cache.positions_open(strategy_id=self.strategy.id))
        self.assertIn(
            position,
            self.cache.positions_open(
                instrument_id=position.instrument_id, strategy_id=self.strategy.id
            ),
        )
        self.assertNotIn(position, self.cache.positions_closed())
        self.assertNotIn(
            position, self.cache.positions_closed(instrument_id=position.instrument_id)
        )
        self.assertNotIn(
            position, self.cache.positions_closed(strategy_id=self.strategy.id)
        )
        self.assertNotIn(
            position,
            self.cache.positions_closed(
                instrument_id=position.instrument_id, strategy_id=self.strategy.id
            ),
        )

    def test_load_position(self):
        # Arrange
        order = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000),
        )

        position_id = PositionId("P-1")
        self.cache.add_order(order, position_id)

        fill = TestStubs.event_order_filled(
            order,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-1"),
            last_px=Price("1.00000"),
        )

        position = Position(fill=fill)
        self.cache.add_position(position)

        # Act
        result = self.cache.load_position(position.id)

        # Assert
        self.assertEqual(position, result)

    def test_update_order_for_accepted_order(self):
        # Arrange
        order = self.strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000),
            Price("1.00000"),
        )

        position_id = PositionId("P-1")
        self.cache.add_order(order, position_id)

        order.apply(TestStubs.event_order_submitted(order))
        self.cache.update_order(order)

        order.apply(TestStubs.event_order_accepted(order))

        # Act
        self.cache.update_order(order)

        # Assert
        self.assertTrue(self.cache.order_exists(order.client_order_id))
        self.assertIn(order.client_order_id, self.cache.client_order_ids())
        self.assertIn(order, self.cache.orders())
        self.assertIn(order, self.cache.orders_working())
        self.assertIn(
            order, self.cache.orders_working(instrument_id=order.instrument_id)
        )
        self.assertIn(order, self.cache.orders_working(strategy_id=self.strategy.id))
        self.assertIn(
            order,
            self.cache.orders_working(
                instrument_id=order.instrument_id, strategy_id=self.strategy.id
            ),
        )
        self.assertNotIn(order, self.cache.orders_completed())
        self.assertNotIn(
            order, self.cache.orders_completed(instrument_id=order.instrument_id)
        )
        self.assertNotIn(
            order, self.cache.orders_completed(strategy_id=self.strategy.id)
        )
        self.assertNotIn(
            order,
            self.cache.orders_completed(
                instrument_id=order.instrument_id, strategy_id=self.strategy.id
            ),
        )
        self.assertEqual(1, self.cache.orders_working_count())
        self.assertEqual(0, self.cache.orders_completed_count())
        self.assertEqual(1, self.cache.orders_total_count())

    def test_update_order_for_completed_order(self):
        # Arrange
        order = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000),
        )

        position_id = PositionId("P-1")
        self.cache.add_order(order, position_id)
        order.apply(TestStubs.event_order_submitted(order))
        self.cache.update_order(order)

        order.apply(TestStubs.event_order_accepted(order))
        self.cache.update_order(order)

        fill = TestStubs.event_order_filled(
            order, instrument=AUDUSD_SIM, last_px=Price("1.00001")
        )

        order.apply(fill)

        # Act
        self.cache.update_order(order)

        # Assert
        self.assertTrue(self.cache.order_exists(order.client_order_id))
        self.assertIn(order.client_order_id, self.cache.client_order_ids())
        self.assertIn(order, self.cache.orders())
        self.assertIn(order, self.cache.orders_completed())
        self.assertIn(
            order, self.cache.orders_completed(instrument_id=order.instrument_id)
        )
        self.assertIn(order, self.cache.orders_completed(strategy_id=self.strategy.id))
        self.assertIn(
            order,
            self.cache.orders_completed(
                instrument_id=order.instrument_id, strategy_id=self.strategy.id
            ),
        )
        self.assertNotIn(order, self.cache.orders_working())
        self.assertNotIn(
            order, self.cache.orders_working(instrument_id=order.instrument_id)
        )
        self.assertNotIn(order, self.cache.orders_working(strategy_id=self.strategy.id))
        self.assertNotIn(
            order,
            self.cache.orders_working(
                instrument_id=order.instrument_id, strategy_id=self.strategy.id
            ),
        )
        self.assertEqual(
            order.venue_order_id, self.cache.venue_order_id(order.client_order_id)
        )
        self.assertEqual(0, self.cache.orders_working_count())
        self.assertEqual(1, self.cache.orders_completed_count())
        self.assertEqual(1, self.cache.orders_total_count())

    def test_update_position_for_open_position(self):
        # Arrange
        order1 = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000),
        )

        position_id = PositionId("P-1")
        self.cache.add_order(order1, position_id)
        order1.apply(TestStubs.event_order_submitted(order1))
        self.cache.update_order(order1)

        order1.apply(TestStubs.event_order_accepted(order1))
        self.cache.update_order(order1)
        fill1 = TestStubs.event_order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-1"),
            last_px=Price("1.00001"),
        )

        position = Position(fill=fill1)

        # Act
        self.cache.add_position(position)

        # Assert
        self.assertTrue(self.cache.position_exists(position.id))
        self.assertIn(position.id, self.cache.position_ids())
        self.assertIn(position, self.cache.positions())
        self.assertIn(position, self.cache.positions_open())
        self.assertIn(
            position, self.cache.positions_open(instrument_id=position.instrument_id)
        )
        self.assertIn(position, self.cache.positions_open(strategy_id=self.strategy.id))
        self.assertIn(
            position,
            self.cache.positions_open(
                instrument_id=position.instrument_id, strategy_id=self.strategy.id
            ),
        )
        self.assertNotIn(position, self.cache.positions_closed())
        self.assertNotIn(
            position, self.cache.positions_closed(instrument_id=position.instrument_id)
        )
        self.assertNotIn(
            position, self.cache.positions_closed(strategy_id=self.strategy.id)
        )
        self.assertNotIn(
            position,
            self.cache.positions_closed(
                instrument_id=position.instrument_id, strategy_id=self.strategy.id
            ),
        )
        self.assertEqual(position, self.cache.position(position_id))
        self.assertEqual(1, self.cache.positions_open_count())
        self.assertEqual(0, self.cache.positions_closed_count())
        self.assertEqual(1, self.cache.positions_total_count())

    def test_update_position_for_closed_position(self):
        # Arrange
        order1 = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000),
        )

        position_id = PositionId("P-1")
        self.cache.add_order(order1, position_id)
        order1.apply(TestStubs.event_order_submitted(order1))
        self.cache.update_order(order1)

        order1.apply(TestStubs.event_order_accepted(order1))
        self.cache.update_order(order1)
        fill1 = TestStubs.event_order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-1"),
            last_px=Price("1.00001"),
        )

        position = Position(fill=fill1)
        self.cache.add_position(position)

        order2 = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity(100000),
        )

        order2.apply(TestStubs.event_order_submitted(order2))
        self.cache.update_order(order2)

        order2.apply(TestStubs.event_order_accepted(order2))
        self.cache.update_order(order2)
        order2_filled = TestStubs.event_order_filled(
            order2,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-1"),
            last_px=Price("1.00001"),
        )

        position.apply(order2_filled)

        # Act
        self.cache.update_position(position)

        # Assert
        self.assertTrue(self.cache.position_exists(position.id))
        self.assertIn(position.id, self.cache.position_ids())
        self.assertIn(position, self.cache.positions())
        self.assertIn(position, self.cache.positions_closed())
        self.assertIn(
            position, self.cache.positions_closed(instrument_id=position.instrument_id)
        )
        self.assertIn(
            position, self.cache.positions_closed(strategy_id=self.strategy.id)
        )
        self.assertIn(
            position,
            self.cache.positions_closed(
                instrument_id=position.instrument_id, strategy_id=self.strategy.id
            ),
        )
        self.assertNotIn(position, self.cache.positions_open())
        self.assertNotIn(
            position, self.cache.positions_open(instrument_id=position.instrument_id)
        )
        self.assertNotIn(
            position, self.cache.positions_open(strategy_id=self.strategy.id)
        )
        self.assertNotIn(
            position,
            self.cache.positions_open(
                instrument_id=position.instrument_id, strategy_id=self.strategy.id
            ),
        )
        self.assertEqual(position, self.cache.position(position_id))
        self.assertEqual(0, self.cache.positions_open_count())
        self.assertEqual(1, self.cache.positions_closed_count())
        self.assertEqual(1, self.cache.positions_total_count())

    def test_update_account(self):
        # Arrange
        event = TestStubs.event_account_state()
        account = Account(event)

        self.cache.add_account(account)

        # Act
        self.cache.update_account(account)

        # Assert
        self.assertTrue(True)  # No exceptions raised

    def test_delete_strategy(self):
        # Arrange
        self.cache.update_strategy(self.strategy)

        # Act
        self.cache.delete_strategy(self.strategy)

        # Assert
        self.assertNotIn(self.strategy.id, self.cache.strategy_ids())

    def test_check_residuals(self):
        # Arrange
        order1 = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000),
        )

        position1_id = PositionId("P-1")
        self.cache.add_order(order1, position1_id)

        order1.apply(TestStubs.event_order_submitted(order1))
        self.cache.update_order(order1)

        order1.apply(TestStubs.event_order_accepted(order1))
        self.cache.update_order(order1)

        fill1 = TestStubs.event_order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=position1_id,
            last_px=Price("1.00000"),
        )

        position1 = Position(fill=fill1)
        self.cache.update_order(order1)
        self.cache.add_position(position1)

        order2 = self.strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000),
            Price("1.0000"),
        )

        position2_id = PositionId("P-2")
        self.cache.add_order(order2, position2_id)

        order2.apply(TestStubs.event_order_submitted(order2))
        self.cache.update_order(order2)

        order2.apply(TestStubs.event_order_accepted(order2))
        self.cache.update_order(order2)

        # Act
        self.cache.check_residuals()

        # Assert
        self.assertTrue(True)  # No exception raised

    def test_reset(self):
        # Arrange
        order1 = self.strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000),
        )

        position1_id = PositionId("P-1")
        self.cache.add_order(order1, position1_id)

        order1.apply(TestStubs.event_order_submitted(order1))
        self.cache.update_order(order1)

        order1.apply(TestStubs.event_order_accepted(order1))
        self.cache.update_order(order1)

        fill1 = TestStubs.event_order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=position1_id,
            last_px=Price("1.00000"),
        )
        position1 = Position(fill=fill1)
        self.cache.update_order(order1)
        self.cache.add_position(position1)

        order2 = self.strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000),
            Price("1.00000"),
        )

        position2_id = PositionId("P-2")
        self.cache.add_order(order2, position2_id)

        order2.apply(TestStubs.event_order_submitted(order2))
        self.cache.update_order(order2)

        order2.apply(TestStubs.event_order_accepted(order2))
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
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000),
        )

        position1_id = PositionId("P-1")
        self.cache.add_order(order1, position1_id)

        order1.apply(TestStubs.event_order_submitted(order1))
        self.cache.update_order(order1)

        order1.apply(TestStubs.event_order_accepted(order1))
        self.cache.update_order(order1)

        fill1 = TestStubs.event_order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=position1_id,
            last_px=Price("1.00000"),
        )

        position1 = Position(fill=fill1)
        self.cache.update_order(order1)
        self.cache.add_position(position1)

        order2 = self.strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000),
            Price("1.00000"),
        )

        position2_id = PositionId("P-2")
        self.cache.add_order(order2, position2_id)
        order2.apply(TestStubs.event_order_submitted(order2))
        self.cache.update_order(order2)

        order2.apply(TestStubs.event_order_accepted(order2))
        self.cache.update_order(order2)

        # Act
        self.cache.reset()
        self.cache.flush_db()

        # Assert
        self.assertTrue(True)  # No exception raised


class ExecutionCacheIntegrityCheckTests(unittest.TestCase):
    def setUp(self):
        # Fixture Setup
        self.venue = Venue("SIM")
        self.usdjpy = TestInstrumentProvider.default_fx_ccy("USD/JPY")
        data = BacktestDataContainer()
        data.add_instrument(self.usdjpy)
        data.add_bars(
            self.usdjpy.id,
            BarAggregation.MINUTE,
            PriceType.BID,
            TestDataProvider.usdjpy_1min_bid(),
        )
        data.add_bars(
            self.usdjpy.id,
            BarAggregation.MINUTE,
            PriceType.ASK,
            TestDataProvider.usdjpy_1min_ask(),
        )

        self.engine = BacktestEngine(
            data=data,
            strategies=[TradingStrategy("000")],
            bypass_logging=True,  # Uncomment this to see integrity check failure messages
        )

        self.engine.add_exchange(
            venue=self.venue,
            oms_type=OMSType.HEDGING,
            starting_balances=[Money(1_000_000, USD)],
            modules=[],
        )

        self.cache = self.engine.get_exec_engine().cache

    def test_exec_cache_check_integrity_when_cache_cleared_fails(self):
        # Arrange
        strategy = EMACross(
            instrument_id=self.usdjpy.id,
            bar_spec=BarSpecification(15, BarAggregation.MINUTE, PriceType.BID),
            trade_size=Decimal(1_000_000),
            fast_ema=10,
            slow_ema=20,
        )

        # Generate a lot of data
        self.engine.run(strategies=[strategy])

        # Remove data
        self.cache.clear_cache()

        # Act
        # Assert
        self.assertFalse(self.cache.check_integrity())

    def test_exec_cache_check_integrity_when_index_cleared_fails(self):
        # Arrange
        strategy = EMACross(
            instrument_id=self.usdjpy.id,
            bar_spec=BarSpecification(15, BarAggregation.MINUTE, PriceType.BID),
            trade_size=Decimal(1_000_000),
            fast_ema=10,
            slow_ema=20,
        )

        # Generate a lot of data
        self.engine.run(strategies=[strategy])

        # Clear index
        self.cache.clear_index()

        # Act
        # Assert
        self.assertFalse(self.cache.check_integrity())
