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

from datetime import timedelta
from decimal import Decimal

from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.logging import LogLevel
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
from nautilus_trader.model.enums import TradingState
from nautilus_trader.model.enums import VenueType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders.bracket import BracketOrder
from nautilus_trader.risk.engine import RiskEngine
from nautilus_trader.trading.portfolio import Portfolio
from nautilus_trader.trading.strategy import TradingStrategy
from tests.test_kit.mocks import MockExecutionClient
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
GBPUSD_SIM = TestInstrumentProvider.default_fx_ccy("GBP/USD")


class TestRiskEngine:
    def setup(self):
        # Fixture Setup
        self.clock = TestClock()
        self.uuid_factory = UUIDFactory()
        self.logger = Logger(self.clock, level_stdout=LogLevel.DEBUG)

        self.trader_id = TraderId("TESTER-000")
        self.account_id = TestStubs.account_id()
        self.venue = Venue("SIM")

        self.cache = TestStubs.cache()

        self.portfolio = Portfolio(
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.exec_engine = ExecutionEngine(
            portfolio=self.portfolio,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.risk_engine = RiskEngine(
            exec_engine=self.exec_engine,
            portfolio=self.portfolio,
            cache=self.cache,
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

    def test_trading_state_after_instantiation_returns_active(self):
        # Arrange, Act
        result = self.risk_engine.trading_state

        # Assert
        assert result == TradingState.ACTIVE

    def test_set_trading_state_changes_value(self):
        # Arrange, Act
        self.risk_engine.set_trading_state(TradingState.HALTED)

        # Assert
        assert self.risk_engine.trading_state == TradingState.HALTED

    def test_max_order_rate_when_no_risk_config_returns_100_per_second(self):
        # Arrange, Act
        result = self.risk_engine.max_order_rate()

        assert result == (100, timedelta(seconds=1))

    def test_max_notionals_per_order_when_no_risk_config_returns_empty_dict(self):
        # Arrange, Act
        result = self.risk_engine.max_notionals_per_order()

        assert result == {}

    def test_max_notional_per_order_when_no_risk_config_returns_none(self):
        # Arrange, Act
        result = self.risk_engine.max_notional_per_order(AUDUSD_SIM.id)

        assert result is None

    def test_set_max_notional_per_order_changes_setting(self):
        # Arrange, Act
        self.risk_engine.set_max_notional_per_order(AUDUSD_SIM.id, 1_000_000)

        max_notionals = self.risk_engine.max_notionals_per_order()
        max_notional = self.risk_engine.max_notional_per_order(AUDUSD_SIM.id)

        # Assert
        assert max_notionals == {AUDUSD_SIM.id: Decimal("1000000")}
        assert max_notional == Decimal(1_000_000)

    def test_given_random_command_then_logs_and_continues(self):
        # Arrange
        random = TradingCommand(
            self.trader_id,
            StrategyId("SCALPER-001"),
            AUDUSD_SIM.id,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        self.risk_engine.execute(random)

    def test_given_random_event_then_logs_and_continues(self):
        # Arrange
        random = Event(
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        self.exec_engine.process(random)

    # -- SUBMIT ORDER TESTS ----------------------------------------------------------------------------

    def test_submit_order_with_default_settings_then_sends_to_client(self):
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
        assert self.exec_engine.command_count == 1
        assert self.exec_client.calls == ["connect", "submit_order"]

    def test_submit_order_when_duplicate_id_then_denies(self):
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

        self.risk_engine.execute(submit_order)

        # Act
        self.risk_engine.execute(submit_order)

        # Assert
        assert self.exec_engine.command_count == 1
        assert self.exec_client.calls == ["connect", "submit_order"]

    def test_submit_order_when_position_id_not_in_cache_then_denies(self):
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
            PositionId("009"),  # <-- not in cache
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_order)

        # Assert
        assert self.exec_engine.command_count == 0  # <-- command never reaches engine

    def test_submit_order_when_instrument_not_in_cache_then_denies(self):
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
            GBPUSD_SIM.id,  # <-- not in cache
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
        assert self.exec_engine.command_count == 0  # <-- command never reaches engine

    def test_submit_order_when_invalid_price_precision_then_denies(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        order = strategy.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("0.9999999999999999"),  # <- invalid price
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
        assert self.exec_engine.command_count == 0  # <-- command never reaches engine

    def test_submit_order_when_invalid_negative_price_and_not_option_then_denies(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        order = strategy.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("-1.0"),  # <- invalid price
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
        assert self.exec_engine.command_count == 0  # <-- command never reaches engine

    def test_submit_order_when_invalid_trigger_price_then_denies(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        order = strategy.order_factory.stop_limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("1.00000"),
            Price.from_str("0.999999999999999"),  # <- invalid trigger
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
        assert self.exec_engine.command_count == 0  # <-- command never reaches engine

    def test_submit_order_when_invalid_quantity_precision_then_denies(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        order = strategy.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_str("1.111111111111111111"),  # <- invalid quantity
            Price.from_str("1.00000"),
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
        assert self.exec_engine.command_count == 0  # <-- command never reaches engine

    def test_submit_order_when_invalid_quantity_exceeds_maximum_then_denies(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        order = strategy.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(1_000_000_000),  # <- invalid quantity fat finger!
            Price.from_str("1.00000"),
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
        assert self.exec_engine.command_count == 0  # <-- command never reaches engine

    def test_submit_order_when_invalid_quantity_less_than_minimum_then_denies(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        order = strategy.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(1),  # <- invalid quantity
            Price.from_str("1.00000"),
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
        assert self.exec_engine.command_count == 0  # <-- command never reaches engine

    def test_submit_order_when_market_order_and_no_market_to_check_then_denies(self):
        # Arrange
        self.risk_engine.set_max_notional_per_order(AUDUSD_SIM.id, 1_000_000)

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
            Quantity.from_int(10000000),
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
        assert self.exec_engine.command_count == 0  # <-- command never reaches engine

    def test_submit_order_when_market_order_and_over_max_notional_then_denies(self):
        # Arrange
        self.risk_engine.set_max_notional_per_order(AUDUSD_SIM.id, 1_000_000)

        # Initialize market
        quote = TestStubs.quote_tick_5decimal(AUDUSD_SIM.id)
        self.exec_engine.cache.add_quote_tick(quote)

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
            Quantity.from_int(10000000),
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
        assert self.exec_engine.command_count == 0  # <-- command never reaches engine

    def test_submit_order_when_reducing_and_buy_order_adds_then_denies(self):
        # Arrange
        self.risk_engine.set_max_notional_per_order(AUDUSD_SIM.id, 1_000_000)

        # Initialize market
        quote = TestStubs.quote_tick_5decimal(AUDUSD_SIM.id)
        self.exec_engine.cache.add_quote_tick(quote)

        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        order1 = strategy.order_factory.market(
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
        self.risk_engine.set_trading_state(TradingState.REDUCING)  # <-- allow reducing orders only

        order2 = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        submit_order2 = SubmitOrder(
            self.trader_id,
            strategy.id,
            PositionId.null(),
            order2,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        self.exec_engine.process(TestStubs.event_order_submitted(order1))
        self.exec_engine.process(TestStubs.event_order_accepted(order1))
        self.exec_engine.process(TestStubs.event_order_filled(order1, AUDUSD_SIM))

        # Act
        self.risk_engine.execute(submit_order2)

        # Assert
        assert self.portfolio.is_net_long(AUDUSD_SIM.id)
        assert self.exec_engine.command_count == 1  # <-- command never reaches engine

    def test_submit_order_when_reducing_and_sell_order_adds_then_denies(self):
        # Arrange
        self.risk_engine.set_max_notional_per_order(AUDUSD_SIM.id, 1_000_000)

        # Initialize market
        quote = TestStubs.quote_tick_5decimal(AUDUSD_SIM.id)
        self.exec_engine.cache.add_quote_tick(quote)

        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        order1 = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
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
        self.risk_engine.set_trading_state(TradingState.REDUCING)  # <-- allow reducing orders only

        order2 = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100000),
        )

        submit_order2 = SubmitOrder(
            self.trader_id,
            strategy.id,
            PositionId.null(),
            order2,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        self.exec_engine.process(TestStubs.event_order_submitted(order1))
        self.exec_engine.process(TestStubs.event_order_accepted(order1))
        self.exec_engine.process(TestStubs.event_order_filled(order1, AUDUSD_SIM))

        # Act
        self.risk_engine.execute(submit_order2)

        # Assert
        assert self.portfolio.is_net_short(AUDUSD_SIM.id)
        assert self.exec_engine.command_count == 1  # <-- command never reaches engine

    def test_submit_order_when_trading_halted_then_denies_order(self):
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

        # Halt trading
        self.risk_engine.set_trading_state(TradingState.HALTED)

        # Act
        self.risk_engine.execute(submit_order)

        # Assert
        assert self.risk_engine.command_count == 1  # <-- command never reaches engine

    # -- SUBMIT BRACKET ORDER TESTS --------------------------------------------------------------------

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
        assert self.exec_engine.command_count == 1
        assert self.exec_client.calls == ["connect", "submit_bracket_order"]

    def test_submit_bracket_order_with_duplicate_entry_id_then_denies(self):
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

        self.risk_engine.execute(submit_bracket)

        # Act
        self.risk_engine.execute(submit_bracket)

        # Assert
        assert self.exec_engine.command_count == 1  # <-- command never reaches engine

    def test_submit_bracket_order_with_duplicate_stop_loss_id_then_denies(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        entry1 = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        stop_loss = strategy.order_factory.stop_market(  # <-- duplicate
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("1.00000"),
        )

        take_profit1 = strategy.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("1.10000"),
        )

        entry2 = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        take_profit2 = strategy.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("1.10000"),
        )

        bracket1 = BracketOrder(
            entry=entry1,
            stop_loss=stop_loss,
            take_profit=take_profit1,
        )

        bracket2 = BracketOrder(
            entry=entry2,
            stop_loss=stop_loss,
            take_profit=take_profit2,
        )

        submit_bracket1 = SubmitBracketOrder(
            self.trader_id,
            strategy.id,
            bracket1,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        submit_bracket2 = SubmitBracketOrder(
            self.trader_id,
            strategy.id,
            bracket2,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        self.risk_engine.execute(submit_bracket1)

        # Act
        self.risk_engine.execute(submit_bracket2)

        # Assert
        assert self.exec_engine.command_count == 1  # <-- command never reaches engine

    def test_submit_bracket_order_with_duplicate_take_profit_id_then_denies(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        entry1 = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        stop_loss1 = strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("1.00000"),
        )

        take_profit = strategy.order_factory.limit(  # <-- duplicate
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("1.10000"),
        )

        entry2 = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        stop_loss2 = strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("1.00000"),
        )

        bracket1 = BracketOrder(
            entry=entry1,
            stop_loss=stop_loss1,
            take_profit=take_profit,
        )

        bracket2 = BracketOrder(
            entry=entry2,
            stop_loss=stop_loss2,
            take_profit=take_profit,
        )

        submit_bracket1 = SubmitBracketOrder(
            self.trader_id,
            strategy.id,
            bracket1,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        submit_bracket2 = SubmitBracketOrder(
            self.trader_id,
            strategy.id,
            bracket2,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        self.risk_engine.execute(submit_bracket1)

        # Act
        self.risk_engine.execute(submit_bracket2)

        # Assert
        assert self.exec_engine.command_count == 1  # <-- command never reaches engine

    def test_submit_bracket_order_when_instrument_not_in_cache_then_denies(self):
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
            GBPUSD_SIM.id,
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
        assert self.exec_engine.command_count == 0  # <-- command never reaches engine

    # -- UPDATE ORDER TESTS ----------------------------------------------------------------------------

    def test_update_order_with_default_settings_then_sends_to_client(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        order = strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("1.00010"),
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
            None,
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

    # -- CANCEL ORDER TESTS ----------------------------------------------------------------------------

    def test_cancel_order_with_default_settings_then_sends_to_client(self):
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
