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

from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.core.message import Event
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.model.commands.trading import CancelOrder
from nautilus_trader.model.commands.trading import SubmitBracketOrder
from nautilus_trader.model.commands.trading import SubmitOrder
from nautilus_trader.model.commands.trading import TradingCommand
from nautilus_trader.model.commands.trading import UpdateOrder
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderState
from nautilus_trader.model.enums import VenueType
from nautilus_trader.model.events.order import OrderCanceled
from nautilus_trader.model.events.order import OrderUpdated
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders.bracket import BracketOrder
from nautilus_trader.model.position import Position
from nautilus_trader.msgbus.message_bus import MessageBus
from nautilus_trader.risk.engine import RiskEngine
from nautilus_trader.trading.portfolio import Portfolio
from nautilus_trader.trading.strategy import TradingStrategy
from tests.test_kit.mocks import MockCacheDatabase
from tests.test_kit.mocks import MockExecutionClient
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
GBPUSD_SIM = TestInstrumentProvider.default_fx_ccy("GBP/USD")
BTCUSDT_BINANCE = TestInstrumentProvider.btcusdt_binance()


class TestExecutionEngine:
    def setup(self):
        # Fixture Setup
        self.clock = TestClock()
        self.uuid_factory = UUIDFactory()
        self.logger = Logger(self.clock)

        self.trader_id = TestStubs.trader_id()
        self.strategy_id = TestStubs.strategy_id()
        self.account_id = TestStubs.account_id()

        self.order_factory = OrderFactory(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            clock=TestClock(),
        )

        self.msgbus = MessageBus(
            clock=self.clock,
            logger=self.logger,
        )

        self.cache_db = MockCacheDatabase(
            trader_id=self.trader_id,
            logger=self.logger,
        )

        self.cache = Cache(
            database=self.cache_db,
            logger=self.logger,
        )

        self.portfolio = Portfolio(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.data_engine = DataEngine(
            portfolio=self.portfolio,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.exec_engine = ExecutionEngine(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.risk_engine = RiskEngine(
            exec_engine=self.exec_engine,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Prepare components
        self.cache.add_instrument(AUDUSD_SIM)
        self.exec_engine.process(TestStubs.event_account_state())

        self.venue = Venue("SIM")
        self.exec_client = MockExecutionClient(
            client_id=ClientId(self.venue.value),
            venue_type=VenueType.ECN,
            account_id=self.account_id,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            engine=self.exec_engine,
            clock=self.clock,
            logger=self.logger,
        )

        self.exec_engine.register_client(self.exec_client)

    def test_registered_clients_returns_expected(self):
        # Arrange
        # Act
        result = self.exec_engine.registered_clients

        # Assert
        assert result == [ClientId("SIM")]
        assert self.exec_engine.default_client is None

    def test_register_brokerage_multi_venue_exec_client(self):
        # Arrange
        exec_client = MockExecutionClient(
            client_id=ClientId("IB"),
            venue_type=VenueType.BROKERAGE_MULTI_VENUE,
            account_id=AccountId("IB", "U1258001"),
            account_type=AccountType.MARGIN,
            base_currency=USD,
            engine=self.exec_engine,
            clock=self.clock,
            logger=self.logger,
        )

        # Act
        self.exec_engine.register_client(exec_client)

        # Assert
        assert self.exec_engine.default_client == exec_client.id
        assert self.exec_engine.registered_clients == [
            exec_client.id,
            self.exec_client.id,
        ]

    def test_register_venue_routing(self):
        # Arrange
        exec_client = MockExecutionClient(
            client_id=ClientId("IB"),
            venue_type=VenueType.BROKERAGE_MULTI_VENUE,
            account_id=AccountId("IB", "U1258001"),
            account_type=AccountType.MARGIN,
            base_currency=USD,
            engine=self.exec_engine,
            clock=self.clock,
            logger=self.logger,
        )

        # Act
        self.exec_engine.register_venue_routing(exec_client, Venue("NYMEX"))

        # Assert
        assert self.exec_engine.default_client is None
        assert self.exec_engine.registered_clients == [
            exec_client.id,
            self.exec_client.id,
        ]

    def test_deregister_client_removes_client(self):
        # Arrange
        # Act
        self.exec_engine.deregister_client(self.exec_client)

        # Assert
        assert self.exec_engine.registered_clients == []

    def test_check_connected_when_client_disconnected_returns_false(self):
        # Arrange
        self.exec_client.disconnect()

        # Act
        result = self.exec_engine.check_connected()

        # Assert
        assert not result

    def test_check_connected_when_client_connected_returns_true(self):
        # Arrange
        self.exec_client.connect()

        # Act
        result = self.exec_engine.check_connected()

        # Assert
        assert result

    def test_check_disconnected_when_client_disconnected_returns_true(self):
        # Arrange
        # Act
        result = self.exec_engine.check_disconnected()

        # Assert
        assert result

    def test_check_disconnected_when_client_connected_returns_false(self):
        # Arrange
        self.exec_client.connect()

        # Act
        result = self.exec_engine.check_disconnected()

        # Assert
        assert not result

    def test_check_integrity_calls_check_on_cache(self):
        # Arrange
        # Act
        result = self.exec_engine.check_integrity()

        # Assert
        assert result  # No exceptions raised

    def test_loading_account_from_cache_registers_with_portfolio(self):
        # Arrange, Act, Assert
        assert self.portfolio.account(self.venue).id == AccountId("SIM", "000")

    def test_setting_of_position_id_counts(self):
        # Arrange
        strategy_id = StrategyId("S-001")
        order = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_str("1.00000000"),
        )

        order.apply(TestStubs.event_order_submitted(order))

        fill1 = TestStubs.event_order_filled(
            order,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-1-001"),
            strategy_id=strategy_id,
            last_px=Price.from_str("50000.00000000"),
        )

        order.apply(fill1)
        position = Position(instrument=BTCUSDT_BINANCE, fill=fill1)

        self.cache_db.add_order(order)
        self.cache_db.update_order(order)
        self.cache_db.add_position(position)

        # Act
        self.portfolio.reset()
        self.exec_engine.load_cache()

        # Assert
        assert self.exec_engine.position_id_count(strategy_id) == 1

    def test_given_random_command_logs_and_continues(self):
        # Arrange
        random = TradingCommand(
            self.trader_id,
            self.strategy_id,
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

    def test_submit_order_with_duplicate_client_order_id_logs(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register(
            self.trader_id,
            self.msgbus,
            self.portfolio,
            self.data_engine,
            self.risk_engine,
            self.clock,
            self.logger,
        )

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        submit_order = SubmitOrder(
            self.trader_id,
            strategy.id,
            PositionId.null(),
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_order)
        self.exec_engine.process(TestStubs.event_order_submitted(order))
        self.risk_engine.execute(submit_order)  # Duplicate command

        # Assert
        assert order.state == OrderState.SUBMITTED

    def test_submit_order_for_random_venue_logs(self):
        # Arrange
        self.exec_engine.cache.add_instrument(BTCUSDT_BINANCE)
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register(
            self.trader_id,
            self.msgbus,
            self.portfolio,
            self.data_engine,
            self.risk_engine,
            self.clock,
            self.logger,
        )

        order = strategy.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_int(10),
        )

        submit_order = SubmitOrder(
            self.trader_id,
            strategy.id,
            PositionId.null(),
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_order)

        # Assert
        assert self.exec_engine.command_count == 1
        assert order.state == OrderState.INITIALIZED

    def test_submit_order_for_none_existent_position_id_invalidates_order(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register(
            self.trader_id,
            self.msgbus,
            self.portfolio,
            self.data_engine,
            self.risk_engine,
            self.clock,
            self.logger,
        )

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        submit_order = SubmitOrder(
            self.trader_id,
            strategy.id,
            PositionId("RANDOM"),  # Invalid PositionId
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_order)

        # Assert
        assert order.state == OrderState.DENIED

    def test_order_filled_with_unrecognized_strategy_id(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register(
            self.trader_id,
            self.msgbus,
            self.portfolio,
            self.data_engine,
            self.risk_engine,
            self.clock,
            self.logger,
        )

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        submit_order = SubmitOrder(
            self.trader_id,
            strategy.id,
            PositionId.null(),
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_order)
        self.exec_engine.process(TestStubs.event_order_submitted(order))
        self.exec_engine.process(
            TestStubs.event_order_filled(
                order,
                AUDUSD_SIM,
                strategy_id=StrategyId("RANDOM-001"),
            )
        )

        # Assert (does not send to strategy)
        assert order.state == OrderState.FILLED

    def test_submit_bracket_order_with_all_duplicate_client_order_id_logs_does_not_submit(
        self,
    ):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register(
            self.trader_id,
            self.msgbus,
            self.portfolio,
            self.data_engine,
            self.risk_engine,
            self.clock,
            self.logger,
        )

        entry = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        stop_loss = strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100000),
            Price.from_str("0.50000"),
        )

        take_profit = strategy.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100000),
            Price.from_str("1.00000"),
        )

        bracket = BracketOrder(
            entry=entry,
            stop_loss=stop_loss,
            take_profit=take_profit,
        )

        submit_bracket = SubmitBracketOrder(
            self.trader_id,
            strategy.id,
            bracket,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_bracket)
        self.exec_engine.process(TestStubs.event_order_submitted(entry))
        self.exec_engine.process(TestStubs.event_order_submitted(stop_loss))
        self.exec_engine.process(TestStubs.event_order_submitted(take_profit))
        self.risk_engine.execute(submit_bracket)  # <-- Duplicate command

        # Assert
        assert entry.state == OrderState.SUBMITTED  # Did not invalidate originals
        assert stop_loss.state == OrderState.SUBMITTED  # Did not invalidate originals
        assert take_profit.state == OrderState.SUBMITTED  # Did not invalidate originals
        assert self.exec_engine.command_count == 1

    def test_submit_bracket_order_with_duplicate_take_profit_client_order_id_logs_does_not_submit(
        self,
    ):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register(
            self.trader_id,
            self.msgbus,
            self.portfolio,
            self.data_engine,
            self.risk_engine,
            self.clock,
            self.logger,
        )

        entry1 = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        stop_loss1 = strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100000),
            Price.from_str("0.50000"),
        )

        take_profit1 = strategy.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100000),
            Price.from_str("1.00000"),
        )

        bracket1 = BracketOrder(
            entry=entry1,
            stop_loss=stop_loss1,
            take_profit=take_profit1,
        )

        submit_bracket1 = SubmitBracketOrder(
            self.trader_id,
            strategy.id,
            bracket1,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        entry2 = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        stop_loss2 = strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100000),
            Price.from_str("0.50000"),
        )

        bracket2 = BracketOrder(
            entry=entry2,
            stop_loss=stop_loss2,
            take_profit=take_profit1,  # Duplicate
        )

        submit_bracket2 = SubmitBracketOrder(
            self.trader_id,
            strategy.id,
            bracket2,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_bracket1)
        self.exec_engine.process(TestStubs.event_order_submitted(entry1))
        self.exec_engine.process(TestStubs.event_order_accepted(entry1))
        self.exec_engine.process(TestStubs.event_order_submitted(stop_loss1))
        self.exec_engine.process(TestStubs.event_order_accepted(stop_loss1))
        self.exec_engine.process(TestStubs.event_order_submitted(take_profit1))
        self.exec_engine.process(TestStubs.event_order_accepted(take_profit1))
        self.risk_engine.execute(submit_bracket2)  # SL and TP

        # Assert
        assert entry2.state == OrderState.DENIED
        assert entry1.state == OrderState.ACCEPTED
        assert stop_loss1.state == OrderState.ACCEPTED
        assert take_profit1.state == OrderState.ACCEPTED  # Did not invalidate original

    def test_submit_bracket_order_with_duplicate_stop_loss_client_order_id_logs_does_not_submit(
        self,
    ):
        # Arrange
        self.exec_engine.start()
        self.risk_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register(
            self.trader_id,
            self.msgbus,
            self.portfolio,
            self.data_engine,
            self.risk_engine,
            self.clock,
            self.logger,
        )

        entry1 = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        stop_loss1 = strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100000),
            Price.from_str("0.50000"),
        )

        take_profit1 = strategy.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100000),
            Price.from_str("1.00000"),
        )

        bracket1 = BracketOrder(
            entry=entry1,
            stop_loss=stop_loss1,
            take_profit=take_profit1,
        )

        submit_bracket1 = SubmitBracketOrder(
            self.trader_id,
            strategy.id,
            bracket1,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        entry2 = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        take_profit2 = strategy.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100000),
            Price.from_str("1.00000"),
        )

        bracket2 = BracketOrder(
            entry=entry2,
            stop_loss=stop_loss1,  # Duplicate
            take_profit=take_profit2,
        )

        submit_bracket2 = SubmitBracketOrder(
            self.trader_id,
            strategy.id,
            bracket2,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_bracket1)
        self.exec_engine.process(TestStubs.event_order_submitted(entry1))
        self.exec_engine.process(TestStubs.event_order_accepted(entry1))
        self.exec_engine.process(TestStubs.event_order_submitted(stop_loss1))
        self.exec_engine.process(TestStubs.event_order_accepted(stop_loss1))
        self.exec_engine.process(TestStubs.event_order_submitted(take_profit1))
        self.exec_engine.process(TestStubs.event_order_accepted(take_profit1))
        self.risk_engine.execute(submit_bracket2)  # SL and TP

        # Assert
        assert entry2.state == OrderState.DENIED
        assert entry1.state == OrderState.ACCEPTED  # Did not invalidate original
        assert stop_loss1.state == OrderState.ACCEPTED  # Did not invalidate original
        assert take_profit1.state == OrderState.ACCEPTED  # Did not invalidate original
        assert take_profit2.state == OrderState.DENIED

    def test_submit_order(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register(
            self.trader_id,
            self.msgbus,
            self.portfolio,
            self.data_engine,
            self.risk_engine,
            self.clock,
            self.logger,
        )

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        submit_order = SubmitOrder(
            self.trader_id,
            strategy.id,
            PositionId.null(),
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_order)

        # Assert
        assert submit_order in self.exec_client.commands
        assert self.cache.order_exists(order.client_order_id)

    def test_submit_order_with_cleared_cache_logs_error(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register(
            self.trader_id,
            self.msgbus,
            self.portfolio,
            self.data_engine,
            self.risk_engine,
            self.clock,
            self.logger,
        )

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        submit_order = SubmitOrder(
            self.trader_id,
            strategy.id,
            PositionId.null(),
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_order)
        self.exec_engine.cache.clear_cache()
        self.exec_engine.process(TestStubs.event_order_accepted(order))

        # Assert
        assert order.state == OrderState.INITIALIZED

    def test_when_applying_event_to_order_with_invalid_state_trigger_logs(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register(
            self.trader_id,
            self.msgbus,
            self.portfolio,
            self.data_engine,
            self.risk_engine,
            self.clock,
            self.logger,
        )

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        submit_order = SubmitOrder(
            self.trader_id,
            strategy.id,
            PositionId.null(),
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        # Act (event attempts to fill order before its submitted)
        self.risk_engine.execute(submit_order)
        self.exec_engine.process(TestStubs.event_order_filled(order, AUDUSD_SIM))

        # Assert
        assert order.state == OrderState.INITIALIZED

    def test_order_filled_event_when_order_not_found_in_cache_logs(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register(
            self.trader_id,
            self.msgbus,
            self.portfolio,
            self.data_engine,
            self.risk_engine,
            self.clock,
            self.logger,
        )

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        # Act (event attempts to fill order before its submitted)
        self.exec_engine.process(TestStubs.event_order_filled(order, AUDUSD_SIM))

        # Assert
        assert self.exec_engine.event_count == 2
        assert order.state == OrderState.INITIALIZED

    def test_cancel_order_for_already_completed_order_logs_and_does_nothing(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register(
            self.trader_id,
            self.msgbus,
            self.portfolio,
            self.data_engine,
            self.risk_engine,
            self.clock,
            self.logger,
        )

        # Push to OrderState.FILLED (completed)
        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        submit_order = SubmitOrder(
            self.trader_id,
            strategy.id,
            PositionId.null(),
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        self.risk_engine.execute(submit_order)
        self.exec_engine.process(TestStubs.event_order_submitted(order))
        self.exec_engine.process(TestStubs.event_order_accepted(order))
        self.exec_engine.process(TestStubs.event_order_filled(order, AUDUSD_SIM))

        cancel_order = CancelOrder(
            self.trader_id,
            order.strategy_id,
            order.instrument_id,
            order.client_order_id,
            VenueOrderId("1"),
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.exec_engine.execute(cancel_order)

        # Assert
        assert order.state == OrderState.FILLED

    def test_update_order_for_already_completed_order_logs_and_does_nothing(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register(
            self.trader_id,
            self.msgbus,
            self.portfolio,
            self.data_engine,
            self.risk_engine,
            self.clock,
            self.logger,
        )

        # Push to OrderState.FILLED (completed)
        order = strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("0.85101"),
        )

        submit_order = SubmitOrder(
            self.trader_id,
            strategy.id,
            PositionId.null(),
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        self.risk_engine.execute(submit_order)
        self.exec_engine.process(TestStubs.event_order_submitted(order))
        self.exec_engine.process(TestStubs.event_order_accepted(order))
        self.exec_engine.process(TestStubs.event_order_filled(order, AUDUSD_SIM))

        update_order = UpdateOrder(
            self.trader_id,
            order.strategy_id,
            order.instrument_id,
            order.client_order_id,
            order.venue_order_id,
            Quantity.from_int(200000),
            order.price,
            None,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.exec_engine.execute(update_order)

        # Assert
        assert order.state == OrderState.FILLED
        assert order.quantity == Quantity.from_int(100000)

    def test_handle_order_event_with_random_client_order_id_and_order_id_cached(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register(
            self.trader_id,
            self.msgbus,
            self.portfolio,
            self.data_engine,
            self.risk_engine,
            self.clock,
            self.logger,
        )

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        submit_order = SubmitOrder(
            self.trader_id,
            strategy.id,
            PositionId.null(),
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        self.risk_engine.execute(submit_order)
        self.exec_engine.process(TestStubs.event_order_submitted(order))
        self.exec_engine.process(TestStubs.event_order_accepted(order))

        canceled = OrderCanceled(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            self.account_id,
            ClientOrderId("web_001"),  # Random id from say a web UI
            order.venue_order_id,
            self.clock.timestamp_ns(),
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.exec_engine.process(canceled)

        # Assert (order was found and OrderCanceled event was applied)
        assert order.state == OrderState.CANCELED

    def test_handle_order_event_with_random_client_order_id_and_order_id_not_cached(
        self,
    ):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register(
            self.trader_id,
            self.msgbus,
            self.portfolio,
            self.data_engine,
            self.risk_engine,
            self.clock,
            self.logger,
        )

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        submit_order = SubmitOrder(
            self.trader_id,
            strategy.id,
            PositionId.null(),
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        self.risk_engine.execute(submit_order)
        self.exec_engine.process(TestStubs.event_order_submitted(order))
        self.exec_engine.process(TestStubs.event_order_accepted(order))

        canceled = OrderCanceled(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            self.account_id,
            ClientOrderId("web_001"),  # Random id from say a web UI
            VenueOrderId("RANDOM_001"),  # Also a random order id the engine won't find
            self.clock.timestamp_ns(),
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.exec_engine.process(canceled)

        # Assert (order was not found, engine did not crash)
        assert order.state == OrderState.ACCEPTED

    def test_handle_duplicate_order_events_logs_error_and_does_not_apply(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register(
            self.trader_id,
            self.msgbus,
            self.portfolio,
            self.data_engine,
            self.risk_engine,
            self.clock,
            self.logger,
        )

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        submit_order = SubmitOrder(
            self.trader_id,
            strategy.id,
            PositionId.null(),
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        self.risk_engine.execute(submit_order)
        self.exec_engine.process(TestStubs.event_order_submitted(order))
        self.exec_engine.process(TestStubs.event_order_accepted(order))

        canceled = OrderCanceled(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            self.account_id,
            ClientOrderId("web_001"),  # Random id from say a web UI
            order.venue_order_id,
            self.clock.timestamp_ns(),
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.exec_engine.process(canceled)
        self.exec_engine.process(canceled)

        # Assert (order was found and OrderCanceled event was applied)
        assert order.state == OrderState.CANCELED
        assert order.event_count == 4

    def test_handle_order_fill_event_with_no_position_id_correctly_handles_fill(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register(
            self.trader_id,
            self.msgbus,
            self.portfolio,
            self.data_engine,
            self.risk_engine,
            self.clock,
            self.logger,
        )

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        submit_order = SubmitOrder(
            self.trader_id,
            strategy.id,
            PositionId.null(),
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        self.risk_engine.execute(submit_order)

        # Act
        self.exec_engine.process(TestStubs.event_order_submitted(order))
        self.exec_engine.process(TestStubs.event_order_accepted(order))
        self.exec_engine.process(
            TestStubs.event_order_filled(
                order=order,
                instrument=AUDUSD_SIM,
            )
        )

        expected_position_id = PositionId("P-19700101-000000-000-001-1")

        # Assert
        assert self.cache.position_exists(expected_position_id)
        assert self.cache.is_position_open(expected_position_id)
        assert not self.cache.is_position_closed(expected_position_id)
        assert isinstance(self.cache.position(expected_position_id), Position)
        assert expected_position_id in self.cache.position_ids()
        assert expected_position_id not in self.cache.position_closed_ids(strategy_id=strategy.id)
        assert expected_position_id not in self.cache.position_closed_ids()
        assert expected_position_id in self.cache.position_open_ids(strategy_id=strategy.id)
        assert expected_position_id in self.cache.position_open_ids()
        assert self.cache.positions_total_count() == 1
        assert self.cache.positions_open_count() == 1
        assert self.cache.positions_closed_count() == 0

    def test_handle_order_fill_event(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register(
            self.trader_id,
            self.msgbus,
            self.portfolio,
            self.data_engine,
            self.risk_engine,
            self.clock,
            self.logger,
        )

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        submit_order = SubmitOrder(
            self.trader_id,
            strategy.id,
            PositionId.null(),
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        self.risk_engine.execute(submit_order)

        # Act
        self.exec_engine.process(TestStubs.event_order_submitted(order))
        self.exec_engine.process(TestStubs.event_order_accepted(order))
        self.exec_engine.process(TestStubs.event_order_filled(order, AUDUSD_SIM))

        expected_position_id = PositionId("P-19700101-000000-000-001-1")

        # Assert
        assert self.cache.position_exists(expected_position_id)
        assert self.cache.is_position_open(expected_position_id)
        assert not self.cache.is_position_closed(expected_position_id)
        assert isinstance(self.cache.position(expected_position_id), Position)
        assert expected_position_id in self.cache.position_ids()
        assert expected_position_id not in self.cache.position_closed_ids(strategy_id=strategy.id)
        assert expected_position_id not in self.cache.position_closed_ids()
        assert expected_position_id in self.cache.position_open_ids(strategy_id=strategy.id)
        assert expected_position_id in self.cache.position_open_ids()
        assert self.cache.positions_total_count() == 1
        assert self.cache.positions_open_count() == 1
        assert self.cache.positions_closed_count() == 0

    def test_handle_multiple_partial_fill_events(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register(
            self.trader_id,
            self.msgbus,
            self.portfolio,
            self.data_engine,
            self.risk_engine,
            self.clock,
            self.logger,
        )

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        submit_order = SubmitOrder(
            self.trader_id,
            strategy.id,
            PositionId.null(),
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        self.risk_engine.execute(submit_order)
        self.exec_engine.process(TestStubs.event_order_submitted(order))
        self.exec_engine.process(TestStubs.event_order_accepted(order))

        # Act
        expected_position_id = PositionId("P-19700101-000000-000-001-1")

        self.exec_engine.process(
            TestStubs.event_order_filled(
                order=order, instrument=AUDUSD_SIM, last_qty=Quantity.from_int(20100)
            ),
        )

        self.exec_engine.process(
            TestStubs.event_order_filled(
                order=order, instrument=AUDUSD_SIM, last_qty=Quantity.from_int(19900)
            ),
        )

        self.exec_engine.process(
            TestStubs.event_order_filled(
                order=order, instrument=AUDUSD_SIM, last_qty=Quantity.from_int(60000)
            ),
        )

        # Assert
        assert self.cache.position_exists(expected_position_id)
        assert self.cache.is_position_open(expected_position_id)
        assert not self.cache.is_position_closed(expected_position_id)
        assert isinstance(self.cache.position(expected_position_id), Position)
        assert expected_position_id in self.cache.position_ids()
        assert expected_position_id not in self.cache.position_closed_ids(strategy_id=strategy.id)
        assert expected_position_id not in self.cache.position_closed_ids()
        assert expected_position_id in self.cache.position_open_ids(strategy_id=strategy.id)
        assert expected_position_id in self.cache.position_open_ids()
        assert self.cache.positions_total_count() == 1
        assert self.cache.positions_open_count() == 1
        assert self.cache.positions_closed_count() == 0

    def test_handle_position_opening_with_position_id_none(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register(
            self.trader_id,
            self.msgbus,
            self.portfolio,
            self.data_engine,
            self.risk_engine,
            self.clock,
            self.logger,
        )

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        submit_order = SubmitOrder(
            self.trader_id,
            strategy.id,
            PositionId.null(),
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        self.risk_engine.execute(submit_order)

        # Act
        self.exec_engine.process(TestStubs.event_order_submitted(order))
        self.exec_engine.process(TestStubs.event_order_accepted(order))
        self.exec_engine.process(
            TestStubs.event_order_filled(order, AUDUSD_SIM, position_id=PositionId.null())
        )

        expected_id = PositionId("P-19700101-000000-000-001-1")  # Generated inside engine

        # Assert
        assert self.cache.position_exists(expected_id)
        assert self.cache.is_position_open(expected_id)
        assert not self.cache.is_position_closed(expected_id)
        assert isinstance(self.cache.position(expected_id), Position)
        assert expected_id in self.cache.position_ids()
        assert expected_id not in self.cache.position_closed_ids(strategy_id=strategy.id)
        assert expected_id not in self.cache.position_closed_ids()
        assert expected_id in self.cache.position_open_ids(strategy_id=strategy.id)
        assert expected_id in self.cache.position_open_ids()
        assert self.cache.positions_total_count() == 1
        assert self.cache.positions_open_count() == 1
        assert self.cache.positions_closed_count() == 0

    def test_add_to_existing_position_on_order_fill(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register(
            self.trader_id,
            self.msgbus,
            self.portfolio,
            self.data_engine,
            self.risk_engine,
            self.clock,
            self.logger,
        )

        order1 = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        order2 = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        submit_order1 = SubmitOrder(
            self.trader_id,
            strategy.id,
            PositionId.null(),
            order1,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        self.risk_engine.execute(submit_order1)
        self.exec_engine.process(TestStubs.event_order_submitted(order1))
        self.exec_engine.process(TestStubs.event_order_accepted(order1))
        self.exec_engine.process(TestStubs.event_order_filled(order1, AUDUSD_SIM))

        expected_position_id = PositionId("P-19700101-000000-000-001-1")

        submit_order2 = SubmitOrder(
            self.trader_id,
            strategy.id,
            expected_position_id,
            order2,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_order2)
        self.exec_engine.process(TestStubs.event_order_submitted(order2))
        self.exec_engine.process(TestStubs.event_order_accepted(order2))
        self.exec_engine.process(
            TestStubs.event_order_filled(order2, AUDUSD_SIM, position_id=expected_position_id)
        )

        # Assert
        assert self.cache.position_exists(expected_position_id)
        assert self.cache.is_position_open(expected_position_id)
        assert not self.cache.is_position_closed(expected_position_id)
        assert isinstance(self.cache.position(expected_position_id), Position)
        assert len(self.cache.positions_closed(strategy_id=strategy.id)) == 0
        assert len(self.cache.positions_closed()) == 0
        assert len(self.cache.positions_open(strategy_id=strategy.id)) == 1
        assert len(self.cache.positions_open()) == 1
        assert self.cache.positions_total_count() == 1
        assert self.cache.positions_open_count() == 1
        assert self.cache.positions_closed_count() == 0

    def test_close_position_on_order_fill(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register(
            self.trader_id,
            self.msgbus,
            self.portfolio,
            self.data_engine,
            self.risk_engine,
            self.clock,
            self.logger,
        )

        order1 = strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("1.00000"),
        )

        order2 = strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100000),
            Price.from_str("1.00000"),
        )

        submit_order1 = SubmitOrder(
            self.trader_id,
            strategy.id,
            PositionId.null(),
            order1,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        position_id = PositionId("P-1")

        self.risk_engine.execute(submit_order1)
        self.exec_engine.process(TestStubs.event_order_submitted(order1))
        self.exec_engine.process(TestStubs.event_order_accepted(order1))
        self.exec_engine.process(
            TestStubs.event_order_filled(order1, AUDUSD_SIM, position_id=position_id)
        )

        submit_order2 = SubmitOrder(
            self.trader_id,
            strategy.id,
            position_id,
            order2,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_order2)
        self.exec_engine.process(TestStubs.event_order_submitted(order2))
        self.exec_engine.process(TestStubs.event_order_accepted(order2))
        self.exec_engine.process(
            TestStubs.event_order_filled(order2, AUDUSD_SIM, position_id=position_id)
        )

        # # Assert
        assert self.cache.position_exists(position_id)
        assert not self.cache.is_position_open(position_id)
        assert self.cache.is_position_closed(position_id)
        assert self.cache.position(position_id).id == position_id
        assert self.cache.positions(strategy_id=strategy.id)[0].id == position_id
        assert self.cache.positions()[0].id == position_id
        assert len(self.cache.positions_open(strategy_id=strategy.id)) == 0
        assert len(self.cache.positions_open()) == 0
        assert self.cache.positions_closed(strategy_id=strategy.id)[0].id == position_id
        assert self.cache.positions_closed()[0].id == position_id
        assert position_id not in self.cache.position_open_ids(strategy_id=strategy.id)
        assert position_id not in self.cache.position_open_ids()
        assert self.cache.positions_total_count() == 1
        assert self.cache.positions_open_count() == 0
        assert self.cache.positions_closed_count() == 1

    def test_multiple_strategy_positions_opened(self):
        # Arrange
        self.exec_engine.start()

        strategy1 = TradingStrategy(order_id_tag="001")
        strategy1.register(
            self.trader_id,
            self.msgbus,
            self.portfolio,
            self.data_engine,
            self.risk_engine,
            self.clock,
            self.logger,
        )

        strategy2 = TradingStrategy(order_id_tag="002")
        strategy2.register(
            self.trader_id,
            self.msgbus,
            self.portfolio,
            self.data_engine,
            self.risk_engine,
            self.clock,
            self.logger,
        )

        order1 = strategy1.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("1.00000"),
        )

        order2 = strategy2.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("1.00000"),
        )

        submit_order1 = SubmitOrder(
            self.trader_id,
            strategy1.id,
            PositionId.null(),
            order1,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        submit_order2 = SubmitOrder(
            self.trader_id,
            strategy2.id,
            PositionId.null(),
            order2,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        position1_id = PositionId("P-1")
        position2_id = PositionId("P-2")

        # Act
        self.risk_engine.execute(submit_order1)
        self.risk_engine.execute(submit_order2)
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
        assert self.cache.position_exists(position1_id)
        assert self.cache.position_exists(position2_id)
        assert self.cache.is_position_open(position1_id)
        assert self.cache.is_position_open(position2_id)
        assert not self.cache.is_position_closed(position1_id)
        assert not self.cache.is_position_closed(position2_id)
        assert isinstance(self.cache.position(position1_id), Position)
        assert isinstance(self.cache.position(position2_id), Position)
        assert position1_id in self.cache.position_ids(strategy_id=strategy1.id)
        assert position2_id in self.cache.position_ids(strategy_id=strategy2.id)
        assert position1_id in self.cache.position_ids()
        assert position2_id in self.cache.position_ids()
        assert len(self.cache.position_open_ids()) == 2
        assert len(self.cache.positions_open(strategy_id=strategy1.id)) == 1
        assert len(self.cache.positions_open(strategy_id=strategy2.id)) == 1
        assert len(self.cache.positions_open(strategy_id=strategy2.id)) == 1
        assert len(self.cache.positions_open()) == 2
        assert len(self.cache.positions_open(strategy_id=strategy1.id)) == 1
        assert len(self.cache.positions_open(strategy_id=strategy2.id)) == 1
        assert position1_id in self.cache.position_open_ids(strategy_id=strategy1.id)
        assert position2_id in self.cache.position_open_ids(strategy_id=strategy2.id)
        assert position1_id in self.cache.position_open_ids()
        assert position2_id in self.cache.position_open_ids()
        assert position1_id not in self.cache.position_closed_ids(strategy_id=strategy1.id)
        assert position2_id not in self.cache.position_closed_ids(strategy_id=strategy2.id)
        assert position1_id not in self.cache.position_closed_ids()
        assert position2_id not in self.cache.position_closed_ids()
        assert self.cache.positions_total_count() == 2
        assert self.cache.positions_open_count() == 2
        assert self.cache.positions_closed_count() == 0

    def test_multiple_strategy_positions_one_active_one_closed(self):
        # Arrange
        self.exec_engine.start()

        strategy1 = TradingStrategy(order_id_tag="001")
        strategy1.register(
            self.trader_id,
            self.msgbus,
            self.portfolio,
            self.data_engine,
            self.risk_engine,
            self.clock,
            self.logger,
        )

        strategy2 = TradingStrategy(order_id_tag="002")
        strategy2.register(
            self.trader_id,
            self.msgbus,
            self.portfolio,
            self.data_engine,
            self.risk_engine,
            self.clock,
            self.logger,
        )

        order1 = strategy1.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("1.00000"),
        )

        order2 = strategy1.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100000),
            Price.from_str("1.00000"),
        )

        order3 = strategy2.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("1.00000"),
        )

        submit_order1 = SubmitOrder(
            self.trader_id,
            strategy1.id,
            PositionId.null(),
            order1,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        position_id1 = PositionId("P-1")

        submit_order2 = SubmitOrder(
            self.trader_id,
            strategy1.id,
            position_id1,
            order2,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        submit_order3 = SubmitOrder(
            self.trader_id,
            strategy2.id,
            PositionId.null(),
            order3,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        position_id2 = PositionId("P-2")

        # Act
        self.risk_engine.execute(submit_order1)
        self.exec_engine.process(TestStubs.event_order_submitted(order1))
        self.exec_engine.process(TestStubs.event_order_accepted(order1))
        self.exec_engine.process(
            TestStubs.event_order_filled(order1, AUDUSD_SIM, position_id=position_id1)
        )

        self.risk_engine.execute(submit_order2)
        self.exec_engine.process(TestStubs.event_order_submitted(order2))
        self.exec_engine.process(TestStubs.event_order_accepted(order2))
        self.exec_engine.process(
            TestStubs.event_order_filled(order2, AUDUSD_SIM, position_id=position_id1)
        )

        self.risk_engine.execute(submit_order3)
        self.exec_engine.process(TestStubs.event_order_submitted(order3))
        self.exec_engine.process(TestStubs.event_order_accepted(order3))
        self.exec_engine.process(
            TestStubs.event_order_filled(order3, AUDUSD_SIM, position_id=position_id2)
        )

        # Assert
        # Already tested .is_position_active and .is_position_closed above
        assert self.cache.position_exists(position_id1)
        assert self.cache.position_exists(position_id2)
        assert position_id1 in self.cache.position_ids(strategy_id=strategy1.id)
        assert position_id2 in self.cache.position_ids(strategy_id=strategy2.id)
        assert position_id1 in self.cache.position_ids()
        assert position_id2 in self.cache.position_ids()
        assert len(self.cache.positions_open(strategy_id=strategy1.id)) == 0
        assert len(self.cache.positions_open(strategy_id=strategy2.id)) == 1
        assert len(self.cache.positions_open()) == 1
        assert len(self.cache.positions_closed()) == 1
        assert len(self.cache.positions()) == 2
        assert position_id1 not in self.cache.position_open_ids(strategy_id=strategy1.id)
        assert position_id2 in self.cache.position_open_ids(strategy_id=strategy2.id)
        assert position_id1 not in self.cache.position_open_ids()
        assert position_id2 in self.cache.position_open_ids()
        assert position_id1 in self.cache.position_closed_ids(strategy_id=strategy1.id)
        assert position_id2 not in self.cache.position_closed_ids(strategy_id=strategy2.id)
        assert position_id1 in self.cache.position_closed_ids()
        assert position_id2 not in self.cache.position_closed_ids()
        assert self.cache.positions_total_count() == 2
        assert self.cache.positions_open_count() == 1
        assert self.cache.positions_closed_count() == 1

    def test_flip_position_on_opposite_filled_same_position_sell(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register(
            self.trader_id,
            self.msgbus,
            self.portfolio,
            self.data_engine,
            self.risk_engine,
            self.clock,
            self.logger,
        )

        order1 = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        order2 = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(150000),
        )

        submit_order1 = SubmitOrder(
            self.trader_id,
            strategy.id,
            PositionId.null(),
            order1,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        position_id = PositionId("P-19700101-000000-000-001-1")

        self.risk_engine.execute(submit_order1)
        self.exec_engine.process(TestStubs.event_order_submitted(order1))
        self.exec_engine.process(TestStubs.event_order_accepted(order1))
        self.exec_engine.process(
            TestStubs.event_order_filled(order1, AUDUSD_SIM, position_id=position_id)
        )

        submit_order2 = SubmitOrder(
            self.trader_id,
            strategy.id,
            position_id,
            order2,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_order2)
        self.exec_engine.process(TestStubs.event_order_submitted(order2))
        self.exec_engine.process(TestStubs.event_order_accepted(order2))
        self.exec_engine.process(
            TestStubs.event_order_filled(order2, AUDUSD_SIM, position_id=position_id)
        )

        # Assert
        position_id_flipped = PositionId("P-19700101-000000-000-001-1F")
        position_flipped = self.cache.position(position_id_flipped)

        assert position_flipped.net_qty == -50000
        assert position_flipped.last_event.last_qty == 50000
        assert self.cache.position_exists(position_id)
        assert self.cache.position_exists(position_id_flipped)
        assert self.cache.is_position_closed(position_id)
        assert self.cache.is_position_open(position_id_flipped)
        assert position_id in self.cache.position_ids()
        assert position_id in self.cache.position_ids(strategy_id=strategy.id)
        assert position_id_flipped in self.cache.position_ids()
        assert position_id_flipped in self.cache.position_ids(strategy_id=strategy.id)
        assert self.cache.positions_total_count() == 2
        assert self.cache.positions_open_count() == 1
        assert self.cache.positions_closed_count() == 1

    def test_flip_position_on_opposite_filled_same_position_buy(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register(
            self.trader_id,
            self.msgbus,
            self.portfolio,
            self.data_engine,
            self.risk_engine,
            self.clock,
            self.logger,
        )

        order1 = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100000),
        )

        order2 = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(150000),
        )

        submit_order1 = SubmitOrder(
            self.trader_id,
            strategy.id,
            PositionId.null(),
            order1,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        position_id = PositionId("P-19700101-000000-000-001-1")

        self.risk_engine.execute(submit_order1)
        self.exec_engine.process(TestStubs.event_order_submitted(order1))
        self.exec_engine.process(TestStubs.event_order_accepted(order1))
        self.exec_engine.process(
            TestStubs.event_order_filled(order1, AUDUSD_SIM, position_id=position_id)
        )

        submit_order2 = SubmitOrder(
            self.trader_id,
            strategy.id,
            position_id,
            order2,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_order2)
        self.exec_engine.process(TestStubs.event_order_submitted(order2))
        self.exec_engine.process(TestStubs.event_order_accepted(order2))
        self.exec_engine.process(
            TestStubs.event_order_filled(order2, AUDUSD_SIM, position_id=position_id)
        )

        # Assert
        position_id_flipped = PositionId("P-19700101-000000-000-001-1F")
        position_flipped = self.cache.position(position_id_flipped)

        assert position_flipped.net_qty == 50000
        assert position_flipped.last_event.last_qty == 50000
        assert self.cache.position_exists(position_id)
        assert self.cache.position_exists(position_id_flipped)
        assert self.cache.is_position_closed(position_id)
        assert self.cache.is_position_open(position_id_flipped)
        assert position_id in self.cache.position_ids()
        assert position_id in self.cache.position_ids(strategy_id=strategy.id)
        assert position_id_flipped in self.cache.position_ids()
        assert position_id_flipped in self.cache.position_ids(strategy_id=strategy.id)
        assert self.cache.positions_total_count() == 2
        assert self.cache.positions_open_count() == 1
        assert self.cache.positions_closed_count() == 1

    def test_handle_updated_order_event(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register(
            self.trader_id,
            self.msgbus,
            self.portfolio,
            self.data_engine,
            self.risk_engine,
            self.clock,
            self.logger,
        )

        order = strategy.order_factory.limit(
            instrument_id=AUDUSD_SIM.id,
            order_side=OrderSide.BUY,
            price=Price.from_str("10.0"),
            quantity=Quantity.from_int(100000),
        )

        submit_order = SubmitOrder(
            self.trader_id,
            strategy.id,
            PositionId.null(),
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        self.risk_engine.execute(submit_order)
        self.exec_engine.process(TestStubs.event_order_submitted(order))
        self.exec_engine.process(TestStubs.event_order_accepted(order))
        self.exec_engine.process(TestStubs.event_order_pending_update(order))

        # Get order, check venue_order_id
        cached_order = self.cache.order(order.client_order_id)
        assert cached_order.venue_order_id == order.venue_order_id

        # Act
        new_venue_id = VenueOrderId("UPDATED")
        order_updated = OrderUpdated(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            instrument_id=AUDUSD_SIM.id,
            account_id=self.account_id,
            client_order_id=order.client_order_id,
            venue_order_id=new_venue_id,
            quantity=order.quantity,
            price=order.price,
            trigger=None,
            ts_updated_ns=self.clock.timestamp_ns(),
            event_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )
        self.exec_engine.process(order_updated)

        # Order should have new venue_order_id
        cached_order = self.cache.order(order.client_order_id)
        assert cached_order.venue_order_id == new_venue_id
