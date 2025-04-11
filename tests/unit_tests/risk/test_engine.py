# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

import pandas as pd
import pytest

from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.component import TestClock
from nautilus_trader.common.messages import TradingStateChanged
from nautilus_trader.config import ExecEngineConfig
from nautilus_trader.config import RiskEngineConfig
from nautilus_trader.core.message import Event
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.emulator import OrderEmulator
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.messages import SubmitOrderList
from nautilus_trader.execution.messages import TradingCommand
from nautilus_trader.model.currencies import ADA
from nautilus_trader.model.currencies import ETH
from nautilus_trader.model.currencies import GBP
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TradingState
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.events import OrderDenied
from nautilus_trader.model.events import OrderModifyRejected
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import OrderListId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders.list import OrderList
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.risk.engine import RiskEngine
from nautilus_trader.test_kit.mocks.exec_clients import MockExecutionClient
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from nautilus_trader.trading.strategy import Strategy


_AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
_GBPUSD_SIM = TestInstrumentProvider.default_fx_ccy("GBP/USD")
_XBTUSD_BITMEX = TestInstrumentProvider.xbtusd_bitmex()
_ADAUSDT_BINANCE = TestInstrumentProvider.adausdt_binance()
_ETHUSDT_BINANCE = TestInstrumentProvider.ethusdt_binance()


class TestRiskEngineWithCashAccount:
    def setup(self):
        # Fixture Setup
        self.clock = TestClock()
        self.trader_id = TestIdStubs.trader_id()
        self.account_id = TestIdStubs.account_id()
        self.venue = Venue("SIM")

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
        )

        self.cache = TestComponentStubs.cache()

        self.portfolio = Portfolio(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.exec_engine = ExecutionEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            config=ExecEngineConfig(debug=True),
        )

        self.risk_engine = RiskEngine(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            config=RiskEngineConfig(debug=True),
        )

        self.emulator = OrderEmulator(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.exec_client = MockExecutionClient(
            client_id=ClientId(self.venue.value),
            venue=self.venue,
            account_type=AccountType.CASH,
            base_currency=USD,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        self.portfolio.update_account(TestEventStubs.cash_account_state())
        self.exec_engine.register_client(self.exec_client)

        # Prepare data
        self.cache.add_instrument(_AUDUSD_SIM)

    def test_config_risk_engine(self):
        # Arrange
        self.msgbus.deregister("RiskEngine.execute", self.risk_engine.execute)
        self.msgbus.deregister("RiskEngine.process", self.risk_engine.process)

        config = RiskEngineConfig(
            bypass=True,  # <-- Bypassing pre-trade risk checks for backtest
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
            config=config,
        )

        # Assert
        assert risk_engine.is_bypassed
        assert risk_engine.max_order_submit_rate() == (5, timedelta(seconds=1))
        assert risk_engine.max_order_modify_rate() == (5, timedelta(seconds=1))
        assert risk_engine.max_notionals_per_order() == {_GBPUSD_SIM.id: Decimal("2000000")}
        assert risk_engine.max_notional_per_order(_GBPUSD_SIM.id) == 2_000_000

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
        assert type(handler[0]) is TradingStateChanged
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
        result = self.risk_engine.max_notional_per_order(_AUDUSD_SIM.id)

        assert result is None

    def test_set_max_notional_per_order_changes_setting(self):
        # Arrange, Act
        self.risk_engine.set_max_notional_per_order(_AUDUSD_SIM.id, 1_000_000)

        max_notionals = self.risk_engine.max_notionals_per_order()
        max_notional = self.risk_engine.max_notional_per_order(_AUDUSD_SIM.id)

        # Assert
        assert max_notionals == {_AUDUSD_SIM.id: Decimal("1000000")}
        assert max_notional == Decimal(1_000_000)

    def test_given_random_command_then_logs_and_continues(self):
        # Arrange
        random = TradingCommand(
            client_id=None,
            trader_id=self.trader_id,
            strategy_id=StrategyId("SCALPER-001"),
            instrument_id=_AUDUSD_SIM.id,
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
        )

        order = strategy.order_factory.market(
            _AUDUSD_SIM.id,
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
        )

        order = strategy.order_factory.market(
            _AUDUSD_SIM.id,
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
        assert self.exec_engine.command_count == 1  # <-- Initial account event
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
        )

        order1 = strategy.order_factory.market(
            _AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        order2 = strategy.order_factory.market(
            _AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
            reduce_only=True,
        )

        order3 = strategy.order_factory.market(
            _AUDUSD_SIM.id,
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
        self.exec_engine.process(TestEventStubs.order_filled(order1, _AUDUSD_SIM))

        submit_order2 = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=PositionId("P-19700101-000000-000-None-1"),
            order=order2,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.risk_engine.execute(submit_order2)
        self.exec_engine.process(TestEventStubs.order_submitted(order2))
        self.exec_engine.process(
            TestEventStubs.order_accepted(order2, venue_order_id=VenueOrderId("2")),
        )
        self.exec_engine.process(TestEventStubs.order_filled(order2, _AUDUSD_SIM))

        submit_order3 = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=PositionId("P-19700101-000000-000-None-1"),
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
        )

        order1 = strategy.order_factory.market(
            _AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        order2 = strategy.order_factory.market(
            _AUDUSD_SIM.id,
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
        self.exec_engine.process(TestEventStubs.order_filled(order1, _AUDUSD_SIM))

        submit_order2 = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=PositionId("P-19700101-000000-000-None-1"),
            order=order2,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_order2)
        self.exec_engine.process(TestEventStubs.order_submitted(order2))
        self.exec_engine.process(
            TestEventStubs.order_accepted(order2, venue_order_id=VenueOrderId("2")),
        )
        self.exec_engine.process(TestEventStubs.order_filled(order2, _AUDUSD_SIM))

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
        )

        order = strategy.order_factory.market(
            _AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            reduce_only=True,
        )

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=PositionId("CUSTOM-001"),  # <-- Custom position ID
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_order)

        # Assert
        assert order.status == OrderStatus.DENIED
        assert self.exec_engine.command_count == 0  # <-- Command never reaches engine

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
        )

        order = strategy.order_factory.market(
            _GBPUSD_SIM.id,  # <-- Not in the cache
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
        assert self.exec_engine.command_count == 0  # <-- Command never reaches engine

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
        )

        order = strategy.order_factory.limit(
            _AUDUSD_SIM.id,
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
        assert self.exec_engine.command_count == 0  # <-- Command never reaches engine

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
        )

        order = strategy.order_factory.limit(
            _AUDUSD_SIM.id,
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
        assert self.exec_engine.command_count == 0  # <-- Command never reaches engine

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
        )

        order = strategy.order_factory.stop_limit(
            _AUDUSD_SIM.id,
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
        assert self.exec_engine.command_count == 0  # <-- Command never reaches engine

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
        )

        order = strategy.order_factory.limit(
            _AUDUSD_SIM.id,
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
        assert self.exec_engine.command_count == 0  # <-- Command never reaches engine

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
        )

        order = strategy.order_factory.limit(
            _AUDUSD_SIM.id,
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
        assert self.exec_engine.command_count == 0  # <-- Command never reaches engine

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
        )

        order = strategy.order_factory.limit(
            _AUDUSD_SIM.id,
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
        assert self.exec_engine.command_count == 0  # <-- Command never reaches engine

    def test_submit_order_when_market_order_and_no_market_then_logs_warning(self):
        # Arrange
        self.risk_engine.set_max_notional_per_order(_AUDUSD_SIM.id, 1_000_000)

        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order = strategy.order_factory.market(
            _AUDUSD_SIM.id,
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
        assert self.exec_engine.command_count == 1  # <-- Command reaches engine with warning

    @pytest.mark.parametrize(("order_side"), [OrderSide.BUY, OrderSide.SELL])
    def test_submit_order_when_less_than_min_notional_for_instrument_then_denies(
        self,
        order_side: OrderSide,
    ) -> None:
        # Arrange
        exec_client = MockExecutionClient(
            client_id=ClientId("BITMEX"),
            venue=_XBTUSD_BITMEX.id.venue,
            account_type=AccountType.CASH,
            base_currency=USD,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        self.portfolio.update_account(TestEventStubs.cash_account_state(AccountId("BITMEX-001")))
        self.exec_engine.register_client(exec_client)

        self.cache.add_instrument(_XBTUSD_BITMEX)
        quote = TestDataStubs.quote_tick(
            instrument=_XBTUSD_BITMEX,
            bid_price=50_000.00,
            ask_price=50_001.00,
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
        )

        order = strategy.order_factory.market(
            _XBTUSD_BITMEX.id,
            order_side,
            Quantity.from_str("0.1"),  # <-- Less than min notional ($1 USD)
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
        assert self.exec_engine.command_count == 0  # <-- Command never reaches engine

    @pytest.mark.parametrize(("order_side"), [OrderSide.BUY, OrderSide.SELL])
    def test_submit_order_when_greater_than_max_notional_for_instrument_then_denies(
        self,
        order_side: OrderSide,
    ) -> None:
        # Arrange
        exec_client = MockExecutionClient(
            client_id=ClientId("BITMEX"),
            venue=_XBTUSD_BITMEX.id.venue,
            account_type=AccountType.CASH,
            base_currency=USD,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        self.portfolio.update_account(TestEventStubs.cash_account_state(AccountId("BITMEX-001")))
        self.exec_engine.register_client(exec_client)

        self.cache.add_instrument(_XBTUSD_BITMEX)
        quote = TestDataStubs.quote_tick(
            instrument=_XBTUSD_BITMEX,
            bid_price=50_000.00,
            ask_price=50_001.00,
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
        )

        order = strategy.order_factory.market(
            _XBTUSD_BITMEX.id,
            order_side,
            Quantity.from_int(11_000_000),  # <-- Greater than max notional ($10 million USD)
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
        assert self.exec_engine.command_count == 0  # <-- Command never reaches engine

    def test_submit_order_when_buy_market_order_and_over_max_notional_then_denies(self):
        # Arrange
        self.risk_engine.set_max_notional_per_order(_AUDUSD_SIM.id, 1_000_000)

        # Initialize market
        quote = TestDataStubs.quote_tick(_AUDUSD_SIM)
        self.cache.add_quote_tick(quote)

        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order = strategy.order_factory.market(
            _AUDUSD_SIM.id,
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
        assert self.exec_engine.command_count == 0  # <-- Command never reaches engine

    def test_submit_order_when_sell_market_order_and_over_max_notional_then_denies(self):
        # Arrange
        self.risk_engine.set_max_notional_per_order(_AUDUSD_SIM.id, 1_000_000)

        # Initialize market
        quote = QuoteTick(
            instrument_id=_AUDUSD_SIM.id,
            bid_price=Price.from_str("0.75000"),
            ask_price=Price.from_str("0.75005"),
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
        )

        order = strategy.order_factory.market(
            _AUDUSD_SIM.id,
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
        assert self.exec_engine.command_count == 0  # <-- Command never reaches engine

    def test_submit_order_when_market_order_and_over_free_balance_then_denies(self):
        # Arrange - Initialize market
        quote = TestDataStubs.quote_tick(_AUDUSD_SIM)
        self.cache.add_quote_tick(quote)

        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order = strategy.order_factory.market(
            _AUDUSD_SIM.id,
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
        assert self.exec_engine.command_count == 0  # <-- Command never reaches engine

    def test_submit_order_list_buys_when_over_free_balance_then_denies(self):
        # Arrange - Initialize market
        quote = TestDataStubs.quote_tick(_AUDUSD_SIM)
        self.cache.add_quote_tick(quote)

        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order1 = strategy.order_factory.market(
            _AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(500_000),
        )

        order2 = strategy.order_factory.market(
            _AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(600_000),  # <--- 100_000 over free balance
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
        assert self.exec_engine.command_count == 0  # <-- Command never reaches engine

    def test_submit_order_list_sells_when_over_free_balance_then_denies(self):
        # Arrange - Initialize market
        quote = TestDataStubs.quote_tick(_AUDUSD_SIM)
        self.cache.add_quote_tick(quote)

        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order1 = strategy.order_factory.market(
            _AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(500_000),
        )

        order2 = strategy.order_factory.market(
            _AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(600_000),
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
        assert self.exec_engine.command_count == 0  # <-- Command never reaches engine

    def test_submit_order_list_sells_when_multi_currency_cash_account_over_cumulative_notional(
        self,
    ):
        # Arrange - change account
        exec_client = MockExecutionClient(
            client_id=ClientId(self.venue.value),
            venue=self.venue,
            account_type=AccountType.CASH,
            base_currency=None,  # <-- Multi-currency
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.exec_engine.deregister_client(self.exec_client)
        self.exec_engine.register_client(exec_client)
        self.cache.reset()  # Clear accounts
        self.cache.add_instrument(_AUDUSD_SIM)  # Re-add instrument
        self.portfolio.update_account(TestEventStubs.cash_account_state(base_currency=None))

        # Prepare market
        quote = TestDataStubs.quote_tick(_AUDUSD_SIM)
        self.cache.add_quote_tick(quote)

        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order1 = strategy.order_factory.market(
            _AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(5_000),
        )

        order2 = strategy.order_factory.market(
            _AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(6_000),
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
        assert self.exec_engine.command_count == 0  # <-- Command never reaches engine

    def test_submit_order_when_reducing_and_buy_order_adds_then_denies(self):
        # Arrange
        self.risk_engine.set_max_notional_per_order(_AUDUSD_SIM.id, 1_000_000)

        # Initialize market
        quote = TestDataStubs.quote_tick(_AUDUSD_SIM)
        self.cache.add_quote_tick(quote)

        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order1 = strategy.order_factory.market(
            _AUDUSD_SIM.id,
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
        self.risk_engine.set_trading_state(TradingState.REDUCING)  # <-- Allow reducing orders only

        order2 = strategy.order_factory.market(
            _AUDUSD_SIM.id,
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
        self.exec_engine.process(TestEventStubs.order_filled(order1, _AUDUSD_SIM))

        # Act
        self.risk_engine.execute(submit_order2)

        # Assert
        assert order1.status == OrderStatus.FILLED
        assert order2.status == OrderStatus.DENIED
        assert self.portfolio.is_net_long(_AUDUSD_SIM.id)
        assert self.exec_engine.command_count == 1  # <-- Command never reaches engine

    def test_submit_order_when_reducing_and_sell_order_adds_then_denies(self):
        # Arrange
        self.risk_engine.set_max_notional_per_order(_AUDUSD_SIM.id, 1_000_000)

        # Initialize market
        quote = TestDataStubs.quote_tick(_AUDUSD_SIM)
        self.cache.add_quote_tick(quote)

        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order1 = strategy.order_factory.market(
            _AUDUSD_SIM.id,
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
        self.risk_engine.set_trading_state(TradingState.REDUCING)  # <-- Allow reducing orders only

        order2 = strategy.order_factory.market(
            _AUDUSD_SIM.id,
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
        self.exec_engine.process(TestEventStubs.order_filled(order1, _AUDUSD_SIM))

        # Act
        self.risk_engine.execute(submit_order2)

        # Assert
        assert order1.status == OrderStatus.FILLED
        assert order2.status == OrderStatus.DENIED
        assert self.portfolio.is_net_short(_AUDUSD_SIM.id)
        assert self.exec_engine.command_count == 1  # <-- Command never reaches engine

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
        )

        order = strategy.order_factory.market(
            _AUDUSD_SIM.id,
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
        self.risk_engine.set_trading_state(TradingState.HALTED)  # <-- Halt trading

        # Act
        self.risk_engine.execute(submit_order)

        # Assert
        assert order.status == OrderStatus.DENIED
        assert self.risk_engine.command_count == 1  # <-- Command never reaches engine

    def test_submit_order_beyond_rate_limit_then_denies_order(self):
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act
        order = None
        for _ in range(101):
            order = strategy.order_factory.market(
                _AUDUSD_SIM.id,
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

            self.risk_engine.execute(submit_order)

        # Assert
        assert order
        assert order.status == OrderStatus.DENIED
        assert isinstance(order.last_event, OrderDenied)
        assert self.risk_engine.command_count == 101
        assert self.exec_engine.command_count == 100  # <-- Does not send last submit event

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
        )

        entry = strategy.order_factory.market(
            _AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        stop_loss = strategy.order_factory.stop_market(
            _AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
        )

        take_profit = strategy.order_factory.limit(
            _AUDUSD_SIM.id,
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
        self.risk_engine.set_trading_state(TradingState.HALTED)  # <-- Halt trading

        # Act
        self.risk_engine.execute(submit_bracket)

        # Assert
        assert entry.status == OrderStatus.DENIED
        assert stop_loss.status == OrderStatus.DENIED
        assert take_profit.status == OrderStatus.DENIED
        assert self.risk_engine.command_count == 1  # <-- Command never reaches engine

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
        )

        # Push portfolio LONG
        long = strategy.order_factory.market(
            _AUDUSD_SIM.id,
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
        self.exec_engine.process(TestEventStubs.order_filled(long, _AUDUSD_SIM))

        entry = strategy.order_factory.market(
            _AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        stop_loss = strategy.order_factory.stop_market(
            _AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
        )

        take_profit = strategy.order_factory.limit(
            _AUDUSD_SIM.id,
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
        self.risk_engine.set_trading_state(TradingState.REDUCING)  # <-- Allow reducing orders only

        # Act
        self.risk_engine.execute(submit_bracket)

        # Assert
        assert entry.status == OrderStatus.DENIED
        assert stop_loss.status == OrderStatus.DENIED
        assert take_profit.status == OrderStatus.DENIED
        assert self.risk_engine.command_count == 1  # <-- Command never reaches engine

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
        )

        # Push portfolio SHORT
        short = strategy.order_factory.market(
            _AUDUSD_SIM.id,
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
        self.exec_engine.process(TestEventStubs.order_filled(short, _AUDUSD_SIM))

        entry = strategy.order_factory.market(
            _AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
        )

        stop_loss = strategy.order_factory.stop_market(
            _AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
        )

        take_profit = strategy.order_factory.limit(
            _AUDUSD_SIM.id,
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
        self.risk_engine.set_trading_state(TradingState.REDUCING)  # <-- Allow reducing orders only

        # Act
        self.risk_engine.execute(submit_bracket)

        # Assert
        assert entry.status == OrderStatus.DENIED
        assert stop_loss.status == OrderStatus.DENIED
        assert take_profit.status == OrderStatus.DENIED
        assert self.risk_engine.command_count == 1  # <-- Command never reaches engine

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
        )

        bracket = strategy.order_factory.bracket(
            _AUDUSD_SIM.id,
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
        )

        bracket = strategy.order_factory.bracket(
            _AUDUSD_SIM.id,
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
        )

        bracket = strategy.order_factory.bracket(
            _GBPUSD_SIM.id,
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
        assert self.exec_engine.command_count == 0  # <-- Command never reaches engine

    def test_submit_order_for_emulation_sends_command_to_emulator(self):
        # Arrange
        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order = strategy.order_factory.limit(
            _AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(1_000),
            Price.from_str("1.00000"),
            emulation_trigger=TriggerType.LAST_PRICE,
        )

        # Act
        strategy.submit_order(order)

        # Assert
        assert self.emulator.get_submit_order_commands().get(order.client_order_id)

    # -- MODIFY ORDER TESTS -----------------------------------------------------------------------

    def test_modify_order_when_no_order_found_logs_error(self):
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        modify = ModifyOrder(
            self.trader_id,
            strategy.id,
            _AUDUSD_SIM.id,
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

    def test_modify_order_beyond_rate_limit_then_rejects(self):
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order = strategy.order_factory.stop_market(
            _AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00010"),
        )

        strategy.submit_order(order)

        # Act
        for i in range(101):
            modify = ModifyOrder(
                self.trader_id,
                strategy.id,
                _AUDUSD_SIM.id,
                order.client_order_id,
                VenueOrderId("1"),
                Quantity.from_int(100_000),
                Price(1.00011 + 0.00001 * i, precision=5),
                None,
                UUID4(),
                self.clock.timestamp_ns(),
            )

            self.risk_engine.execute(modify)

        # Assert
        assert isinstance(order.last_event, OrderModifyRejected)
        assert self.risk_engine.command_count == 102
        assert self.exec_engine.command_count == 101  # <-- Does not send last modify event

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
        )

        order = strategy.order_factory.stop_market(
            _AUDUSD_SIM.id,
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
        )

        order = strategy.order_factory.stop_market(
            _AUDUSD_SIM.id,
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

    def test_submit_order_with_passed_gtd(self):
        # Arrange
        self.exec_engine.start()

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.clock.set_time(2_000)  # <-- Set clock to 2,000 nanos past epoch

        order = strategy.order_factory.stop_market(
            _AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00020"),
            time_in_force=TimeInForce.GTD,
            expire_time=pd.Timestamp(1_000),  # <-- Expire time prior to time now
        )
        submit_order = SubmitOrder(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        self.risk_engine.execute(submit_order)

        # Assert
        assert order.status == OrderStatus.DENIED


class TestRiskEngineWithBettingAccount:
    def setup(self):
        # Fixture Setup
        self.clock = TestClock()
        self.trader_id = TestIdStubs.trader_id()
        self.account_id = TestIdStubs.account_id()
        self.venue = Venue("SIM")
        self.instrument = TestInstrumentProvider.betting_instrument(venue=self.venue.value)

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
        )

        self.cache = TestComponentStubs.cache()

        self.portfolio = Portfolio(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.exec_engine = ExecutionEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            config=ExecEngineConfig(debug=True),
        )

        self.risk_engine = RiskEngine(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            config=RiskEngineConfig(debug=True),
        )

        self.emulator = OrderEmulator(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.exec_client = MockExecutionClient(
            client_id=ClientId(self.venue.value),
            venue=self.venue,
            account_type=AccountType.BETTING,
            base_currency=GBP,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.exec_engine.register_client(self.exec_client)

        # Set account balance
        self.account_state = TestEventStubs.betting_account_state(
            balance=1000,
            account_id=self.account_id,
        )
        self.portfolio.update_account(self.account_state)

        # Prepare data
        self.cache.add_instrument(self.instrument)
        self.quote_tick = TestDataStubs.quote_tick(
            self.instrument,
            bid_price=2.0,
            ask_price=3.0,
            bid_size=50,
            ask_size=50,
        )
        self.cache.add_quote_tick(self.quote_tick)

        # Strategy
        self.strategy = Strategy()
        self.strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.exec_engine.start()

    @pytest.mark.parametrize(
        "side,quantity,price,expected_status",
        [
            (OrderSide.SELL, 500, 2.0, OrderStatus.INITIALIZED),
            (OrderSide.SELL, 999, 2.0, OrderStatus.INITIALIZED),
            (OrderSide.SELL, 1100, 2.0, OrderStatus.DENIED),
            (OrderSide.BUY, 100, 5.0, OrderStatus.INITIALIZED),
            (OrderSide.BUY, 150, 5.0, OrderStatus.INITIALIZED),
            (OrderSide.BUY, 300, 5.0, OrderStatus.DENIED),
        ],
    )
    def test_submit_order_when_market_order_and_over_free_balance_then_denies(
        self,
        side,
        quantity,
        price,
        expected_status,
    ):
        # Arrange
        order = self.strategy.order_factory.limit(
            self.instrument.id,
            side,
            Quantity.from_int(quantity),
            Price(price, precision=1),
        )
        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_order)

        # Assert
        assert order.status == expected_status


class TestRiskEngineWithCryptoCashAccount:
    def setup(self):
        # Fixture Setup
        self.clock = TestClock()
        self.trader_id = TestIdStubs.trader_id()
        self.account_id = AccountId("BINANCE-001")
        self.venue = Venue("BINANCE")

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
        )

        self.cache = TestComponentStubs.cache()

        self.portfolio = Portfolio(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.exec_engine = ExecutionEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            config=ExecEngineConfig(debug=True),
        )

        self.risk_engine = RiskEngine(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            config=RiskEngineConfig(debug=True),
        )

        self.emulator = OrderEmulator(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.exec_client = MockExecutionClient(
            client_id=ClientId(self.venue.value),
            venue=self.venue,
            account_type=AccountType.CASH,
            base_currency=USD,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        balances = [
            AccountBalance(
                Money(440, ADA),
                Money(0, ADA),
                Money(440, ADA),
            ),
            AccountBalance(
                Money(268.84000000, USDT),
                Money(0, USDT),
                Money(268.84000000, USDT),
            ),
            AccountBalance(
                Money(0.00000000, ETH),
                Money(0, ETH),
                Money(0.00000000, ETH),
            ),
        ]

        account_state = AccountState(
            account_id=self.account_id,
            account_type=AccountType.CASH,
            base_currency=None,
            reported=True,  # reported
            balances=balances,
            margins=[],
            info={},
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        self.portfolio.update_account(account_state)
        self.exec_engine.register_client(self.exec_client)

        self.risk_engine.start()
        self.exec_engine.start()

    @pytest.mark.parametrize(
        ("order_side"),
        [
            OrderSide.BUY,
            OrderSide.SELL,
        ],
    )
    def test_submit_order_for_less_than_max_cum_transaction_value_adausdt(
        self,
        order_side: OrderSide,
    ) -> None:
        # Arrange
        self.cache.add_instrument(_ADAUSDT_BINANCE)
        quote = TestDataStubs.quote_tick(
            instrument=_ADAUSDT_BINANCE,
            bid_price=0.6109,
            ask_price=0.6110,
        )
        self.cache.add_quote_tick(quote)

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order = strategy.order_factory.market(
            _ADAUSDT_BINANCE.id,
            order_side,
            Quantity.from_int(440),
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
        assert order.status == OrderStatus.INITIALIZED
        assert self.exec_engine.command_count == 1

    @pytest.mark.skip(reason="WIP")
    def test_partial_fill_and_full_fill_account_balance_correct(self):
        # Arrange
        self.cache.add_instrument(_ETHUSDT_BINANCE)
        quote = TestDataStubs.quote_tick(
            instrument=_ETHUSDT_BINANCE,
            bid_price=10_000.00,
            ask_price=10_000.10,
        )
        self.cache.add_quote_tick(quote)

        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order1 = strategy.order_factory.market(
            _ETHUSDT_BINANCE.id,
            OrderSide.BUY,
            _ETHUSDT_BINANCE.make_qty(0.02),
        )

        order2 = strategy.order_factory.market(
            _ETHUSDT_BINANCE.id,
            OrderSide.BUY,
            _ETHUSDT_BINANCE.make_qty(0.02),
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
        self.exec_engine.process(TestEventStubs.order_submitted(order1, account_id=self.account_id))
        self.exec_engine.process(TestEventStubs.order_accepted(order1))
        self.exec_engine.process(
            TestEventStubs.order_filled(
                order1,
                _ETHUSDT_BINANCE,
                account_id=self.account_id,
                last_qty=_ETHUSDT_BINANCE.make_qty(0.0005),
            ),
        )

        submit_order2 = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=strategy.id,
            position_id=PositionId("P-19700101-000000-000-None-1"),
            order=order2,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_order2)
        self.exec_engine.process(TestEventStubs.order_submitted(order2, account_id=self.account_id))
        self.exec_engine.process(TestEventStubs.order_accepted(order2))
        self.exec_engine.process(
            TestEventStubs.order_filled(
                order2,
                _ETHUSDT_BINANCE,
                account_id=self.account_id,
            ),
        )

        # Assert
        account = self.cache.account(self.account_id)
        assert account.balance(_ETHUSDT_BINANCE.base_currency).total == Money(0.00000000, ETH)
        assert self.portfolio.net_position(_ETHUSDT_BINANCE.id) == Decimal("0.02050")
