# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
from nautilus_trader.common.enums import LogLevel
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.messages import TradingStateChanged
from nautilus_trader.config import ExecEngineConfig
from nautilus_trader.config import RiskEngineConfig
from nautilus_trader.core.message import Event
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.emulator import OrderEmulator
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.messages import SubmitOrderList
from nautilus_trader.execution.messages import TradingCommand
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import TradingState
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import OrderListId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders.list import OrderList
from nautilus_trader.msgbus.bus import MessageBus
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.risk.engine import RiskEngine
from nautilus_trader.test_kit.mocks.exec_clients import MockExecutionClient
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from nautilus_trader.trading.strategy import Strategy


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
GBPUSD_SIM = TestInstrumentProvider.default_fx_ccy("GBP/USD")
BTCUSDT_BINANCE = TestInstrumentProvider.btcusdt_binance()


class TestRiskEngineWithCashAccount:
    def setup(self):
        # Fixture Setup
        self.clock = TestClock()
        self.logger = Logger(
            clock=self.clock,
            level_stdout=LogLevel.DEBUG,
            bypass=True,
        )

        self.trader_id = TestIdStubs.trader_id()
        self.account_id = TestIdStubs.account_id()
        self.venue = Venue("SIM")

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
            logger=self.logger,
        )

        self.cache = TestComponentStubs.cache()

        self.portfolio = Portfolio(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.exec_engine = ExecutionEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
            config=ExecEngineConfig(debug=True),
        )

        self.risk_engine = RiskEngine(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
            config=RiskEngineConfig(debug=True),
        )

        self.emulator = OrderEmulator(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.exec_client = MockExecutionClient(
            client_id=ClientId(self.venue.value),
            venue=self.venue,
            account_type=AccountType.CASH,
            base_currency=USD,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )
        self.portfolio.update_account(TestEventStubs.cash_account_state())
        self.exec_engine.register_client(self.exec_client)

        # Prepare data
        self.cache.add_instrument(AUDUSD_SIM)

    def test_config_risk_engine(self):
        # Arrange
        self.msgbus.deregister("RiskEngine.execute", self.risk_engine.execute)
        self.msgbus.deregister("RiskEngine.process", self.risk_engine.process)

        config = RiskEngineConfig(
            bypass=True,  # <-- bypassing pre-trade risk checks for backtest
            max_order_submit_rate="5/00:00:01",
            max_order_modify_rate="5/00:00:01",
            max_notional_per_order={"GBP/USD.SIM": 2_000_000},
        )

        # Act
        risk_engine = RiskEngine(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
            config=config,
        )

        # Assert
        assert risk_engine.is_bypassed
        assert risk_engine.max_order_submit_rate() == (5, timedelta(seconds=1))
        assert risk_engine.max_order_modify_rate() == (5, timedelta(seconds=1))
        assert risk_engine.max_notionals_per_order() == {GBPUSD_SIM.id: Decimal("2000000")}
        assert risk_engine.max_notional_per_order(GBPUSD_SIM.id) == 2_000_000

    def test_risk_engine_on_stop(self):
        # Arrange, Act
        self.risk_engine.start()
        self.risk_engine.stop()

        # Assert
        assert self.risk_engine.is_stopped

    def test_process_event_then_handles(self):
        # Arrange
        event = Event(
            event_id=UUID4(),
            ts_event=self.clock.timestamp_ns(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.process(event)

        # Assert
        assert self.risk_engine.event_count == 1

    def test_trading_state_after_instantiation_returns_active(self):
        # Arrange, Act
        result = self.risk_engine.trading_state

        # Assert
        assert result == TradingState.ACTIVE

    def test_set_trading_state_when_no_change_logs_warning(self):
        # Arrange, Act
        self.risk_engine.set_trading_state(TradingState.ACTIVE)

        # Assert
        assert self.risk_engine.trading_state == TradingState.ACTIVE

    def test_set_trading_state_changes_value_and_publishes_event(self):
        # Arrange
        handler = []
        self.msgbus.subscribe(topic="events.risk*", handler=handler.append)

        # Act
        self.risk_engine.set_trading_state(TradingState.HALTED)

        # Assert
        assert type(handler[0]) == TradingStateChanged
        assert self.risk_engine.trading_state == TradingState.HALTED

    def test_max_order_submit_rate_when_no_risk_config_returns_100_per_second(self):
        # Arrange, Act
        result = self.risk_engine.max_order_submit_rate()

        assert result == (100, timedelta(seconds=1))

    def test_max_order_modify_rate_when_no_risk_config_returns_100_per_second(self):
        # Arrange, Act
        result = self.risk_engine.max_order_modify_rate()

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
            client_id=None,
            trader_id=self.trader_id,
            strategy_id=StrategyId("SCALPER-001"),
            instrument_id=AUDUSD_SIM.id,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.risk_engine.execute(random)

    def test_given_random_event_then_logs_and_continues(self):
        # Arrange
        random = Event(
            event_id=UUID4(),
            ts_event=self.clock.timestamp_ns(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.risk_engine.process(random)

    # -- SUBMIT ORDER TESTS -----------------------------------------------------------------------

    def test_submit_order_with_default_settings_then_sends_to_client(self):
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_order)

        # Assert
        assert self.exec_engine.command_count == 1
        assert self.exec_client.calls == ["_start", "submit_order"]

    def test_submit_order_when_risk_bypassed_sends_to_execution_engine(self):
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_order)

        # Assert
        assert self.exec_engine.command_count == 1  # <-- initial account event
        assert self.exec_client.calls == ["_start", "submit_order"]

    def test_submit_reduce_only_order_when_position_already_closed_then_denies(self):
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        order1 = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        order2 = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
            reduce_only=True,
        )

        order3 = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
            reduce_only=True,
        )

        submit_order1 = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order1,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.risk_engine.execute(submit_order1)
        self.exec_engine.process(TestEventStubs.order_submitted(order1))
        self.exec_engine.process(TestEventStubs.order_accepted(order1))
        self.exec_engine.process(TestEventStubs.order_filled(order1, AUDUSD_SIM))

        submit_order2 = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=PositionId("P-19700101-0000-000-None-1"),
            order=order2,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.risk_engine.execute(submit_order2)
        self.exec_engine.process(TestEventStubs.order_submitted(order2))
        self.exec_engine.process(TestEventStubs.order_accepted(order2))
        self.exec_engine.process(TestEventStubs.order_filled(order2, AUDUSD_SIM))

        submit_order3 = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=PositionId("P-19700101-0000-000-None-1"),
            order=order3,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_order3)

        # Assert
        assert order1.status == OrderStatus.FILLED
        assert order2.status == OrderStatus.FILLED
        assert order3.status == OrderStatus.DENIED
        assert self.exec_engine.command_count == 2
        assert self.exec_client.calls == ["_start", "submit_order", "submit_order"]

    def test_submit_reduce_only_order_when_position_would_be_increased_then_denies(self):
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        order1 = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        order2 = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(200_000),
            reduce_only=True,
        )

        submit_order1 = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order1,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.risk_engine.execute(submit_order1)
        self.exec_engine.process(TestEventStubs.order_submitted(order1))
        self.exec_engine.process(TestEventStubs.order_accepted(order1))
        self.exec_engine.process(TestEventStubs.order_filled(order1, AUDUSD_SIM))

        submit_order2 = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=PositionId("P-19700101-0000-000-None-1"),
            order=order2,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_order2)
        self.exec_engine.process(TestEventStubs.order_submitted(order2))
        self.exec_engine.process(TestEventStubs.order_accepted(order2))
        self.exec_engine.process(TestEventStubs.order_filled(order2, AUDUSD_SIM))

        # Assert
        assert order1.status == OrderStatus.FILLED
        assert order2.status == OrderStatus.DENIED
        assert self.exec_engine.command_count == 1
        assert self.exec_client.calls == ["_start", "submit_order"]

    def test_submit_order_reduce_only_order_with_custom_position_id_not_open_then_denies(self):
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            reduce_only=True,
        )

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=PositionId("CUSTOM-001"),  # <-- custom position ID
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_order)

        # Assert
        assert order.status == OrderStatus.DENIED
        assert self.exec_engine.command_count == 0  # <-- command never reaches engine

    def test_submit_order_when_instrument_not_in_cache_then_denies(self):
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        order = strategy.order_factory.market(
            GBPUSD_SIM.id,  # <-- not in the cache
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_order)

        # Assert
        assert order.status == OrderStatus.DENIED
        assert self.exec_engine.command_count == 0  # <-- command never reaches engine

    def test_submit_order_when_invalid_price_precision_then_denies(self):
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        order = strategy.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("0.999999999"),  # <- invalid price
        )

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_order)

        # Assert
        assert order.status == OrderStatus.DENIED
        assert self.exec_engine.command_count == 0  # <-- command never reaches engine

    def test_submit_order_when_invalid_negative_price_and_not_option_then_denies(self):
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        order = strategy.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("-1.0"),  # <- invalid price
        )

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_order)

        # Assert
        assert order.status == OrderStatus.DENIED
        assert self.exec_engine.command_count == 0  # <-- command never reaches engine

    def test_submit_order_when_invalid_trigger_price_then_denies(self):
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        order = strategy.order_factory.stop_limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
            Price.from_str("0.999999999"),  # <- invalid trigger
        )

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_order)

        # Assert
        assert order.status == OrderStatus.DENIED
        assert self.exec_engine.command_count == 0  # <-- command never reaches engine

    def test_submit_order_when_invalid_quantity_precision_then_denies(self):
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        order = strategy.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_str("1.111111111"),  # <- invalid quantity
            Price.from_str("1.00000"),
        )

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_order)

        # Assert
        assert order.status == OrderStatus.DENIED
        assert self.exec_engine.command_count == 0  # <-- command never reaches engine

    def test_submit_order_when_invalid_quantity_exceeds_maximum_then_denies(self):
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        order = strategy.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(1_000_000_000),  # <- invalid quantity fat finger!
            Price.from_str("1.00000"),
        )

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_order)

        # Assert
        assert order.status == OrderStatus.DENIED
        assert self.exec_engine.command_count == 0  # <-- command never reaches engine

    def test_submit_order_when_invalid_quantity_less_than_minimum_then_denies(self):
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        order = strategy.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(1),  # <- invalid quantity
            Price.from_str("1.00000"),
        )

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_order)

        # Assert
        assert order.status == OrderStatus.DENIED
        assert self.exec_engine.command_count == 0  # <-- command never reaches engine

    def test_submit_order_when_market_order_and_no_market_then_logs_warning(self):
        # Arrange
        self.risk_engine.set_max_notional_per_order(AUDUSD_SIM.id, 1_000_000)

        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(10_000_000),
        )

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_order)

        # Assert
        assert self.exec_engine.command_count == 1  # <-- command reaches engine with warning

    def test_submit_order_when_buy_market_order_and_over_max_notional_then_denies(self):
        # Arrange
        self.risk_engine.set_max_notional_per_order(AUDUSD_SIM.id, 1_000_000)

        # Initialize market
        quote = TestDataStubs.quote_tick(AUDUSD_SIM)
        self.cache.add_quote_tick(quote)

        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(10_000_000),
        )

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_order)

        # Assert
        assert order.status == OrderStatus.DENIED
        assert self.exec_engine.command_count == 0  # <-- command never reaches engine

    def test_submit_order_when_sell_market_order_and_over_max_notional_then_denies(self):
        # Arrange
        self.risk_engine.set_max_notional_per_order(AUDUSD_SIM.id, 1_000_000)

        # Initialize market
        quote = QuoteTick(
            instrument_id=AUDUSD_SIM.id,
            bid=Price.from_str("0.75000"),
            ask=Price.from_str("0.75005"),
            bid_size=Quantity.from_int(5_000_000),
            ask_size=Quantity.from_int(5_000_000),
            ts_event=0,
            ts_init=0,
        )
        self.cache.add_quote_tick(quote)

        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(10_000_000),
        )

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_order)

        # Assert
        assert order.status == OrderStatus.DENIED
        assert self.exec_engine.command_count == 0  # <-- command never reaches engine

    def test_submit_order_when_market_order_and_over_free_balance_then_denies(self):
        # Arrange - Initialize market
        quote = TestDataStubs.quote_tick(AUDUSD_SIM)
        self.cache.add_quote_tick(quote)

        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(10_000_000),
        )

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_order)

        # Assert
        assert order.status == OrderStatus.DENIED
        assert self.exec_engine.command_count == 0  # <-- command never reaches engine

    def test_submit_order_list_buys_when_over_free_balance_then_denies(self):
        # Arrange - Initialize market
        quote = TestDataStubs.quote_tick(AUDUSD_SIM)
        self.cache.add_quote_tick(quote)

        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        order1 = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(500_000),
        )

        order2 = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(500_000),
        )

        order_list = OrderList(
            order_list_id=OrderListId("1"),
            orders=[order1, order2],
        )

        submit_order = SubmitOrderList(
            self.trader_id,
            strategy.id,
            order_list,
            UUID4(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_order)

        # Assert
        assert order1.status == OrderStatus.DENIED
        assert order2.status == OrderStatus.DENIED
        assert self.exec_engine.command_count == 0  # <-- command never reaches engine

    def test_submit_order_list_sells_when_over_free_balance_then_denies(self):
        # Arrange - Initialize market
        quote = TestDataStubs.quote_tick(AUDUSD_SIM)
        self.cache.add_quote_tick(quote)

        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        order1 = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(500_000),
        )

        order2 = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(500_000),
        )

        order_list = OrderList(
            order_list_id=OrderListId("1"),
            orders=[order1, order2],
        )

        submit_order = SubmitOrderList(
            self.trader_id,
            strategy.id,
            order_list,
            UUID4(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_order)

        # Assert
        assert order1.status == OrderStatus.DENIED
        assert order2.status == OrderStatus.DENIED
        assert self.exec_engine.command_count == 0  # <-- command never reaches engine

    def test_submit_order_when_reducing_and_buy_order_adds_then_denies(self):
        # Arrange
        self.risk_engine.set_max_notional_per_order(AUDUSD_SIM.id, 1_000_000)

        # Initialize market
        quote = TestDataStubs.quote_tick(AUDUSD_SIM)
        self.cache.add_quote_tick(quote)

        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        order1 = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        submit_order1 = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order1,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.risk_engine.execute(submit_order1)
        self.risk_engine.set_trading_state(TradingState.REDUCING)  # <-- allow reducing orders only

        order2 = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        submit_order2 = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order2,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.exec_engine.process(TestEventStubs.order_submitted(order1))
        self.exec_engine.process(TestEventStubs.order_accepted(order1))
        self.exec_engine.process(TestEventStubs.order_filled(order1, AUDUSD_SIM))

        # Act
        self.risk_engine.execute(submit_order2)

        # Assert
        assert order1.status == OrderStatus.FILLED
        assert order2.status == OrderStatus.DENIED
        assert self.portfolio.is_net_long(AUDUSD_SIM.id)
        assert self.exec_engine.command_count == 1  # <-- command never reaches engine

    def test_submit_order_when_reducing_and_sell_order_adds_then_denies(self):
        # Arrange
        self.risk_engine.set_max_notional_per_order(AUDUSD_SIM.id, 1_000_000)

        # Initialize market
        quote = TestDataStubs.quote_tick(AUDUSD_SIM)
        self.cache.add_quote_tick(quote)

        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        order1 = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
        )

        submit_order1 = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order1,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.risk_engine.execute(submit_order1)
        self.risk_engine.set_trading_state(TradingState.REDUCING)  # <-- allow reducing orders only

        order2 = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
        )

        submit_order2 = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order2,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.exec_engine.process(TestEventStubs.order_submitted(order1))
        self.exec_engine.process(TestEventStubs.order_accepted(order1))
        self.exec_engine.process(TestEventStubs.order_filled(order1, AUDUSD_SIM))

        # Act
        self.risk_engine.execute(submit_order2)

        # Assert
        assert order1.status == OrderStatus.FILLED
        assert order2.status == OrderStatus.DENIED
        assert self.portfolio.is_net_short(AUDUSD_SIM.id)
        assert self.exec_engine.command_count == 1  # <-- command never reaches engine

    def test_submit_order_when_trading_halted_then_denies_order(self):
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Halt trading
        self.risk_engine.set_trading_state(TradingState.HALTED)  # <-- halt trading

        # Act
        self.risk_engine.execute(submit_order)

        # Assert
        assert order.status == OrderStatus.DENIED
        assert self.risk_engine.command_count == 1  # <-- command never reaches engine

    def test_submit_order_list_when_trading_halted_then_denies_orders(self):
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        entry = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        stop_loss = strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
        )

        take_profit = strategy.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.10000"),
        )

        bracket = OrderList(
            order_list_id=OrderListId("1"),
            orders=[entry, stop_loss, take_profit],
        )

        submit_bracket = SubmitOrderList(
            self.trader_id,
            strategy.id,
            bracket,
            UUID4(),
            self.clock.timestamp_ns(),
        )

        # Halt trading
        self.risk_engine.set_trading_state(TradingState.HALTED)  # <-- halt trading

        # Act
        self.risk_engine.execute(submit_bracket)

        # Assert
        assert entry.status == OrderStatus.DENIED
        assert stop_loss.status == OrderStatus.DENIED
        assert take_profit.status == OrderStatus.DENIED
        assert self.risk_engine.command_count == 1  # <-- command never reaches engine

    def test_submit_order_list_buys_when_trading_reducing_then_denies_orders(self):
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Push portfolio LONG
        long = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=long,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.exec_engine.execute(submit_order)

        self.exec_engine.process(TestEventStubs.order_submitted(long))
        self.exec_engine.process(TestEventStubs.order_accepted(long))
        self.exec_engine.process(TestEventStubs.order_filled(long, AUDUSD_SIM))

        entry = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        stop_loss = strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
        )

        take_profit = strategy.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.10000"),
        )

        bracket = OrderList(
            order_list_id=OrderListId("1"),
            orders=[entry, stop_loss, take_profit],
        )

        submit_bracket = SubmitOrderList(
            self.trader_id,
            strategy.id,
            bracket,
            UUID4(),
            self.clock.timestamp_ns(),
        )

        # Reduce trading
        self.risk_engine.set_trading_state(TradingState.REDUCING)  # <-- allow reducing orders only

        # Act
        self.risk_engine.execute(submit_bracket)

        # Assert
        assert entry.status == OrderStatus.DENIED
        assert stop_loss.status == OrderStatus.DENIED
        assert take_profit.status == OrderStatus.DENIED
        assert self.risk_engine.command_count == 1  # <-- command never reaches engine

    def test_submit_order_list_sells_when_trading_reducing_then_denies_orders(self):
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Push portfolio SHORT
        short = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
        )

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=short,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.exec_engine.execute(submit_order)

        self.exec_engine.process(TestEventStubs.order_submitted(short))
        self.exec_engine.process(TestEventStubs.order_accepted(short))
        self.exec_engine.process(TestEventStubs.order_filled(short, AUDUSD_SIM))

        entry = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
        )

        stop_loss = strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
        )

        take_profit = strategy.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
            Price.from_str("1.10000"),
        )

        bracket = OrderList(
            order_list_id=OrderListId("1"),
            orders=[entry, stop_loss, take_profit],
        )

        submit_bracket = SubmitOrderList(
            self.trader_id,
            strategy.id,
            bracket,
            UUID4(),
            self.clock.timestamp_ns(),
        )

        # Reduce trading
        self.risk_engine.set_trading_state(TradingState.REDUCING)  # <-- allow reducing orders only

        # Act
        self.risk_engine.execute(submit_bracket)

        # Assert
        assert entry.status == OrderStatus.DENIED
        assert stop_loss.status == OrderStatus.DENIED
        assert take_profit.status == OrderStatus.DENIED
        assert self.risk_engine.command_count == 1  # <-- command never reaches engine

    # -- SUBMIT BRACKET ORDER TESTS ---------------------------------------------------------------

    def test_submit_bracket_with_default_settings_sends_to_client(self):
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        bracket = strategy.order_factory.bracket(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            sl_trigger_price=Price.from_str("1.00000"),
            tp_price=Price.from_str("1.00010"),
        )

        submit_bracket = SubmitOrderList(
            self.trader_id,
            strategy.id,
            bracket,
            UUID4(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_bracket)

        # Assert
        assert self.exec_engine.command_count == 1
        assert self.exec_client.calls == ["_start", "submit_order_list"]

    def test_submit_bracket_with_emulated_orders_sends_to_emulator(self):
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        bracket = strategy.order_factory.bracket(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            sl_trigger_price=Price.from_str("1.00000"),
            tp_price=Price.from_str("1.00010"),
            emulation_trigger=TriggerType.BID_ASK,
        )

        submit_bracket = SubmitOrderList(
            self.trader_id,
            strategy.id,
            bracket,
            UUID4(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.emulator.execute(submit_bracket)

        # Assert
        assert submit_bracket.has_emulated_order
        assert self.exec_engine.command_count == 1  # Sends entry order
        assert self.exec_client.calls == ["_start", "submit_order"]
        assert len(self.emulator.get_submit_order_commands()) == 1

    def test_submit_bracket_order_when_instrument_not_in_cache_then_denies(self):
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        bracket = strategy.order_factory.bracket(
            GBPUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            sl_trigger_price=Price.from_str("1.00000"),
            tp_price=Price.from_str("1.00010"),
        )

        submit_bracket = SubmitOrderList(
            self.trader_id,
            strategy.id,
            bracket,
            UUID4(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_bracket)

        # Assert
        assert bracket.orders[0].status == OrderStatus.DENIED
        assert bracket.orders[1].status == OrderStatus.DENIED
        assert bracket.orders[2].status == OrderStatus.DENIED
        assert self.exec_engine.command_count == 0  # <-- command never reaches engine

    def test_submit_order_for_emulation_sends_command_to_emulator(self):
        # Arrange
        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        order = strategy.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(1_000),
            Price.from_str("1.00000"),
            emulation_trigger=TriggerType.LAST_TRADE,
        )

        # Act
        strategy.submit_order(order)

        # Assert
        assert self.emulator.get_submit_order_commands().get(order.client_order_id)

    # -- MODIFY ORDER TESTS -----------------------------------------------------------------------

    def test_modify_order_when_no_order_found_denies(self):
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        modify = ModifyOrder(
            self.trader_id,
            strategy.id,
            AUDUSD_SIM.id,
            ClientOrderId("invalid"),
            VenueOrderId("1"),
            Quantity.from_int(100_000),
            Price.from_str("1.00010"),
            None,
            UUID4(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(modify)

        # Assert
        assert self.exec_client.calls == ["_start"]
        assert self.risk_engine.command_count == 1
        assert self.exec_engine.command_count == 0

    def test_modify_order_when_already_closed_then_denies(self):
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        order = strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00010"),
        )

        submit = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.risk_engine.execute(submit)

        self.exec_engine.process(TestEventStubs.order_submitted(order))
        self.exec_engine.process(TestEventStubs.order_accepted(order))
        self.exec_engine.process(TestEventStubs.order_filled(order, AUDUSD_SIM))

        modify = ModifyOrder(
            self.trader_id,
            strategy.id,
            order.instrument_id,
            order.client_order_id,
            VenueOrderId("1"),
            order.quantity,
            Price.from_str("1.00010"),
            None,
            UUID4(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(modify)

        # Assert
        assert self.exec_client.calls == ["_start", "submit_order"]
        assert self.risk_engine.command_count == 2
        assert self.exec_engine.command_count == 1

    def test_modify_order_when_already_pending_cancel_then_denies(self):
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        order = strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00010"),
        )

        submit = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.risk_engine.execute(submit)

        self.exec_engine.process(TestEventStubs.order_submitted(order))
        self.exec_engine.process(TestEventStubs.order_accepted(order))
        self.exec_engine.process(TestEventStubs.order_pending_cancel(order))

        modify = ModifyOrder(
            self.trader_id,
            strategy.id,
            order.instrument_id,
            order.client_order_id,
            VenueOrderId("1"),
            order.quantity,
            Price.from_str("1.00010"),
            None,
            UUID4(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(modify)

        # Assert
        assert self.exec_client.calls == ["_start", "submit_order"]
        assert self.risk_engine.command_count == 2
        assert self.exec_engine.command_count == 1

    def test_modify_order_when_already_pending_modify_then_denies(self):
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        order = strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00010"),
        )

        submit = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.risk_engine.execute(submit)

        self.exec_engine.process(TestEventStubs.order_submitted(order))
        self.exec_engine.process(TestEventStubs.order_accepted(order))
        self.exec_engine.process(TestEventStubs.order_pending_cancel(order))

        modify = ModifyOrder(
            self.trader_id,
            strategy.id,
            order.instrument_id,
            order.client_order_id,
            VenueOrderId("1"),
            order.quantity,
            Price.from_str("1.00010"),
            None,
            UUID4(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(modify)

        # Assert
        assert self.exec_client.calls == ["_start", "submit_order"]
        assert self.risk_engine.command_count == 2
        assert self.exec_engine.command_count == 1

    def test_modify_order_with_default_settings_then_sends_to_client(self):
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        order = strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00010"),
        )

        submit = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        modify = ModifyOrder(
            self.trader_id,
            strategy.id,
            order.instrument_id,
            order.client_order_id,
            VenueOrderId("1"),
            order.quantity,
            Price.from_str("1.00010"),
            None,
            UUID4(),
            self.clock.timestamp_ns(),
        )

        self.risk_engine.execute(submit)

        # Act
        self.risk_engine.execute(modify)

        # Assert
        assert self.exec_client.calls == ["_start", "submit_order", "modify_order"]
        assert self.risk_engine.command_count == 2
        assert self.exec_engine.command_count == 2

    def test_modify_order_for_emulated_order_then_sends_to_emulator(self):
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        order = strategy.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00020"),
            emulation_trigger=TriggerType.BID_ASK,
        )

        strategy.submit_order(order)
        self.exec_engine.process(TestEventStubs.order_submitted(order))
        self.exec_engine.process(TestEventStubs.order_accepted(order))

        new_trigger_price = Price.from_str("1.00010")

        # Act
        strategy.modify_order(
            order=order,
            quantity=order.quantity,
            trigger_price=new_trigger_price,
        )

        # Assert
        assert order.trigger_price == new_trigger_price

    # -- CANCEL ORDER TESTS -----------------------------------------------------------------------

    def test_cancel_order_when_order_does_not_exist_then_denies(self):
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        cancel = CancelOrder(
            self.trader_id,
            strategy.id,
            AUDUSD_SIM.id,
            ClientOrderId("1"),
            VenueOrderId("1"),
            UUID4(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(cancel)

        # Assert
        assert self.exec_client.calls == ["_start"]
        assert self.risk_engine.command_count == 1
        assert self.exec_engine.command_count == 0

    def test_cancel_order_when_already_closed_then_denies(self):
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        submit = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.risk_engine.execute(submit)
        self.exec_engine.process(TestEventStubs.order_submitted(order))
        self.exec_engine.process(TestEventStubs.order_rejected(order))

        cancel = CancelOrder(
            self.trader_id,
            strategy.id,
            order.instrument_id,
            order.client_order_id,
            VenueOrderId("1"),
            UUID4(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(cancel)

        # Assert
        assert self.exec_client.calls == ["_start", "submit_order"]
        assert self.risk_engine.command_count == 2
        assert self.exec_engine.command_count == 1

    def test_cancel_order_with_default_settings_then_sends_to_client(self):
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        submit = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        cancel = CancelOrder(
            self.trader_id,
            strategy.id,
            order.instrument_id,
            order.client_order_id,
            VenueOrderId("1"),
            UUID4(),
            self.clock.timestamp_ns(),
        )

        self.risk_engine.execute(submit)

        # Act
        self.risk_engine.execute(cancel)

        # Assert
        assert self.exec_client.calls == ["_start", "submit_order", "cancel_order"]
        assert self.risk_engine.command_count == 2
        assert self.exec_engine.command_count == 2

    def test_cancel_order_for_emulated_order_then_sends_to_emulator_and_cancels_order(self):
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        order = strategy.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(1_000),
            Price.from_str("1.00000"),
            emulation_trigger=TriggerType.LAST_TRADE,
        )

        # Act
        strategy.submit_order(order)
        strategy.cancel_order(order)

        # Assert
        assert order.is_canceled

    def test_cancel_all_orders_then_sends_to_execution_engine_and_cancels_orders(self):
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        order = strategy.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(1_000),
            Price.from_str("1.00000"),
        )

        strategy.submit_order(order)
        self.exec_engine.process(TestEventStubs.order_submitted(order))
        self.exec_engine.process(TestEventStubs.order_accepted(order))

        # Act
        strategy.cancel_all_orders(order.instrument_id)

        # Assert
        assert self.exec_client.calls == ["_start", "submit_order", "cancel_all_orders"]

    def test_cancel_all_orders_for_emulated_order_then_sends_to_emulator_and_cancels_order(self):
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        order = strategy.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(1_000),
            Price.from_str("1.00000"),
            emulation_trigger=TriggerType.LAST_TRADE,
        )

        # Act
        strategy.submit_order(order)
        strategy.cancel_all_orders(order.instrument_id)

        # Assert
        assert order.is_canceled

    def test_cancel_all_orders_with_open_orders_then_sends_to_execution_engine_and_cancels_orders(
        self,
    ):
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        order1 = strategy.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(1_000),
            Price.from_str("1.00000"),
        )

        order2 = strategy.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(1_000),
            Price.from_str("1.00000"),
        )

        strategy.submit_order(order1)
        strategy.submit_order(order2)
        self.exec_engine.process(TestEventStubs.order_submitted(order1))
        self.exec_engine.process(TestEventStubs.order_submitted(order2))
        self.exec_engine.process(TestEventStubs.order_accepted(order1))
        self.exec_engine.process(TestEventStubs.order_accepted(order2))

        # Act
        strategy.cancel_all_orders(order1.instrument_id)

        # Assert
        assert self.exec_client.calls == [
            "_start",
            "submit_order",
            "submit_order",
            "cancel_all_orders",
        ]

    def test_cancel_all_orders_for_both_open_and_emulated_orders_then_sends_to_emulator_and_cancels_order(
        self,
    ):
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        order1 = strategy.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(1_000),
            Price.from_str("1.00000"),
        )

        order2 = strategy.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(1_000),
            Price.from_str("1.00000"),
        )

        order3 = strategy.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(1_000),
            Price.from_str("1.00000"),
            emulation_trigger=TriggerType.BID_ASK,
        )

        order4 = strategy.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(1_000),
            Price.from_str("1.00000"),
            emulation_trigger=TriggerType.BID_ASK,
        )

        strategy.submit_order(order1)
        strategy.submit_order(order2)
        strategy.submit_order(order3)
        strategy.submit_order(order4)
        self.exec_engine.process(TestEventStubs.order_submitted(order1))
        self.exec_engine.process(TestEventStubs.order_submitted(order2))
        self.exec_engine.process(TestEventStubs.order_accepted(order1))
        self.exec_engine.process(TestEventStubs.order_accepted(order2))

        # Act
        strategy.cancel_all_orders(order1.instrument_id)

        # Assert
        assert order3.is_canceled
        assert order4.is_canceled
        assert self.exec_client.calls == [
            "_start",
            "submit_order",
            "submit_order",
            "cancel_all_orders",
        ]
