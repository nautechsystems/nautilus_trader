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

import pytest

from nautilus_trader.backtest.data_client import BacktestMarketDataClient
from nautilus_trader.common.actor import Actor
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.enums import ComponentState
from nautilus_trader.common.enums import LogLevel
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.core.fsm import InvalidStateTrigger
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.model.currencies import EUR
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.data.base import Data
from nautilus_trader.model.data.base import DataType
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ComponentId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.msgbus.bus import MessageBus
from nautilus_trader.trading.filters import NewsEvent
from nautilus_trader.trading.filters import NewsImpact
from nautilus_trader.trading.strategy import TradingStrategy
from tests.test_kit.mocks import KaboomActor
from tests.test_kit.mocks import MockActor
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import UNIX_EPOCH
from tests.test_kit.stubs import TestStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
GBPUSD_SIM = TestInstrumentProvider.default_fx_ccy("GBP/USD")
USDJPY_SIM = TestInstrumentProvider.default_fx_ccy("USD/JPY")


class TestActor:
    def setup(self):
        # Fixture Setup
        self.clock = TestClock()
        self.uuid_factory = UUIDFactory()
        self.logger = Logger(
            clock=self.clock,
            level_stdout=LogLevel.DEBUG,
        )

        self.trader_id = TestStubs.trader_id()
        self.account_id = TestStubs.account_id()
        self.component_id = ComponentId("MyComponent-001")

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
            logger=self.logger,
        )

        self.cache = TestStubs.cache()

        self.data_engine = DataEngine(
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
        )

        self.data_client = BacktestMarketDataClient(
            client_id=ClientId("SIM"),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.data_engine.register_client(self.data_client)

        # Add instruments
        self.data_engine.process(AUDUSD_SIM)
        self.data_engine.process(GBPUSD_SIM)
        self.data_engine.process(USDJPY_SIM)
        self.cache.add_instrument(AUDUSD_SIM)
        self.cache.add_instrument(GBPUSD_SIM)
        self.cache.add_instrument(USDJPY_SIM)

        self.data_engine.start()
        self.exec_engine.start()

    def test_str_and_repr(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="GBP/USD-MM")

        # Act, Assert
        assert str(strategy) == "TradingStrategy-GBP/USD-MM"
        assert repr(strategy) == "TradingStrategy-GBP/USD-MM"

    def test_id(self):
        # Arrange, Act
        actor = Actor(component_id=self.component_id)

        # Assert
        assert actor.id == self.component_id

    def test_initialization(self):
        # Arrange
        actor = Actor(self.component_id)

        # Act, Assert
        assert ComponentState.INITIALIZED == actor.state

    def test_handle_event(self):
        # Arrange
        actor = Actor(self.component_id)

        event = TestStubs.event_cash_account_state()

        # Act
        actor.handle_event(event)

        # Assert
        assert True  # Exception not raised

    def test_on_start_when_not_overridden_does_nothing(self):
        # Arrange
        actor = Actor(self.component_id)

        # Act
        actor.on_start()

        # Assert
        assert True  # Exception not raised

    def test_on_stop_when_not_overridden_does_nothing(self):
        # Arrange
        actor = Actor(self.component_id)

        # Act
        actor.on_stop()

        # Assert
        assert True  # Exception not raised

    def test_on_resume_when_not_overridden_does_nothing(self):
        # Arrange
        actor = Actor(self.component_id)

        # Act
        actor.on_resume()

        # Assert
        assert True  # Exception not raised

    def test_on_reset_when_not_overridden_does_nothing(self):
        # Arrange
        actor = Actor(self.component_id)

        # Act
        actor.on_reset()

        # Assert
        assert True  # Exception not raised

    def test_on_dispose_when_not_overridden_does_nothing(self):
        # Arrange
        actor = Actor(self.component_id)

        # Act
        actor.on_dispose()

        # Assert
        assert True  # Exception not raised

    def test_on_event_when_not_overridden_does_nothing(self):
        # Arrange
        actor = Actor(self.component_id)

        # Act
        actor.on_event(TestStubs.event_cash_account_state())

        # Assert
        assert True  # Exception not raised

    def test_on_quote_tick_when_not_overridden_does_nothing(self):
        # Arrange
        actor = Actor(self.component_id)

        tick = TestStubs.quote_tick_5decimal()

        # Act
        actor.on_quote_tick(tick)

        # Assert
        assert True  # Exception not raised

    def test_on_trade_tick_when_not_overridden_does_nothing(self):
        # Arrange
        actor = Actor(self.component_id)

        tick = TestStubs.trade_tick_5decimal()

        # Act
        actor.on_trade_tick(tick)

        # Assert
        assert True  # Exception not raised

    def test_on_bar_when_not_overridden_does_nothing(self):
        # Arrange
        actor = Actor(self.component_id)

        bar = TestStubs.bar_5decimal()

        # Act
        actor.on_bar(bar)

        # Assert
        assert True  # Exception not raised

    def test_on_data_when_not_overridden_does_nothing(self):
        # Arrange
        actor = Actor(self.component_id)
        news_event = NewsEvent(
            impact=NewsImpact.HIGH,
            name="Unemployment Rate",
            currency=EUR,
            ts_event=0,
            ts_init=0,
        )

        # Act
        actor.on_data(news_event)

        # Assert
        assert True  # Exception not raised

    def test_start_when_not_registered_with_trader_raises_runtime_error(self):
        # Arrange
        actor = Actor(self.component_id)

        # Act, Assert
        with pytest.raises(RuntimeError):
            actor.start()

    def test_stop_when_not_registered_with_trader_raises_runtime_error(self):
        # Arrange
        actor = Actor(self.component_id)

        try:
            actor.start()
        except RuntimeError:
            # Normally a bad practice but allows strategy to be put into
            # the needed state to run the test.
            pass

        # Act, Assert
        with pytest.raises(RuntimeError):
            actor.stop()

    def test_resume_when_not_registered_with_trader_raises_runtime_error(self):
        # Arrange
        actor = Actor(self.component_id)

        try:
            actor.start()
        except RuntimeError:
            # Normally a bad practice but allows strategy to be put into
            # the needed state to run the test.
            pass

        try:
            actor.stop()
        except RuntimeError:
            # Normally a bad practice but allows strategy to be put into
            # the needed state to run the test.
            pass

        # Act, Assert
        with pytest.raises(RuntimeError):
            actor.resume()

    def test_reset_when_not_registered_with_trader_raises_runtime_error(self):
        # Arrange
        actor = Actor(self.component_id)

        # Act, Assert
        with pytest.raises(RuntimeError):
            actor.reset()

    def test_dispose_when_not_registered_with_trader_raises_runtime_error(self):
        # Arrange
        actor = Actor(self.component_id)

        # Act, Assert
        with pytest.raises(RuntimeError):
            actor.dispose()

    def test_start_when_not_in_valid_state_raises_invalid_state_trigger(self):
        # Arrange
        actor = Actor(self.component_id)
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        actor.dispose()  # Always a final state

        # Act, Assert
        with pytest.raises(InvalidStateTrigger):
            actor.start()

    def test_stop_when_not_in_valid_state_raises_invalid_state_trigger(self):
        # Arrange
        actor = Actor(self.component_id)
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        actor.dispose()  # Always a final state

        # Act, Assert
        with pytest.raises(InvalidStateTrigger):
            actor.stop()

    def test_resume_when_not_in_valid_state_raises_invalid_state_trigger(self):
        # Arrange
        actor = Actor(self.component_id)
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        actor.dispose()  # Always a final state

        # Act, Assert
        with pytest.raises(InvalidStateTrigger):
            actor.resume()

    def test_reset_when_not_in_valid_state_raises_invalid_state_trigger(self):
        # Arrange
        actor = Actor(self.component_id)
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        actor.dispose()  # Always a final state

        # Act, Assert
        with pytest.raises(InvalidStateTrigger):
            actor.reset()

    def test_dispose_when_not_in_valid_state_raises_invalid_state_trigger(self):
        # Arrange
        actor = Actor(self.component_id)
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        actor.dispose()  # Always a final state

        # Act, Assert
        with pytest.raises(InvalidStateTrigger):
            actor.dispose()

    def test_start_when_user_code_raises_error_logs_and_reraises(self):
        # Arrange
        actor = KaboomActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Act, Assert
        with pytest.raises(RuntimeError):
            actor.start()
        assert actor.state == ComponentState.RUNNING

    def test_stop_when_user_code_raises_error_logs_and_reraises(self):
        # Arrange
        actor = KaboomActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        actor.set_explode_on_start(False)
        actor.start()

        # Act, Assert
        with pytest.raises(RuntimeError):
            actor.stop()
        assert actor.state == ComponentState.STOPPED

    def test_resume_when_user_code_raises_error_logs_and_reraises(self):
        # Arrange
        actor = KaboomActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        actor.set_explode_on_start(False)
        actor.set_explode_on_stop(False)
        actor.start()
        actor.stop()

        # Act, Assert
        with pytest.raises(RuntimeError):
            actor.resume()
        assert actor.state == ComponentState.RUNNING

    def test_reset_when_user_code_raises_error_logs_and_reraises(self):
        # Arrange
        actor = KaboomActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Act, Assert
        with pytest.raises(RuntimeError):
            actor.reset()
        assert actor.state == ComponentState.INITIALIZED

    def test_dispose_when_user_code_raises_error_logs_and_reraises(self):
        # Arrange
        actor = KaboomActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Act, Assert
        with pytest.raises(RuntimeError):
            actor.dispose()
        assert actor.state == ComponentState.DISPOSED

    def test_handle_quote_tick_when_user_code_raises_exception_logs_and_reraises(self):
        # Arrange
        actor = KaboomActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        actor.set_explode_on_start(False)
        actor.start()

        tick = TestStubs.quote_tick_5decimal(AUDUSD_SIM.id)

        # Act, Assert
        with pytest.raises(RuntimeError):
            actor.handle_quote_tick(tick)

    def test_handle_trade_tick_when_user_code_raises_exception_logs_and_reraises(self):
        # Arrange
        actor = KaboomActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        actor.set_explode_on_start(False)
        actor.start()

        tick = TestStubs.trade_tick_5decimal(AUDUSD_SIM.id)

        # Act, Assert
        with pytest.raises(RuntimeError):
            actor.handle_trade_tick(tick)

    def test_handle_bar_when_user_code_raises_exception_logs_and_reraises(self):
        # Arrange
        actor = KaboomActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        actor.set_explode_on_start(False)
        actor.start()

        bar = TestStubs.bar_5decimal()

        # Act, Assert
        with pytest.raises(RuntimeError):
            actor.handle_bar(bar)

    def test_handle_data_when_user_code_raises_exception_logs_and_reraises(self):
        # Arrange
        actor = KaboomActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        actor.set_explode_on_start(False)
        actor.start()

        # Act, Assert
        with pytest.raises(RuntimeError):
            actor.handle_data(
                NewsEvent(
                    impact=NewsImpact.HIGH,
                    name="Unemployment Rate",
                    currency=USD,
                    ts_event=0,
                    ts_init=0,
                ),
            )

    def test_handle_event_when_user_code_raises_exception_logs_and_reraises(self):
        # Arrange
        actor = KaboomActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        actor.set_explode_on_start(False)
        actor.start()

        event = TestStubs.event_cash_account_state(account_id=AccountId("TEST", "000"))

        # Act, Assert
        with pytest.raises(RuntimeError):
            actor.on_event(event)

    def test_start(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Act
        actor.start()

        # Assert
        assert "on_start" in actor.calls
        assert actor.state == ComponentState.RUNNING

    def test_stop(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Act
        actor.start()
        actor.stop()

        # Assert
        assert "on_stop" in actor.calls
        assert actor.state == ComponentState.STOPPED

    def test_resume(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        actor.start()
        actor.stop()

        # Act
        actor.resume()

        # Assert
        assert "on_resume" in actor.calls
        assert actor.state == ComponentState.RUNNING

    def test_reset(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Act
        actor.reset()

        # Assert
        assert "on_reset" in actor.calls
        assert actor.state == ComponentState.INITIALIZED

    def test_dispose(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        actor.reset()

        # Act
        actor.dispose()

        # Assert
        assert "on_dispose" in actor.calls
        assert actor.state == ComponentState.DISPOSED

    def test_handle_instrument_with_blow_up_logs_exception(self):
        # Arrange
        actor = KaboomActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        actor.set_explode_on_start(False)
        actor.start()

        # Act, Assert
        with pytest.raises(RuntimeError):
            actor.handle_instrument(AUDUSD_SIM)

    def test_handle_instrument_when_not_running_does_not_send_to_on_instrument(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Act
        actor.handle_instrument(AUDUSD_SIM)

        # Assert
        assert actor.calls == []
        assert actor.object_storer.get_store() == []

    def test_handle_instrument_when_running_sends_to_on_instrument(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        actor.start()

        # Act
        actor.handle_instrument(AUDUSD_SIM)

        # Assert
        assert actor.calls == ["on_start", "on_instrument"]
        assert actor.object_storer.get_store()[0] == AUDUSD_SIM

    def test_handle_ticker_when_not_running_does_not_send_to_on_quote_tick(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        tick = TestStubs.quote_tick_5decimal(AUDUSD_SIM.id)

        # Act
        actor.handle_quote_tick(tick)

        # Assert
        assert actor.calls == []
        assert actor.object_storer.get_store() == []

    def test_handle_ticker_when_running_sends_to_on_quote_tick(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        actor.start()

        ticker = TestStubs.ticker()

        # Act
        actor.handle_ticker(ticker)

        # Assert
        assert actor.calls == ["on_start", "on_ticker"]
        assert actor.object_storer.get_store()[0] == ticker

    def test_handle_quote_tick_when_not_running_does_not_send_to_on_quote_tick(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        ticker = TestStubs.ticker()

        # Act
        actor.handle_ticker(ticker)

        # Assert
        assert actor.calls == []
        assert actor.object_storer.get_store() == []

    def test_handle_quote_tick_when_running_sends_to_on_quote_tick(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        actor.start()

        tick = TestStubs.quote_tick_5decimal(AUDUSD_SIM.id)

        # Act
        actor.handle_quote_tick(tick)

        # Assert
        assert actor.calls == ["on_start", "on_quote_tick"]
        assert actor.object_storer.get_store()[0] == tick

    def test_handle_trade_tick_when_not_running_does_not_send_to_on_trade_tick(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        tick = TestStubs.trade_tick_5decimal(AUDUSD_SIM.id)

        # Act
        actor.handle_trade_tick(tick)

        # Assert
        assert actor.calls == []
        assert actor.object_storer.get_store() == []

    def test_handle_trade_tick_when_running_sends_to_on_trade_tick(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        actor.start()

        tick = TestStubs.trade_tick_5decimal(AUDUSD_SIM.id)

        # Act
        actor.handle_trade_tick(tick)

        # Assert
        assert actor.calls == ["on_start", "on_trade_tick"]
        assert actor.object_storer.get_store()[0] == tick

    def test_handle_bar_when_not_running_does_not_send_to_on_bar(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        bar = TestStubs.bar_5decimal()

        # Act
        actor.handle_bar(bar)

        # Assert
        assert actor.calls == []
        assert actor.object_storer.get_store() == []

    def test_handle_bar_when_running_sends_to_on_bar(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        actor.start()

        bar = TestStubs.bar_5decimal()

        # Act
        actor.handle_bar(bar)

        # Assert
        assert actor.calls == ["on_start", "on_bar"]
        assert actor.object_storer.get_store()[0] == bar

    def test_handle_data_when_not_running_does_not_send_to_on_data(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        data = NewsEvent(
            impact=NewsImpact.HIGH,
            name="Unemployment Rate",
            currency=USD,
            ts_event=0,
            ts_init=0,
        )

        # Act
        actor.handle_data(data)

        # Assert
        assert actor.calls == []
        assert actor.object_storer.get_store() == []

    def test_handle_data_when_running_sends_to_on_data(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        actor.start()

        data = NewsEvent(
            impact=NewsImpact.HIGH,
            name="Unemployment Rate",
            currency=USD,
            ts_event=0,
            ts_init=0,
        )

        # Act
        actor.handle_data(data)

        # Assert
        assert actor.calls == ["on_start", "on_data"]
        assert actor.object_storer.get_store()[0] == data

    def test_subscribe_custom_data(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        data_type = DataType(str, {"type": "NEWS_WIRE", "topic": "Earthquake"})

        # Act
        actor.subscribe_data(ClientId("QUANDL"), data_type)

        # Assert
        assert self.data_engine.command_count == 1

    def test_unsubscribe_custom_data(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        data_type = DataType(str, {"type": "NEWS_WIRE", "topic": "Earthquake"})
        actor.subscribe_data(ClientId("QUANDL"), data_type)

        # Act
        actor.unsubscribe_data(ClientId("QUANDL"), data_type)

        # Assert
        assert self.data_engine.command_count == 2

    def test_subscribe_order_book(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Act
        actor.subscribe_order_book_snapshots(AUDUSD_SIM.id, level=2)

        # Assert
        assert self.data_engine.command_count == 1

    def test_unsubscribe_order_book(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        actor.subscribe_order_book_snapshots(AUDUSD_SIM.id, level=2)

        # Act
        actor.unsubscribe_order_book_snapshots(AUDUSD_SIM.id)

        # Assert
        assert self.data_engine.command_count == 2

    def test_subscribe_order_book_data(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Act
        actor.subscribe_order_book_deltas(AUDUSD_SIM.id, level=2)

        # Assert
        assert self.data_engine.command_count == 1

    def test_unsubscribe_order_book_deltas(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        actor.unsubscribe_order_book_deltas(AUDUSD_SIM.id)

        # Act
        actor.unsubscribe_order_book_deltas(AUDUSD_SIM.id)

        # Assert
        assert self.data_engine.command_count == 2

    def test_subscribe_instruments(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Act
        actor.subscribe_instruments(Venue("SIM"))

        # Assert
        assert self.data_engine.command_count == 1
        assert self.data_engine.subscribed_instruments() == [
            InstrumentId.from_str("AUD/USD.SIM"),
            InstrumentId.from_str("GBP/USD.SIM"),
            InstrumentId.from_str("USD/JPY.SIM"),
        ]

    # @pytest.mark.skip(reason="implement")
    def test_unsubscribe_instruments(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Act
        actor.unsubscribe_instruments(Venue("SIM"))

        # Assert
        assert self.data_engine.command_count == 1
        assert self.data_engine.subscribed_instruments() == []

    def test_subscribe_instrument(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Act
        actor.subscribe_instrument(AUDUSD_SIM.id)

        # Assert
        expected_instrument = InstrumentId(Symbol("AUD/USD"), Venue("SIM"))
        assert self.data_engine.command_count == 1
        assert self.data_engine.subscribed_instruments() == [expected_instrument]

    def test_unsubscribe_instrument(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        actor.subscribe_instrument(AUDUSD_SIM.id)

        # Act
        actor.unsubscribe_instrument(AUDUSD_SIM.id)

        # Assert
        assert self.data_engine.subscribed_instruments() == []
        assert self.data_engine.command_count == 2

    def test_subscribe_ticker(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Act
        actor.subscribe_ticker(AUDUSD_SIM.id)

        # Assert
        expected_instrument = InstrumentId(Symbol("AUD/USD"), Venue("SIM"))
        assert self.data_engine.subscribed_tickers() == [expected_instrument]
        assert self.data_engine.command_count == 1

    def test_unsubscribe_ticker(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        actor.subscribe_ticker(AUDUSD_SIM.id)

        # Act
        actor.unsubscribe_ticker(AUDUSD_SIM.id)

        # Assert
        assert self.data_engine.subscribed_tickers() == []
        assert self.data_engine.command_count == 2

    def test_subscribe_quote_ticks(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Act
        actor.subscribe_quote_ticks(AUDUSD_SIM.id)

        # Assert
        expected_instrument = InstrumentId(Symbol("AUD/USD"), Venue("SIM"))
        assert self.data_engine.subscribed_quote_ticks() == [expected_instrument]
        assert self.data_engine.command_count == 1

    def test_unsubscribe_quote_ticks(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        actor.subscribe_quote_ticks(AUDUSD_SIM.id)

        # Act
        actor.unsubscribe_quote_ticks(AUDUSD_SIM.id)

        # Assert
        assert self.data_engine.subscribed_quote_ticks() == []
        assert self.data_engine.command_count == 2

    def test_subscribe_trade_ticks(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Act
        actor.subscribe_trade_ticks(AUDUSD_SIM.id)

        # Assert
        expected_instrument = InstrumentId(Symbol("AUD/USD"), Venue("SIM"))
        assert self.data_engine.subscribed_trade_ticks() == [expected_instrument]
        assert self.data_engine.command_count == 1

    def test_unsubscribe_trade_ticks(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        actor.subscribe_trade_ticks(AUDUSD_SIM.id)

        # Act
        actor.unsubscribe_trade_ticks(AUDUSD_SIM.id)

        # Assert
        assert self.data_engine.subscribed_trade_ticks() == []
        assert self.data_engine.command_count == 2

    def test_subscribe_strategy_data(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Act
        actor.subscribe_strategy_data(data_type=Data)

        # Assert
        assert self.msgbus.has_subscribers("data.strategy.Data.*")

    def test_subscribe_strategy_data_with_strategy_filter(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Act
        actor.subscribe_strategy_data(
            data_type=Data,
            strategy_id=StrategyId("Monitor-002"),
        )

        # Assert
        assert self.msgbus.has_subscribers("data.strategy.Data.Monitor-002")

    def test_unsubscribe_strategy_data(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        actor.subscribe_strategy_data(data_type=Data)

        # Act
        actor.unsubscribe_strategy_data(data_type=Data)

        # Assert
        assert not self.msgbus.has_subscribers("data.strategy.Data.*")

    def test_unsubscribe_strategy_data_with_strategy_filter(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        actor.subscribe_strategy_data(
            data_type=Data,
            strategy_id=StrategyId("Monitor-002"),
        )

        # Act
        actor.unsubscribe_strategy_data(
            data_type=Data,
            strategy_id=StrategyId("Monitor-002"),
        )

        # Assert
        assert not self.msgbus.has_subscribers("data.strategy.Data.Monitor-002")

    def test_publish_data_sends_to_subscriber(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        handler = []
        self.msgbus.subscribe(
            topic="data*",
            handler=handler.append,
        )

        # Act
        data = Data(
            ts_event=self.clock.timestamp_ns(),
            ts_init=self.clock.timestamp_ns(),
        )
        actor.publish_data(data=data)

        # Assert
        assert data in handler

    def test_subscribe_bars(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        bar_type = TestStubs.bartype_audusd_1min_bid()

        # Act
        actor.subscribe_bars(bar_type)

        # Assert
        assert self.data_engine.subscribed_bars() == [bar_type]
        assert self.data_engine.command_count == 1

    def test_unsubscribe_bars(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        bar_type = TestStubs.bartype_audusd_1min_bid()

        actor.subscribe_bars(bar_type)

        # Act
        actor.unsubscribe_bars(bar_type)

        # Assert
        assert self.data_engine.subscribed_bars() == []
        assert self.data_engine.command_count == 2

    def test_request_data_sends_request_to_data_engine(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        data_type = DataType(str, {"type": "NEWS_WIRE", "topic": "Earthquakes"})

        # Act
        actor.request_data(ClientId("BLOOMBERG-01"), data_type)

        # Assert
        assert self.data_engine.request_count == 1

    def test_request_quote_ticks_sends_request_to_data_engine(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Act
        actor.request_quote_ticks(AUDUSD_SIM.id)

        # Assert
        assert self.data_engine.request_count == 1

    def test_request_trade_ticks_sends_request_to_data_engine(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Act
        actor.request_trade_ticks(AUDUSD_SIM.id)

        # Assert
        assert self.data_engine.request_count == 1

    def test_request_bars_sends_request_to_data_engine(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        bar_type = TestStubs.bartype_audusd_1min_bid()

        # Act
        actor.request_bars(bar_type)

        # Assert
        assert self.data_engine.request_count == 1

    @pytest.mark.parametrize(
        "start,stop",
        [
            (UNIX_EPOCH, UNIX_EPOCH),
            (UNIX_EPOCH + timedelta(milliseconds=1), UNIX_EPOCH),
        ],
    )
    def test_request_bars_with_invalid_params_raises_value_error(self, start, stop):
        # Arrange
        actor = MockActor()
        actor.register_base(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        bar_type = TestStubs.bartype_audusd_1min_bid()

        # Act, Assert
        with pytest.raises(ValueError):
            actor.request_bars(bar_type, start, stop)
