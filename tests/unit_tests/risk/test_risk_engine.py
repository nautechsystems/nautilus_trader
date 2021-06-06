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

from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.core.message import Event
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.model.commands import CancelOrder
from nautilus_trader.model.commands import SubmitBracketOrder
from nautilus_trader.model.commands import SubmitOrder
from nautilus_trader.model.commands import TradingCommand
from nautilus_trader.model.commands import UpdateOrder
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import VenueType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.risk.engine import RiskEngine
from nautilus_trader.trading.portfolio import Portfolio
from nautilus_trader.trading.strategy import TradingStrategy
from tests.test_kit.mocks import MockExecutionClient
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


class TestRiskEngine:
    def setup(self):
        # Fixture Setup
        self.clock = TestClock()
        self.uuid_factory = UUIDFactory()
        self.logger = Logger(self.clock)

        self.trader_id = TraderId("TESTER-000")
        self.account_id = TestStubs.account_id()
        self.venue = Venue("SIM")

        self.portfolio = Portfolio(
            cache=TestStubs.cache(),
            clock=self.clock,
            logger=self.logger,
        )

        self.exec_engine = ExecutionEngine(
            portfolio=self.portfolio,
            cache=TestStubs.cache(),
            clock=self.clock,
            logger=self.logger,
        )

        self.risk_engine = RiskEngine(
            exec_engine=self.exec_engine,
            portfolio=self.portfolio,
            cache=TestStubs.cache(),
            clock=self.clock,
            logger=self.logger,
            config={},
        )

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

        # Wire up components
        self.exec_engine.register_risk_engine(self.risk_engine)
        self.exec_engine.register_client(self.exec_client)

        # Prepare data
        self.exec_engine.cache.add_instrument(AUDUSD_SIM)

    def test_set_block_all_orders_changes_flag_value(self):
        # Arrange
        # Act
        self.risk_engine.set_block_all_orders()

        # Assert
        assert self.risk_engine.block_all_orders

    def test_given_random_command_logs_and_continues(self):
        # Arrange
        random = TradingCommand(
            self.trader_id,
            StrategyId("SCALPER-001"),
            AUDUSD_SIM.id,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        self.risk_engine.execute(random)

    def test_given_random_event_logs_and_continues(self):
        # Arrange
        random = Event(
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        self.exec_engine.process(random)

    def test_submit_order_with_default_settings_sends_to_client(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

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
        assert self.exec_client.calls == ["connect", "submit_order"]

    def test_submit_bracket_with_default_settings_sends_to_client(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        entry = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        bracket = strategy.order_factory.bracket(
            entry_order=entry,
            stop_loss=Price.from_str("1.00000"),
            take_profit=Price.from_str("1.00010"),
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

        # Assert
        assert self.exec_client.calls == ["connect", "submit_bracket_order"]

    def test_submit_order_when_block_all_orders_true_then_denies_order(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

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

        self.risk_engine.set_block_all_orders()

        # Act
        self.risk_engine.execute(submit_order)

        # Assert
        assert self.exec_client.calls == ["connect"]
        assert self.risk_engine.command_count == 1

    def test_update_order_with_default_settings_sends_to_client(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        submit = SubmitOrder(
            self.trader_id,
            strategy.id,
            PositionId.null(),
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        update = UpdateOrder(
            self.trader_id,
            strategy.id,
            order.instrument_id,
            order.client_order_id,
            order.venue_order_id,
            order.quantity,
            Price.from_str("1.00010"),
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        self.risk_engine.execute(submit)

        # Act
        self.risk_engine.execute(update)

        # Assert
        assert self.exec_client.calls == ["connect", "submit_order", "update_order"]
        assert self.risk_engine.command_count == 2
        assert self.exec_engine.command_count == 2

    def test_cancel_order_with_default_settings_sends_to_client(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        submit = SubmitOrder(
            self.trader_id,
            strategy.id,
            PositionId.null(),
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        cancel = CancelOrder(
            self.trader_id,
            strategy.id,
            order.instrument_id,
            order.client_order_id,
            order.venue_order_id,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        self.risk_engine.execute(submit)

        # Act
        self.risk_engine.execute(cancel)

        # Assert
        assert self.exec_client.calls == ["connect", "submit_order", "cancel_order"]
        assert self.risk_engine.command_count == 2
        assert self.exec_engine.command_count == 2

    def test_submit_bracket_when_block_all_orders_true_then_denies_order(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        entry = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        bracket = strategy.order_factory.bracket(
            entry_order=entry,
            stop_loss=Price.from_str("1.00000"),
            take_profit=Price.from_str("1.00010"),
        )

        submit_bracket = SubmitBracketOrder(
            self.trader_id,
            strategy.id,
            bracket,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        self.risk_engine.set_block_all_orders()

        # Act
        self.risk_engine.execute(submit_bracket)

        # Assert
        assert self.exec_client.calls == ["connect"]
        assert self.risk_engine.command_count == 1
        assert self.exec_engine.event_count == 3
