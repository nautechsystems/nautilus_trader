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

import unittest

from nautilus_trader.analysis.performance import PerformanceAnalyzer
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.common.logging import TestLogger
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.core.message import Event
from nautilus_trader.core.uuid import uuid4
from nautilus_trader.data.cache import DataCache
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.model.commands import AmendOrder
from nautilus_trader.model.commands import CancelOrder
from nautilus_trader.model.commands import SubmitBracketOrder
from nautilus_trader.model.commands import SubmitOrder
from nautilus_trader.model.commands import TradingCommand
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderState
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.events import OrderCancelled
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import OrderId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.order.bracket import BracketOrder
from nautilus_trader.model.position import Position
from nautilus_trader.trading.account import Account
from nautilus_trader.trading.portfolio import Portfolio
from nautilus_trader.trading.strategy import TradingStrategy
from tests.test_kit.mocks import MockExecutionClient
from tests.test_kit.mocks import MockExecutionDatabase
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
GBPUSD_SIM = TestInstrumentProvider.default_fx_ccy("GBP/USD")
BTCUSDT_BINANCE = TestInstrumentProvider.btcusdt_binance()


class ExecutionEngineTests(unittest.TestCase):
    def setUp(self):
        # Fixture Setup
        self.clock = TestClock()
        self.uuid_factory = UUIDFactory()
        self.logger = TestLogger(self.clock)

        self.trader_id = TraderId("TESTER", "000")
        self.account_id = TestStubs.account_id()

        self.order_factory = OrderFactory(
            trader_id=self.trader_id,
            strategy_id=StrategyId("S", "001"),
            clock=TestClock(),
        )

        self.portfolio = Portfolio(
            clock=self.clock,
            logger=self.logger,
        )
        self.portfolio.register_cache(DataCache(self.logger))

        self.analyzer = PerformanceAnalyzer()

        self.database = MockExecutionDatabase(
            trader_id=self.trader_id, logger=self.logger
        )
        self.exec_engine = ExecutionEngine(
            database=self.database,
            portfolio=self.portfolio,
            clock=self.clock,
            logger=self.logger,
        )

        self.cache = self.exec_engine.cache
        self.exec_engine.process(TestStubs.event_account_state())

        self.venue = Venue("SIM")
        self.exec_client = MockExecutionClient(
            name=self.venue.value,
            account_id=self.account_id,
            engine=self.exec_engine,
            clock=self.clock,
            logger=self.logger,
        )

        self.exec_engine.register_client(self.exec_client)

    def test_registered_venues_returns_expected(self):
        # Arrange
        # Act
        result = self.exec_engine.registered_clients

        # Assert
        self.assertEqual(["SIM"], result)

    def test_deregister_client_removes_client(self):
        # Arrange
        # Act
        self.exec_engine.deregister_client(self.exec_client)

        # Assert
        self.assertEqual([], self.exec_engine.registered_clients)

    def test_register_strategy(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            self.trader_id,
            self.clock,
            self.logger,
        )

        # Act
        self.exec_engine.register_strategy(strategy)

        # Assert
        self.assertIn(strategy.id, self.exec_engine.registered_strategies)

    def test_deregister_strategy(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        # Act
        self.exec_engine.deregister_strategy(strategy)

        # Assert
        self.assertNotIn(strategy.id, self.exec_engine.registered_strategies)

    def test_reset_retains_registered_strategies(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)  # Also registers with portfolio

        # Act
        self.exec_engine.reset()

        # Assert
        self.assertIn(strategy.id, self.exec_engine.registered_strategies)

    def test_check_connected_when_client_disconnected_returns_false(self):
        # Arrange
        self.exec_client.disconnect()

        # Act
        result = self.exec_engine.check_connected()

        # Assert
        self.assertFalse(result)

    def test_check_connected_when_client_connected_returns_true(self):
        # Arrange
        self.exec_client.connect()

        # Act
        result = self.exec_engine.check_connected()

        # Assert
        self.assertTrue(result)

    def test_check_disconnected_when_client_disconnected_returns_true(self):
        # Arrange
        # Act
        result = self.exec_engine.check_disconnected()

        # Assert
        self.assertTrue(result)

    def test_check_disconnected_when_client_connected_returns_false(self):
        # Arrange
        self.exec_client.connect()

        # Act
        result = self.exec_engine.check_disconnected()

        # Assert
        self.assertFalse(result)

    def test_check_integrity_calls_check_on_cache(self):
        # Arrange
        # Act
        result = self.exec_engine.check_integrity()

        # Assert
        self.assertTrue(result)  # No exceptions raised

    def test_loading_account_from_cache_registers_with_portfolio(self):
        # Arrange
        event = AccountState(
            AccountId("SIM", "001"),
            [Money(1_000_000, USD)],
            [Money(1_000_000, USD)],
            [Money(0, USD)],
            info={"default_currency": "USD"},  # Set the default currency
            event_id=uuid4(),
            timestamp_ns=0,
        )

        account = Account(event)
        self.database.add_account(account)

        # Act
        self.exec_engine.load_cache()

        # Assert
        self.assertEqual(account, self.portfolio.account(self.venue))

    def test_setting_of_position_id_counts(self):
        # Arrange
        strategy_id = StrategyId("S", "001")
        order = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity("1.00000000"),
        )

        order.apply(TestStubs.event_order_submitted(order))

        fill1 = TestStubs.event_order_filled(
            order,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-1-001"),
            strategy_id=strategy_id,
            last_px=Price("50000.00000000"),
        )

        order.apply(fill1)
        position = Position(fill1)

        self.database.add_order(order)
        self.database.update_order(order)
        self.database.add_position(position)

        # Act
        self.exec_engine.load_cache()

        # Assert
        self.assertEqual(1, self.exec_engine.position_id_count(strategy_id))

    def test_given_random_command_logs_and_continues(self):
        # Arrange
        random = TradingCommand(
            AUDUSD_SIM.id,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        self.exec_engine.execute(random)

    def test_given_random_event_logs_and_continues(self):
        # Arrange
        random = Event(
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        self.exec_engine.process(random)

    def test_submit_order_with_duplicate_cl_ord_id_logs(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000),
        )

        submit_order = SubmitOrder(
            order.instrument_id,
            self.trader_id,
            self.account_id,
            strategy.id,
            PositionId.null(),
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.exec_engine.execute(submit_order)
        self.exec_engine.process(TestStubs.event_order_submitted(order))
        self.exec_engine.execute(submit_order)  # Duplicate command

        # Assert
        self.assertEqual(OrderState.SUBMITTED, order.state)

    def test_submit_order_for_random_venue_logs(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        order = strategy.order_factory.market(
            InstrumentId(Symbol("AAPL"), Venue("NYSE")),
            OrderSide.BUY,
            Quantity(1000),
        )

        submit_order = SubmitOrder(
            order.instrument_id,
            self.trader_id,
            self.account_id,
            strategy.id,
            PositionId.null(),
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.exec_engine.execute(submit_order)

        # Assert
        self.assertEqual(1, self.exec_engine.command_count)
        self.assertEqual(OrderState.INITIALIZED, order.state)

    def test_submit_order_for_none_existent_position_id_invalidates_order(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000),
        )

        submit_order = SubmitOrder(
            order.instrument_id,
            self.trader_id,
            self.account_id,
            strategy.id,
            PositionId("RANDOM"),  # Invalid PositionId
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.exec_engine.execute(submit_order)

        # Assert
        self.assertEqual(OrderState.INVALID, order.state)

    def test_order_filled_with_unrecognized_strategy_id(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000),
        )

        submit_order = SubmitOrder(
            order.instrument_id,
            self.trader_id,
            self.account_id,
            strategy.id,
            PositionId.null(),
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.exec_engine.execute(submit_order)
        self.exec_engine.process(TestStubs.event_order_submitted(order))
        self.exec_engine.process(
            TestStubs.event_order_filled(
                order,
                AUDUSD_SIM,
                strategy_id=StrategyId("RANDOM", "001"),
            )
        )

        # Assert (does not send to strategy)
        self.assertEqual(OrderState.FILLED, order.state)

    def test_submit_bracket_order_with_all_duplicate_cl_ord_id_logs_does_not_submit(
        self,
    ):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        entry = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000),
        )

        stop_loss = strategy.order_factory.stop_market(
            AUDUSD_SIM.id, OrderSide.SELL, Quantity(100000), Price("0.50000")
        )

        take_profit = strategy.order_factory.limit(
            AUDUSD_SIM.id, OrderSide.SELL, Quantity(100000), Price("1.00000")
        )

        bracket = BracketOrder(
            entry=entry,
            stop_loss=stop_loss,
            take_profit=take_profit,
        )

        submit_bracket = SubmitBracketOrder(
            entry.instrument_id,
            self.trader_id,
            self.account_id,
            strategy.id,
            bracket,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.exec_engine.execute(submit_bracket)
        self.exec_engine.execute(submit_bracket)  # Duplicate command

        # Assert
        self.assertEqual(
            OrderState.INITIALIZED, entry.state
        )  # Did not invalidate originals
        self.assertEqual(
            OrderState.INITIALIZED, stop_loss.state
        )  # Did not invalidate originals
        self.assertEqual(
            OrderState.INITIALIZED, take_profit.state
        )  # Did not invalidate originals

    def test_submit_bracket_order_with_duplicate_take_profit_cl_ord_id_logs_does_not_submit(
        self,
    ):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        entry1 = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000),
        )

        stop_loss1 = strategy.order_factory.stop_market(
            AUDUSD_SIM.id, OrderSide.SELL, Quantity(100000), Price("0.50000")
        )

        take_profit1 = strategy.order_factory.limit(
            AUDUSD_SIM.id, OrderSide.SELL, Quantity(100000), Price("1.00000")
        )

        bracket1 = BracketOrder(
            entry=entry1,
            stop_loss=stop_loss1,
            take_profit=take_profit1,
        )

        submit_bracket1 = SubmitBracketOrder(
            entry1.instrument_id,
            self.trader_id,
            self.account_id,
            strategy.id,
            bracket1,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        entry2 = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000),
        )

        stop_loss2 = strategy.order_factory.stop_market(
            AUDUSD_SIM.id, OrderSide.SELL, Quantity(100000), Price("0.50000")
        )

        bracket2 = BracketOrder(
            entry=entry2,
            stop_loss=stop_loss2,
            take_profit=take_profit1,  # Duplicate
        )

        submit_bracket2 = SubmitBracketOrder(
            entry2.instrument_id,
            self.trader_id,
            self.account_id,
            strategy.id,
            bracket2,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.exec_engine.execute(submit_bracket1)
        self.exec_engine.process(TestStubs.event_order_submitted(entry1))
        self.exec_engine.process(TestStubs.event_order_accepted(entry1))
        self.exec_engine.process(TestStubs.event_order_submitted(stop_loss1))
        self.exec_engine.process(TestStubs.event_order_accepted(stop_loss1))
        self.exec_engine.process(TestStubs.event_order_submitted(take_profit1))
        self.exec_engine.process(TestStubs.event_order_accepted(take_profit1))
        self.exec_engine.execute(submit_bracket2)  # SL and TP

        # Assert
        self.assertEqual(OrderState.INVALID, entry2.state)
        self.assertEqual(OrderState.ACCEPTED, entry1.state)
        self.assertEqual(OrderState.ACCEPTED, stop_loss1.state)
        self.assertEqual(
            OrderState.ACCEPTED, take_profit1.state
        )  # Did not invalidate original

    def test_submit_bracket_order_with_duplicate_stop_loss_cl_ord_id_logs_does_not_submit(
        self,
    ):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        entry1 = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000),
        )

        stop_loss1 = strategy.order_factory.stop_market(
            AUDUSD_SIM.id, OrderSide.SELL, Quantity(100000), Price("0.50000")
        )

        take_profit1 = strategy.order_factory.limit(
            AUDUSD_SIM.id, OrderSide.SELL, Quantity(100000), Price("1.00000")
        )

        bracket1 = BracketOrder(
            entry=entry1,
            stop_loss=stop_loss1,
            take_profit=take_profit1,
        )

        submit_bracket1 = SubmitBracketOrder(
            entry1.instrument_id,
            self.trader_id,
            self.account_id,
            strategy.id,
            bracket1,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        entry2 = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000),
        )

        take_profit2 = strategy.order_factory.limit(
            AUDUSD_SIM.id, OrderSide.SELL, Quantity(100000), Price("1.00000")
        )

        bracket2 = BracketOrder(
            entry=entry2,
            stop_loss=stop_loss1,  # Duplicate
            take_profit=take_profit2,
        )

        submit_bracket2 = SubmitBracketOrder(
            entry2.instrument_id,
            self.trader_id,
            self.account_id,
            strategy.id,
            bracket2,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.exec_engine.execute(submit_bracket1)
        self.exec_engine.process(TestStubs.event_order_submitted(entry1))
        self.exec_engine.process(TestStubs.event_order_accepted(entry1))
        self.exec_engine.process(TestStubs.event_order_submitted(stop_loss1))
        self.exec_engine.process(TestStubs.event_order_accepted(stop_loss1))
        self.exec_engine.process(TestStubs.event_order_submitted(take_profit1))
        self.exec_engine.process(TestStubs.event_order_accepted(take_profit1))
        self.exec_engine.execute(submit_bracket2)  # SL and TP

        # Assert
        self.assertEqual(OrderState.INVALID, entry2.state)
        self.assertEqual(
            OrderState.ACCEPTED, entry1.state
        )  # Did not invalidate original
        self.assertEqual(
            OrderState.ACCEPTED, stop_loss1.state
        )  # Did not invalidate original
        self.assertEqual(
            OrderState.ACCEPTED, take_profit1.state
        )  # Did not invalidate original
        self.assertEqual(OrderState.INVALID, take_profit2.state)

    def test_submit_order(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000),
        )

        submit_order = SubmitOrder(
            order.instrument_id,
            self.trader_id,
            self.account_id,
            strategy.id,
            PositionId.null(),
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.exec_engine.execute(submit_order)

        # Assert
        self.assertIn(submit_order, self.exec_client.commands)
        self.assertTrue(self.cache.order_exists(order.cl_ord_id))

    def test_submit_order_with_cleared_cache_logs_error(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000),
        )

        submit_order = SubmitOrder(
            order.instrument_id,
            self.trader_id,
            self.account_id,
            strategy.id,
            PositionId.null(),
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.exec_engine.execute(submit_order)
        self.exec_engine.cache.clear_cache()
        self.exec_engine.process(TestStubs.event_order_accepted(order))

        # Assert
        self.assertEqual(OrderState.INITIALIZED, order.state)

    def test_when_applying_event_to_order_with_invalid_state_trigger_logs(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000),
        )

        submit_order = SubmitOrder(
            order.instrument_id,
            self.trader_id,
            self.account_id,
            strategy.id,
            PositionId.null(),
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        # Act (event attempts to fill order before its submitted)
        self.exec_engine.execute(submit_order)
        self.exec_engine.process(TestStubs.event_order_filled(order, AUDUSD_SIM))

        # Assert
        self.assertEqual(OrderState.INITIALIZED, order.state)

    def test_order_filled_event_when_order_not_found_in_cache_logs(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000),
        )

        # Act (event attempts to fill order before its submitted)
        self.exec_engine.process(TestStubs.event_order_filled(order, AUDUSD_SIM))

        # Assert
        self.assertEqual(2, self.exec_engine.event_count)
        self.assertEqual(OrderState.INITIALIZED, order.state)

    def test_cancel_order_for_already_completed_order_logs_and_does_nothing(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        # Push order state to filled (completed)
        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000),
        )

        submit_order = SubmitOrder(
            order.instrument_id,
            self.trader_id,
            self.account_id,
            strategy.id,
            PositionId.null(),
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        self.exec_engine.execute(submit_order)
        self.exec_engine.process(TestStubs.event_order_submitted(order))
        self.exec_engine.process(TestStubs.event_order_accepted(order))
        self.exec_engine.process(TestStubs.event_order_filled(order, AUDUSD_SIM))

        cancel_order = CancelOrder(
            order.instrument_id,
            self.trader_id,
            self.account_id,
            order.cl_ord_id,
            OrderId("1"),
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.exec_engine.execute(cancel_order)

        # Assert
        self.assertEqual(OrderState.FILLED, order.state)

    def test_amend_order_for_already_completed_order_logs_and_does_nothing(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        # Push order state to filled (completed)
        order = strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000),
            Price("0.85101"),
        )

        submit_order = SubmitOrder(
            order.instrument_id,
            self.trader_id,
            self.account_id,
            strategy.id,
            PositionId.null(),
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        self.exec_engine.execute(submit_order)
        self.exec_engine.process(TestStubs.event_order_submitted(order))
        self.exec_engine.process(TestStubs.event_order_accepted(order))
        self.exec_engine.process(TestStubs.event_order_filled(order, AUDUSD_SIM))

        amend_order = AmendOrder(
            order.instrument_id,
            self.trader_id,
            self.account_id,
            order.cl_ord_id,
            Quantity(200000),
            order.price,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.exec_engine.execute(amend_order)

        # Assert
        self.assertEqual(OrderState.FILLED, order.state)
        self.assertEqual(Quantity(100000), order.quantity)

    def test_handle_order_event_with_random_cl_ord_id_and_order_id_cached(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000),
        )

        submit_order = SubmitOrder(
            order.instrument_id,
            self.trader_id,
            self.account_id,
            strategy.id,
            PositionId.null(),
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        self.exec_engine.execute(submit_order)
        self.exec_engine.process(TestStubs.event_order_submitted(order))
        self.exec_engine.process(TestStubs.event_order_accepted(order))

        cancelled = OrderCancelled(
            self.account_id,
            ClientOrderId("web_001"),  # Random id from say a web UI
            order.id,
            self.clock.timestamp_ns(),
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.exec_engine.process(cancelled)

        # Assert (order was found and OrderCancelled event was applied)
        self.assertEqual(OrderState.CANCELLED, order.state)

    def test_handle_order_event_with_random_cl_ord_id_and_order_id_not_cached(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000),
        )

        submit_order = SubmitOrder(
            order.instrument_id,
            self.trader_id,
            self.account_id,
            strategy.id,
            PositionId.null(),
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        self.exec_engine.execute(submit_order)
        self.exec_engine.process(TestStubs.event_order_submitted(order))
        self.exec_engine.process(TestStubs.event_order_accepted(order))

        cancelled = OrderCancelled(
            self.account_id,
            ClientOrderId("web_001"),  # Random id from say a web UI
            OrderId("RANDOM_001"),  # Also a random order id the engine won't find
            self.clock.timestamp_ns(),
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.exec_engine.process(cancelled)

        # Assert (order was not found, engine did not crash)
        self.assertEqual(OrderState.ACCEPTED, order.state)

    def test_handle_duplicate_order_events_logs_error_and_does_not_apply(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000),
        )

        submit_order = SubmitOrder(
            order.instrument_id,
            self.trader_id,
            self.account_id,
            strategy.id,
            PositionId.null(),
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        self.exec_engine.execute(submit_order)
        self.exec_engine.process(TestStubs.event_order_submitted(order))
        self.exec_engine.process(TestStubs.event_order_accepted(order))

        cancelled = OrderCancelled(
            self.account_id,
            ClientOrderId("web_001"),  # Random id from say a web UI
            order.id,
            self.clock.timestamp_ns(),
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.exec_engine.process(cancelled)
        self.exec_engine.process(cancelled)

        # Assert (order was found and OrderCancelled event was applied)
        self.assertEqual(OrderState.CANCELLED, order.state)
        self.assertEqual(4, order.event_count)

    def test_handle_order_fill_event_with_no_strategy_id_correctly_handles_fill(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000),
        )

        submit_order = SubmitOrder(
            order.instrument_id,
            self.trader_id,
            self.account_id,
            strategy.id,
            PositionId.null(),
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        self.exec_engine.execute(submit_order)

        # Act
        self.exec_engine.process(TestStubs.event_order_submitted(order))
        self.exec_engine.process(TestStubs.event_order_accepted(order))
        self.exec_engine.process(
            TestStubs.event_order_filled(
                order=order,
                instrument=AUDUSD_SIM,
                strategy_id=StrategyId.null(),
            )
        )

        expected_position_id = PositionId("P-19700101-000000-000-001-1")

        # Assert
        self.assertTrue(self.cache.position_exists(expected_position_id))
        self.assertTrue(self.cache.is_position_open(expected_position_id))
        self.assertFalse(self.cache.is_position_closed(expected_position_id))
        self.assertEqual(Position, type(self.cache.position(expected_position_id)))
        self.assertIn(expected_position_id, self.cache.position_ids())
        self.assertNotIn(
            expected_position_id,
            self.cache.position_closed_ids(strategy_id=strategy.id),
        )
        self.assertNotIn(expected_position_id, self.cache.position_closed_ids())
        self.assertIn(
            expected_position_id, self.cache.position_open_ids(strategy_id=strategy.id)
        )
        self.assertIn(expected_position_id, self.cache.position_open_ids())
        self.assertEqual(1, self.cache.positions_total_count())
        self.assertEqual(1, self.cache.positions_open_count())
        self.assertEqual(0, self.cache.positions_closed_count())

    def test_handle_order_fill_event_with_no_position_id_correctly_handles_fill(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000),
        )

        submit_order = SubmitOrder(
            order.instrument_id,
            self.trader_id,
            self.account_id,
            strategy.id,
            PositionId.null(),
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        self.exec_engine.execute(submit_order)

        # Act
        self.exec_engine.process(TestStubs.event_order_submitted(order))
        self.exec_engine.process(TestStubs.event_order_accepted(order))
        self.exec_engine.process(
            TestStubs.event_order_filled(
                order=order,
                instrument=AUDUSD_SIM,
                strategy_id=StrategyId.null(),
            )
        )

        expected_position_id = PositionId("P-19700101-000000-000-001-1")

        # Assert
        self.assertTrue(self.cache.position_exists(expected_position_id))
        self.assertTrue(self.cache.is_position_open(expected_position_id))
        self.assertFalse(self.cache.is_position_closed(expected_position_id))
        self.assertEqual(Position, type(self.cache.position(expected_position_id)))
        self.assertIn(expected_position_id, self.cache.position_ids())
        self.assertNotIn(
            expected_position_id,
            self.cache.position_closed_ids(strategy_id=strategy.id),
        )
        self.assertNotIn(expected_position_id, self.cache.position_closed_ids())
        self.assertIn(
            expected_position_id, self.cache.position_open_ids(strategy_id=strategy.id)
        )
        self.assertIn(expected_position_id, self.cache.position_open_ids())
        self.assertEqual(1, self.cache.positions_total_count())
        self.assertEqual(1, self.cache.positions_open_count())
        self.assertEqual(0, self.cache.positions_closed_count())

    def test_handle_order_fill_event(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000),
        )

        submit_order = SubmitOrder(
            order.instrument_id,
            self.trader_id,
            self.account_id,
            strategy.id,
            PositionId.null(),
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        self.exec_engine.execute(submit_order)

        # Act
        self.exec_engine.process(TestStubs.event_order_submitted(order))
        self.exec_engine.process(TestStubs.event_order_accepted(order))
        self.exec_engine.process(TestStubs.event_order_filled(order, AUDUSD_SIM))

        expected_position_id = PositionId("P-19700101-000000-000-001-1")

        # Assert
        self.assertTrue(self.cache.position_exists(expected_position_id))
        self.assertTrue(self.cache.is_position_open(expected_position_id))
        self.assertFalse(self.cache.is_position_closed(expected_position_id))
        self.assertEqual(Position, type(self.cache.position(expected_position_id)))
        self.assertIn(expected_position_id, self.cache.position_ids())
        self.assertNotIn(
            expected_position_id,
            self.cache.position_closed_ids(strategy_id=strategy.id),
        )
        self.assertNotIn(expected_position_id, self.cache.position_closed_ids())
        self.assertIn(
            expected_position_id, self.cache.position_open_ids(strategy_id=strategy.id)
        )
        self.assertIn(expected_position_id, self.cache.position_open_ids())
        self.assertEqual(1, self.cache.positions_total_count())
        self.assertEqual(1, self.cache.positions_open_count())
        self.assertEqual(0, self.cache.positions_closed_count())

    def test_handle_multiple_partial_fill_events(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000),
        )

        submit_order = SubmitOrder(
            order.instrument_id,
            self.trader_id,
            self.account_id,
            strategy.id,
            PositionId.null(),
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        self.exec_engine.execute(submit_order)
        self.exec_engine.process(TestStubs.event_order_submitted(order))
        self.exec_engine.process(TestStubs.event_order_accepted(order))

        # Act
        expected_position_id = PositionId("P-19700101-000000-000-001-1")

        self.exec_engine.process(
            TestStubs.event_order_filled(
                order=order, instrument=AUDUSD_SIM, last_qty=Quantity(20100)
            ),
        )

        self.exec_engine.process(
            TestStubs.event_order_filled(
                order=order, instrument=AUDUSD_SIM, last_qty=Quantity(19900)
            ),
        )

        self.exec_engine.process(
            TestStubs.event_order_filled(
                order=order, instrument=AUDUSD_SIM, last_qty=Quantity(60000)
            ),
        )

        # Assert
        self.assertTrue(self.cache.position_exists(expected_position_id))
        self.assertTrue(self.cache.is_position_open(expected_position_id))
        self.assertFalse(self.cache.is_position_closed(expected_position_id))
        self.assertEqual(Position, type(self.cache.position(expected_position_id)))
        self.assertIn(expected_position_id, self.cache.position_ids())
        self.assertNotIn(
            expected_position_id,
            self.cache.position_closed_ids(strategy_id=strategy.id),
        )
        self.assertNotIn(expected_position_id, self.cache.position_closed_ids())
        self.assertIn(
            expected_position_id, self.cache.position_open_ids(strategy_id=strategy.id)
        )
        self.assertIn(expected_position_id, self.cache.position_open_ids())
        self.assertEqual(1, self.cache.positions_total_count())
        self.assertEqual(1, self.cache.positions_open_count())
        self.assertEqual(0, self.cache.positions_closed_count())

    def test_handle_position_opening_with_position_id_none(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000),
        )

        submit_order = SubmitOrder(
            order.instrument_id,
            self.trader_id,
            self.account_id,
            strategy.id,
            PositionId.null(),
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        self.exec_engine.execute(submit_order)

        # Act
        self.exec_engine.process(TestStubs.event_order_submitted(order))
        self.exec_engine.process(TestStubs.event_order_accepted(order))
        self.exec_engine.process(
            TestStubs.event_order_filled(
                order, AUDUSD_SIM, position_id=PositionId.null()
            )
        )

        expected_id = PositionId(
            "P-19700101-000000-000-001-1"
        )  # Generated inside engine

        # Assert
        self.assertTrue(self.cache.position_exists(expected_id))
        self.assertTrue(self.cache.is_position_open(expected_id))
        self.assertFalse(self.cache.is_position_closed(expected_id))
        self.assertEqual(Position, type(self.cache.position(expected_id)))
        self.assertIn(expected_id, self.cache.position_ids())
        self.assertNotIn(
            expected_id, self.cache.position_closed_ids(strategy_id=strategy.id)
        )
        self.assertNotIn(expected_id, self.cache.position_closed_ids())
        self.assertIn(
            expected_id, self.cache.position_open_ids(strategy_id=strategy.id)
        )
        self.assertIn(expected_id, self.cache.position_open_ids())
        self.assertEqual(1, self.cache.positions_total_count())
        self.assertEqual(1, self.cache.positions_open_count())
        self.assertEqual(0, self.cache.positions_closed_count())

    def test_add_to_existing_position_on_order_fill(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        order1 = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000),
        )

        order2 = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000),
        )

        submit_order1 = SubmitOrder(
            order1.instrument_id,
            self.trader_id,
            self.account_id,
            strategy.id,
            PositionId.null(),
            order1,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        self.exec_engine.execute(submit_order1)
        self.exec_engine.process(TestStubs.event_order_submitted(order1))
        self.exec_engine.process(TestStubs.event_order_accepted(order1))
        self.exec_engine.process(TestStubs.event_order_filled(order1, AUDUSD_SIM))

        expected_position_id = PositionId("P-19700101-000000-000-001-1")

        submit_order2 = SubmitOrder(
            order2.instrument_id,
            self.trader_id,
            self.account_id,
            strategy.id,
            expected_position_id,
            order2,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.exec_engine.execute(submit_order2)
        self.exec_engine.process(TestStubs.event_order_submitted(order2))
        self.exec_engine.process(TestStubs.event_order_accepted(order2))
        self.exec_engine.process(
            TestStubs.event_order_filled(
                order2, AUDUSD_SIM, position_id=expected_position_id
            )
        )

        # Assert
        self.assertTrue(self.cache.position_exists(expected_position_id))
        self.assertTrue(self.cache.is_position_open(expected_position_id))
        self.assertFalse(self.cache.is_position_closed(expected_position_id))
        self.assertEqual(Position, type(self.cache.position(expected_position_id)))
        self.assertEqual(0, len(self.cache.positions_closed(strategy_id=strategy.id)))
        self.assertEqual(0, len(self.cache.positions_closed()))
        self.assertEqual(1, len(self.cache.positions_open(strategy_id=strategy.id)))
        self.assertEqual(1, len(self.cache.positions_open()))
        self.assertEqual(1, self.cache.positions_total_count())
        self.assertEqual(1, self.cache.positions_open_count())
        self.assertEqual(0, self.cache.positions_closed_count())

    def test_close_position_on_order_fill(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        order1 = strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000),
            Price("1.00000"),
        )

        order2 = strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity(100000),
            Price("1.00000"),
        )

        submit_order1 = SubmitOrder(
            order1.instrument_id,
            self.trader_id,
            self.account_id,
            strategy.id,
            PositionId.null(),
            order1,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        position_id = PositionId("P-1")

        self.exec_engine.execute(submit_order1)
        self.exec_engine.process(TestStubs.event_order_submitted(order1))
        self.exec_engine.process(TestStubs.event_order_accepted(order1))
        self.exec_engine.process(
            TestStubs.event_order_filled(order1, AUDUSD_SIM, position_id=position_id)
        )

        submit_order2 = SubmitOrder(
            order2.instrument_id,
            self.trader_id,
            self.account_id,
            strategy.id,
            position_id,
            order2,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.exec_engine.execute(submit_order2)
        self.exec_engine.process(TestStubs.event_order_submitted(order2))
        self.exec_engine.process(TestStubs.event_order_accepted(order2))
        self.exec_engine.process(
            TestStubs.event_order_filled(order2, AUDUSD_SIM, position_id=position_id)
        )

        # # Assert
        self.assertTrue(self.cache.position_exists(position_id))
        self.assertFalse(self.cache.is_position_open(position_id))
        self.assertTrue(self.cache.is_position_closed(position_id))
        self.assertEqual(position_id, self.cache.position(position_id).id)
        self.assertEqual(
            position_id, self.cache.positions(strategy_id=strategy.id)[0].id
        )
        self.assertEqual(position_id, self.cache.positions()[0].id)
        self.assertEqual(0, len(self.cache.positions_open(strategy_id=strategy.id)))
        self.assertEqual(0, len(self.cache.positions_open()))
        self.assertEqual(
            position_id, self.cache.positions_closed(strategy_id=strategy.id)[0].id
        )
        self.assertEqual(position_id, self.cache.positions_closed()[0].id)
        self.assertNotIn(
            position_id, self.cache.position_open_ids(strategy_id=strategy.id)
        )
        self.assertNotIn(position_id, self.cache.position_open_ids())
        self.assertEqual(1, self.cache.positions_total_count())
        self.assertEqual(0, self.cache.positions_open_count())
        self.assertEqual(1, self.cache.positions_closed_count())

    def test_multiple_strategy_positions_opened(self):
        # Arrange
        self.exec_engine.start()

        strategy1 = TradingStrategy(order_id_tag="001")
        strategy1.register_trader(
            TraderId("TESTER", "000"),
            self.clock,
            self.logger,
        )

        strategy2 = TradingStrategy(order_id_tag="002")
        strategy2.register_trader(
            TraderId("TESTER", "000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy1)
        self.exec_engine.register_strategy(strategy2)

        order1 = strategy1.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000),
            Price("1.00000"),
        )

        order2 = strategy2.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000),
            Price("1.00000"),
        )

        submit_order1 = SubmitOrder(
            order1.instrument_id,
            self.trader_id,
            self.account_id,
            strategy1.id,
            PositionId.null(),
            order1,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        submit_order2 = SubmitOrder(
            order2.instrument_id,
            self.trader_id,
            self.account_id,
            strategy2.id,
            PositionId.null(),
            order2,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        position1_id = PositionId("P-1")
        position2_id = PositionId("P-2")

        # Act
        self.exec_engine.execute(submit_order1)
        self.exec_engine.execute(submit_order2)
        self.exec_engine.process(TestStubs.event_order_submitted(order1))
        self.exec_engine.process(TestStubs.event_order_accepted(order1))
        self.exec_engine.process(
            TestStubs.event_order_filled(order1, AUDUSD_SIM, position_id=position1_id)
        )
        self.exec_engine.process(TestStubs.event_order_submitted(order2))
        self.exec_engine.process(TestStubs.event_order_accepted(order2))
        self.exec_engine.process(
            TestStubs.event_order_filled(order2, AUDUSD_SIM, position_id=position2_id)
        )

        # Assert
        self.assertTrue(self.cache.position_exists(position1_id))
        self.assertTrue(self.cache.position_exists(position2_id))
        self.assertTrue(self.cache.is_position_open(position1_id))
        self.assertTrue(self.cache.is_position_open(position2_id))
        self.assertFalse(self.cache.is_position_closed(position1_id))
        self.assertFalse(self.cache.is_position_closed(position2_id))
        self.assertEqual(Position, type(self.cache.position(position1_id)))
        self.assertEqual(Position, type(self.cache.position(position2_id)))
        self.assertIn(position1_id, self.cache.position_ids(strategy_id=strategy1.id))
        self.assertIn(position2_id, self.cache.position_ids(strategy_id=strategy2.id))
        self.assertIn(position1_id, self.cache.position_ids())
        self.assertIn(position2_id, self.cache.position_ids())
        self.assertEqual(2, len(self.cache.position_open_ids()))
        self.assertEqual(1, len(self.cache.positions_open(strategy_id=strategy1.id)))
        self.assertEqual(1, len(self.cache.positions_open(strategy_id=strategy2.id)))
        self.assertEqual(1, len(self.cache.positions_open(strategy_id=strategy2.id)))
        self.assertEqual(2, len(self.cache.positions_open()))
        self.assertEqual(1, len(self.cache.positions_open(strategy_id=strategy1.id)))
        self.assertEqual(1, len(self.cache.positions_open(strategy_id=strategy2.id)))
        self.assertIn(
            position1_id, self.cache.position_open_ids(strategy_id=strategy1.id)
        )
        self.assertIn(
            position2_id, self.cache.position_open_ids(strategy_id=strategy2.id)
        )
        self.assertIn(position1_id, self.cache.position_open_ids())
        self.assertIn(position2_id, self.cache.position_open_ids())
        self.assertNotIn(
            position1_id, self.cache.position_closed_ids(strategy_id=strategy1.id)
        )
        self.assertNotIn(
            position2_id, self.cache.position_closed_ids(strategy_id=strategy2.id)
        )
        self.assertNotIn(position1_id, self.cache.position_closed_ids())
        self.assertNotIn(position2_id, self.cache.position_closed_ids())
        self.assertEqual(2, self.cache.positions_total_count())
        self.assertEqual(2, self.cache.positions_open_count())
        self.assertEqual(0, self.cache.positions_closed_count())

    def test_multiple_strategy_positions_one_active_one_closed(self):
        # Arrange
        self.exec_engine.start()

        strategy1 = TradingStrategy(order_id_tag="001")
        strategy1.register_trader(
            TraderId("TESTER", "000"),
            self.clock,
            self.logger,
        )

        strategy2 = TradingStrategy(order_id_tag="002")
        strategy2.register_trader(
            TraderId("TESTER", "000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy1)
        self.exec_engine.register_strategy(strategy2)

        order1 = strategy1.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000),
            Price("1.00000"),
        )

        order2 = strategy1.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity(100000),
            Price("1.00000"),
        )

        order3 = strategy2.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000),
            Price("1.00000"),
        )

        submit_order1 = SubmitOrder(
            order1.instrument_id,
            self.trader_id,
            self.account_id,
            strategy1.id,
            PositionId.null(),
            order1,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        position_id1 = PositionId("P-1")

        submit_order2 = SubmitOrder(
            order2.instrument_id,
            self.trader_id,
            self.account_id,
            strategy1.id,
            position_id1,
            order2,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        submit_order3 = SubmitOrder(
            order2.instrument_id,
            self.trader_id,
            self.account_id,
            strategy2.id,
            PositionId.null(),
            order3,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        position_id2 = PositionId("P-2")

        # Act
        self.exec_engine.execute(submit_order1)
        self.exec_engine.process(TestStubs.event_order_submitted(order1))
        self.exec_engine.process(TestStubs.event_order_accepted(order1))
        self.exec_engine.process(
            TestStubs.event_order_filled(order1, AUDUSD_SIM, position_id=position_id1)
        )

        self.exec_engine.execute(submit_order2)
        self.exec_engine.process(TestStubs.event_order_submitted(order2))
        self.exec_engine.process(TestStubs.event_order_accepted(order2))
        self.exec_engine.process(
            TestStubs.event_order_filled(order2, AUDUSD_SIM, position_id=position_id1)
        )

        self.exec_engine.execute(submit_order3)
        self.exec_engine.process(TestStubs.event_order_submitted(order3))
        self.exec_engine.process(TestStubs.event_order_accepted(order3))
        self.exec_engine.process(
            TestStubs.event_order_filled(order3, AUDUSD_SIM, position_id=position_id2)
        )

        # Assert
        # Already tested .is_position_active and .is_position_closed above
        self.assertTrue(self.cache.position_exists(position_id1))
        self.assertTrue(self.cache.position_exists(position_id2))
        self.assertIn(position_id1, self.cache.position_ids(strategy_id=strategy1.id))
        self.assertIn(position_id2, self.cache.position_ids(strategy_id=strategy2.id))
        self.assertIn(position_id1, self.cache.position_ids())
        self.assertIn(position_id2, self.cache.position_ids())
        self.assertEqual(0, len(self.cache.positions_open(strategy_id=strategy1.id)))
        self.assertEqual(1, len(self.cache.positions_open(strategy_id=strategy2.id)))
        self.assertEqual(1, len(self.cache.positions_open()))
        self.assertEqual(1, len(self.cache.positions_closed()))
        self.assertEqual(2, len(self.cache.positions()))
        self.assertNotIn(
            position_id1, self.cache.position_open_ids(strategy_id=strategy1.id)
        )
        self.assertIn(
            position_id2, self.cache.position_open_ids(strategy_id=strategy2.id)
        )
        self.assertNotIn(position_id1, self.cache.position_open_ids())
        self.assertIn(position_id2, self.cache.position_open_ids())
        self.assertIn(
            position_id1, self.cache.position_closed_ids(strategy_id=strategy1.id)
        )
        self.assertNotIn(
            position_id2, self.cache.position_closed_ids(strategy_id=strategy2.id)
        )
        self.assertIn(position_id1, self.cache.position_closed_ids())
        self.assertNotIn(position_id2, self.cache.position_closed_ids())
        self.assertEqual(2, self.cache.positions_total_count())
        self.assertEqual(1, self.cache.positions_open_count())
        self.assertEqual(1, self.cache.positions_closed_count())

    def test_flip_position_on_opposite_filled_same_position_sell(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        order1 = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100000),
        )

        order2 = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity(150000),
        )

        submit_order1 = SubmitOrder(
            order1.instrument_id,
            self.trader_id,
            self.account_id,
            strategy.id,
            PositionId.null(),
            order1,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        position_id = PositionId("P-19700101-000000-000-001-1")

        self.exec_engine.execute(submit_order1)
        self.exec_engine.process(TestStubs.event_order_submitted(order1))
        self.exec_engine.process(TestStubs.event_order_accepted(order1))
        self.exec_engine.process(
            TestStubs.event_order_filled(order1, AUDUSD_SIM, position_id=position_id)
        )

        submit_order2 = SubmitOrder(
            order2.instrument_id,
            self.trader_id,
            self.account_id,
            strategy.id,
            position_id,
            order2,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.exec_engine.execute(submit_order2)
        self.exec_engine.process(TestStubs.event_order_submitted(order2))
        self.exec_engine.process(TestStubs.event_order_accepted(order2))
        self.exec_engine.process(
            TestStubs.event_order_filled(order2, AUDUSD_SIM, position_id=position_id)
        )

        # Assert
        position_id_flipped = PositionId("P-19700101-000000-000-001-1F")
        position_flipped = self.cache.position(position_id_flipped)

        self.assertEqual(-50000, position_flipped.relative_qty)
        self.assertEqual(50000, position_flipped.last_event.fill_qty)
        self.assertEqual(150000, position_flipped.last_event.cum_qty)
        self.assertEqual(0, position_flipped.last_event.leaves_qty)
        self.assertEqual(
            Quantity(100000), self.cache.order(order1.cl_ord_id).last_event.cum_qty
        )
        self.assertEqual(0, self.cache.order(order1.cl_ord_id).last_event.leaves_qty)
        self.assertTrue(self.cache.position_exists(position_id))
        self.assertTrue(self.cache.position_exists(position_id_flipped))
        self.assertTrue(self.cache.is_position_closed(position_id))
        self.assertTrue(self.cache.is_position_open(position_id_flipped))
        self.assertIn(position_id, self.cache.position_ids())
        self.assertIn(position_id, self.cache.position_ids(strategy_id=strategy.id))
        self.assertIn(position_id_flipped, self.cache.position_ids())
        self.assertIn(
            position_id_flipped, self.cache.position_ids(strategy_id=strategy.id)
        )
        self.assertEqual(2, self.cache.positions_total_count())
        self.assertEqual(1, self.cache.positions_open_count())
        self.assertEqual(1, self.cache.positions_closed_count())

    def test_flip_position_on_opposite_filled_same_position_buy(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        order1 = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity(100000),
        )

        order2 = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(150000),
        )

        submit_order1 = SubmitOrder(
            order1.instrument_id,
            self.trader_id,
            self.account_id,
            strategy.id,
            PositionId.null(),
            order1,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        position_id = PositionId("P-19700101-000000-000-001-1")

        self.exec_engine.execute(submit_order1)
        self.exec_engine.process(TestStubs.event_order_submitted(order1))
        self.exec_engine.process(TestStubs.event_order_accepted(order1))
        self.exec_engine.process(
            TestStubs.event_order_filled(order1, AUDUSD_SIM, position_id=position_id)
        )

        submit_order2 = SubmitOrder(
            order2.instrument_id,
            self.trader_id,
            self.account_id,
            strategy.id,
            position_id,
            order2,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.exec_engine.execute(submit_order2)
        self.exec_engine.process(TestStubs.event_order_submitted(order2))
        self.exec_engine.process(TestStubs.event_order_accepted(order2))
        self.exec_engine.process(
            TestStubs.event_order_filled(order2, AUDUSD_SIM, position_id=position_id)
        )

        # Assert
        position_id_flipped = PositionId("P-19700101-000000-000-001-1F")
        position_flipped = self.cache.position(position_id_flipped)

        self.assertEqual(50000, position_flipped.relative_qty)
        self.assertEqual(50000, position_flipped.last_event.fill_qty)
        self.assertEqual(150000, position_flipped.last_event.cum_qty)
        self.assertEqual(0, position_flipped.last_event.leaves_qty)
        self.assertEqual(
            Quantity(100000), self.cache.order(order1.cl_ord_id).last_event.cum_qty
        )
        self.assertEqual(0, self.cache.order(order1.cl_ord_id).last_event.leaves_qty)
        self.assertTrue(self.cache.position_exists(position_id))
        self.assertTrue(self.cache.position_exists(position_id_flipped))
        self.assertTrue(self.cache.is_position_closed(position_id))
        self.assertTrue(self.cache.is_position_open(position_id_flipped))
        self.assertIn(position_id, self.cache.position_ids())
        self.assertIn(position_id, self.cache.position_ids(strategy_id=strategy.id))
        self.assertIn(position_id_flipped, self.cache.position_ids())
        self.assertIn(
            position_id_flipped, self.cache.position_ids(strategy_id=strategy.id)
        )
        self.assertEqual(2, self.cache.positions_total_count())
        self.assertEqual(1, self.cache.positions_open_count())
        self.assertEqual(1, self.cache.positions_closed_count())
