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

import sys
from datetime import timedelta

import pytest

from nautilus_trader.backtest.data_client import BacktestMarketDataClient
from nautilus_trader.common.actor import Actor
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.enums import ComponentState
from nautilus_trader.common.enums import LogLevel
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.logging import LoggerAdapter
from nautilus_trader.config import ActorConfig
from nautilus_trader.config import ImportableActorConfig
from nautilus_trader.core.data import Data
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.model.currencies import EUR
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.data.base import DataType
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.events.order import OrderDenied
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ComponentId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.msgbus.bus import MessageBus
from nautilus_trader.persistence.streaming.writer import StreamingFeatherWriter
from nautilus_trader.test_kit.mocks.actors import KaboomActor
from nautilus_trader.test_kit.mocks.actors import MockActor
from nautilus_trader.test_kit.mocks.data import data_catalog_setup
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs import UNIX_EPOCH
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from nautilus_trader.trading.filters import NewsEvent
from nautilus_trader.trading.filters import NewsImpact


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
GBPUSD_SIM = TestInstrumentProvider.default_fx_ccy("GBP/USD")
USDJPY_SIM = TestInstrumentProvider.default_fx_ccy("USD/JPY")


class TestActor:
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
        self.component_id = "MyComponent-001"

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
            logger=self.logger,
        )

        self.cache = TestComponentStubs.cache()

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

    def test_actor_fully_qualified_name(self):
        # Arrange
        config = ActorConfig(component_id="ALPHA-01")
        actor = Actor(config=config)

        # Act
        result = actor.to_importable_config()

        # Assert
        assert isinstance(result, ImportableActorConfig)
        assert result.actor_path == "nautilus_trader.common.actor:Actor"
        assert result.config_path == "nautilus_trader.config.common:ActorConfig"
        assert result.config == {"component_id": "ALPHA-01"}

    def test_id(self):
        # Arrange, Act
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Assert
        assert actor.id == ComponentId(self.component_id)

    def test_pre_initialization(self):
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))

        # Act, Assert
        assert actor.state == ComponentState.PRE_INITIALIZED
        assert not actor.is_initialized

    def test_initialization(self):
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Act, Assert
        assert actor.state == ComponentState.READY
        assert actor.is_initialized

    def test_register_warning_event(self):
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Act
        actor.register_warning_event(OrderDenied)

        # Assert
        assert True  # Exception not raised

    def test_deregister_warning_event(self):
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        actor.register_warning_event(OrderDenied)

        # Act
        actor.deregister_warning_event(OrderDenied)

        # Assert
        assert True  # Exception not raised

    def test_handle_event(self):
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        event = TestEventStubs.cash_account_state()

        # Act
        actor.handle_event(event)

        # Assert
        assert True  # Exception not raised

    def test_on_start_when_not_overridden_does_nothing(self):
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Act
        actor.on_start()

        # Assert
        assert True  # Exception not raised

    def test_on_stop_when_not_overridden_does_nothing(self):
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Act
        actor.on_stop()

        # Assert
        assert True  # Exception not raised

    def test_on_resume_when_not_overridden_does_nothing(self):
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Act
        actor.on_resume()

        # Assert
        assert True  # Exception not raised

    def test_on_reset_when_not_overridden_does_nothing(self):
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Act
        actor.on_reset()

        # Assert
        assert True  # Exception not raised

    def test_on_dispose_when_not_overridden_does_nothing(self):
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Act
        actor.on_dispose()

        # Assert
        assert True  # Exception not raised

    def test_on_degrade_when_not_overridden_does_nothing(self):
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Act
        actor.on_degrade()

        # Assert
        assert True  # Exception not raised

    def test_on_fault_when_not_overridden_does_nothing(self):
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Act
        actor.on_fault()

        # Assert
        assert True  # Exception not raised

    def test_on_instrument_when_not_overridden_does_nothing(self):
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Act
        actor.on_instrument(TestInstrumentProvider.btcusdt_binance())

        # Assert
        assert True  # Exception not raised

    def test_on_order_book_when_not_overridden_does_nothing(self):
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Act
        actor.on_order_book(TestDataStubs.order_book())

        # Assert
        assert True  # Exception not raised

    def test_on_order_book_delta_when_not_overridden_does_nothing(self):
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Act
        actor.on_order_book_delta(TestDataStubs.order_book_snapshot())

        # Assert
        assert True  # Exception not raised

    def test_on_ticker_when_not_overridden_does_nothing(self):
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Act
        actor.on_ticker(TestDataStubs.ticker())

        # Assert
        assert True  # Exception not raised

    def test_on_venue_status_update_when_not_overridden_does_nothing(self):
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Act
        actor.on_venue_status_update(TestDataStubs.venue_status_update())

        # Assert
        assert True  # Exception not raised

    def test_on_instrument_status_update_when_not_overridden_does_nothing(self):
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Act
        actor.on_instrument_status_update(TestDataStubs.instrument_status_update())

        # Assert
        assert True  # Exception not raised

    def test_on_event_when_not_overridden_does_nothing(self):
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Act
        actor.on_event(TestEventStubs.cash_account_state())

        # Assert
        assert True  # Exception not raised

    def test_on_quote_tick_when_not_overridden_does_nothing(self):
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        tick = TestDataStubs.quote_tick_5decimal()

        # Act
        actor.on_quote_tick(tick)

        # Assert
        assert True  # Exception not raised

    def test_on_trade_tick_when_not_overridden_does_nothing(self):
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        tick = TestDataStubs.trade_tick_5decimal()

        # Act
        actor.on_trade_tick(tick)

        # Assert
        assert True  # Exception not raised

    def test_on_bar_when_not_overridden_does_nothing(self):
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        bar = TestDataStubs.bar_5decimal()

        # Act
        actor.on_bar(bar)

        # Assert
        assert True  # Exception not raised

    def test_on_historical_data_when_not_overridden_does_nothing(self):
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        bar = TestDataStubs.bar_5decimal()

        # Act
        actor.on_historical_data(bar)

        # Assert
        assert True  # Exception not raised

    def test_on_data_when_not_overridden_does_nothing(self):
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

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

    def test_start_when_invalid_state_does_not_start(self):
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Act
        actor.start()

        # Assert
        assert actor.state == ComponentState.RUNNING

    def test_stop_when_invalid_state_does_not_stop(self):
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Act
        actor.stop()

        # Assert
        assert actor.state == ComponentState.READY

    def test_resume_when_invalid_state_does_not_resume(self):
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Act
        actor.resume()

        # Assert
        assert actor.state == ComponentState.READY

    def test_reset_when_invalid_state_does_not_reset(self):
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Act
        actor.reset()

        # Assert
        assert actor.state == ComponentState.READY

    def test_dispose_when_invalid_state_does_not_dispose(self):
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Act
        actor.dispose()

        # Assert
        assert actor.state == ComponentState.DISPOSED

    def test_degrade_when_invalid_state_does_not_degrade(self):
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Act
        actor.degrade()

        # Assert
        assert actor.state == ComponentState.READY

    def test_fault_when_invalid_state_does_not_fault(self):
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Act
        actor.fault()

        # Assert
        assert actor.state == ComponentState.READY

    def test_start_when_user_code_raises_error_logs_and_reraises(self):
        # Arrange
        actor = KaboomActor()
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Act, Assert
        with pytest.raises(RuntimeError):
            actor.start()
        assert actor.state == ComponentState.STARTING

    def test_stop_when_user_code_raises_error_logs_and_reraises(self):
        # Arrange
        actor = KaboomActor()
        actor.register_base(
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
        assert actor.state == ComponentState.STOPPING

    def test_resume_when_user_code_raises_error_logs_and_reraises(self):
        # Arrange
        actor = KaboomActor()
        actor.register_base(
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
        assert actor.state == ComponentState.RESUMING

    def test_reset_when_user_code_raises_error_logs_and_reraises(self):
        # Arrange
        actor = KaboomActor()
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Act, Assert
        with pytest.raises(RuntimeError):
            actor.reset()
        assert actor.state == ComponentState.RESETTING

    def test_dispose_when_user_code_raises_error_logs_and_reraises(self):
        # Arrange
        actor = KaboomActor()
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Act, Assert
        with pytest.raises(RuntimeError):
            actor.dispose()
        assert actor.state == ComponentState.DISPOSING

    def test_degrade_when_user_code_raises_error_logs_and_reraises(self):
        # Arrange
        actor = KaboomActor()
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        actor.set_explode_on_start(False)
        actor.start()

        # Act, Assert
        with pytest.raises(RuntimeError):
            actor.degrade()
        assert actor.state == ComponentState.DEGRADING

    def test_fault_when_user_code_raises_error_logs_and_reraises(self):
        # Arrange
        actor = KaboomActor()
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        actor.set_explode_on_start(False)
        actor.start()

        # Act, Assert
        with pytest.raises(RuntimeError):
            actor.fault()
        assert actor.state == ComponentState.FAULTING

    def test_handle_quote_tick_when_user_code_raises_exception_logs_and_reraises(self):
        # Arrange
        actor = KaboomActor()
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        actor.set_explode_on_start(False)
        actor.start()

        tick = TestDataStubs.quote_tick_5decimal(AUDUSD_SIM.id)

        # Act, Assert
        with pytest.raises(RuntimeError):
            actor.handle_quote_tick(tick)

    def test_handle_trade_tick_when_user_code_raises_exception_logs_and_reraises(self):
        # Arrange
        actor = KaboomActor()
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        actor.set_explode_on_start(False)
        actor.start()

        tick = TestDataStubs.trade_tick_5decimal(AUDUSD_SIM.id)

        # Act, Assert
        with pytest.raises(RuntimeError):
            actor.handle_trade_tick(tick)

    def test_handle_bar_when_user_code_raises_exception_logs_and_reraises(self):
        # Arrange
        actor = KaboomActor()
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        actor.set_explode_on_start(False)
        actor.start()

        bar = TestDataStubs.bar_5decimal()

        # Act, Assert
        with pytest.raises(RuntimeError):
            actor.handle_bar(bar)

    def test_handle_data_when_user_code_raises_exception_logs_and_reraises(self):
        # Arrange
        actor = KaboomActor()
        actor.register_base(
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
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        actor.set_explode_on_start(False)
        actor.start()

        event = TestEventStubs.cash_account_state(account_id=AccountId("TEST-000"))

        # Act, Assert
        with pytest.raises(RuntimeError):
            actor.on_event(event)

    def test_start(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
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
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Act
        actor.reset()

        # Assert
        assert "on_reset" in actor.calls
        assert actor.state == ComponentState.READY

    def test_dispose(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
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

    def test_degrade(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        actor.start()

        # Act
        actor.degrade()

        # Assert
        assert "on_degrade" in actor.calls
        assert actor.state == ComponentState.DEGRADED

    def test_fault(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        actor.start()

        # Act
        actor.fault()

        # Assert
        assert "on_fault" in actor.calls
        assert actor.state == ComponentState.FAULTED

    def test_handle_instrument_with_blow_up_logs_exception(self):
        # Arrange
        actor = KaboomActor()
        actor.register_base(
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
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Act
        actor.handle_instrument(AUDUSD_SIM)

        # Assert
        assert actor.calls == []
        assert actor.store == []

    def test_handle_instrument_when_running_sends_to_on_instrument(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
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
        assert actor.store[0] == AUDUSD_SIM

    def test_handle_instruments_when_running_sends_to_on_instruments(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        actor.start()

        # Act
        actor.handle_instruments([AUDUSD_SIM])

        # Assert
        assert actor.calls == ["on_start", "on_instrument"]
        assert actor.store[0] == AUDUSD_SIM

    def test_handle_instruments_when_not_running_does_not_send_to_on_instrument(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Act
        actor.handle_instruments([AUDUSD_SIM])

        # Assert
        assert actor.calls == []
        assert actor.store == []

    def test_handle_ticker_when_not_running_does_not_send_to_on_quote_tick(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        tick = TestDataStubs.quote_tick_5decimal(AUDUSD_SIM.id)

        # Act
        actor.handle_quote_tick(tick)

        # Assert
        assert actor.calls == []
        assert actor.store == []

    def test_handle_ticker_when_running_sends_to_on_quote_tick(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        actor.start()

        ticker = TestDataStubs.ticker()

        # Act
        actor.handle_ticker(ticker)

        # Assert
        assert actor.calls == ["on_start", "on_ticker"]
        assert actor.store[0] == ticker

    def test_handle_quote_tick_when_not_running_does_not_send_to_on_quote_tick(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        ticker = TestDataStubs.ticker()

        # Act
        actor.handle_ticker(ticker)

        # Assert
        assert actor.calls == []
        assert actor.store == []

    def test_handle_quote_tick_when_running_sends_to_on_quote_tick(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        actor.start()

        tick = TestDataStubs.quote_tick_5decimal(AUDUSD_SIM.id)

        # Act
        actor.handle_quote_tick(tick)

        # Assert
        assert actor.calls == ["on_start", "on_quote_tick"]
        assert actor.store[0] == tick

    def test_handle_trade_tick_when_not_running_does_not_send_to_on_trade_tick(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        tick = TestDataStubs.trade_tick_5decimal(AUDUSD_SIM.id)

        # Act
        actor.handle_trade_tick(tick)

        # Assert
        assert actor.calls == []
        assert actor.store == []

    def test_handle_trade_tick_when_running_sends_to_on_trade_tick(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        actor.start()

        tick = TestDataStubs.trade_tick_5decimal(AUDUSD_SIM.id)

        # Act
        actor.handle_trade_tick(tick)

        # Assert
        assert actor.calls == ["on_start", "on_trade_tick"]
        assert actor.store == [tick]

    def test_handle_bar_when_not_running_does_not_send_to_on_bar(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        bar = TestDataStubs.bar_5decimal()

        # Act
        actor.handle_bar(bar)

        # Assert
        assert actor.calls == []
        assert actor.store == []

    def test_handle_bar_when_running_sends_to_on_bar(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        actor.start()

        bar = TestDataStubs.bar_5decimal()

        # Act
        actor.handle_bar(bar)

        # Assert
        assert actor.calls == ["on_start", "on_bar"]
        assert actor.store[0] == bar

    def test_handle_bars(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )
        result = []
        actor.on_historical_data = result.append

        actor.start()

        bars = [TestDataStubs.bar_5decimal(), TestDataStubs.bar_5decimal()]

        # Act
        actor.handle_bars(bars)

        # Assert
        assert result == bars

    def test_handle_data_when_not_running_does_not_send_to_on_data(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
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
        assert actor.store == []

    def test_handle_data_when_running_sends_to_on_data(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
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
        assert actor.store[0] == data

    def test_subscribe_custom_data(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        data_type = DataType(NewsEvent, {"type": "NEWS_WIRE", "topic": "Earthquake"})

        # Act
        actor.subscribe_data(data_type)

        # Assert
        assert self.data_engine.command_count == 0
        assert (
            actor.msgbus.subscriptions()[0].topic
            == "data.NewsEvent.type=NEWS_WIRE.topic=Earthquake"
        )

    def test_subscribe_custom_data_with_client_id(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        data_type = DataType(NewsEvent, {"type": "NEWS_WIRE", "topic": "Earthquake"})

        # Act
        actor.subscribe_data(data_type, ClientId("QUANDL"))

        # Assert
        assert self.data_engine.command_count == 1
        assert (
            actor.msgbus.subscriptions()[0].topic
            == "data.NewsEvent.type=NEWS_WIRE.topic=Earthquake"
        )

    def test_unsubscribe_custom_data(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        data_type = DataType(NewsEvent, {"type": "NEWS_WIRE", "topic": "Earthquake"})
        actor.subscribe_data(data_type)

        # Act
        actor.unsubscribe_data(data_type)

        # Assert
        assert self.data_engine.command_count == 0
        assert actor.msgbus.subscriptions() == []

    def test_unsubscribe_custom_data_with_client_id(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        data_type = DataType(NewsEvent, {"type": "NEWS_WIRE", "topic": "Earthquake"})
        actor.subscribe_data(data_type, ClientId("QUANDL"))

        # Act
        actor.unsubscribe_data(data_type, ClientId("QUANDL"))

        # Assert
        assert self.data_engine.command_count == 2
        assert actor.msgbus.subscriptions() == []

    def test_subscribe_order_book(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Act
        actor.subscribe_order_book_snapshots(AUDUSD_SIM.id, book_type=BookType.L2_MBP)

        # Assert
        assert self.data_engine.command_count == 1

    def test_unsubscribe_order_book(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        actor.subscribe_order_book_snapshots(AUDUSD_SIM.id, book_type=BookType.L2_MBP)

        # Act
        actor.unsubscribe_order_book_snapshots(AUDUSD_SIM.id)

        # Assert
        assert self.data_engine.command_count == 2

    def test_subscribe_order_book_data(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Act
        actor.subscribe_order_book_deltas(AUDUSD_SIM.id, book_type=BookType.L2_MBP)

        # Assert
        assert self.data_engine.command_count == 1

    def test_unsubscribe_order_book_deltas(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
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

    def test_unsubscribe_instruments(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
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

    def test_publish_data_sends_to_subscriber(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
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
        actor.publish_data(data_type=DataType(Data), data=data)

        # Assert
        assert data in handler

    def test_publish_signal_warns_invalid_type(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Act, Assert
        with pytest.raises(KeyError):
            actor.publish_signal(name="test", value={"a": 1}, ts_event=0)

    def test_publish_signal_sends_to_subscriber(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
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
        value = 5.0
        actor.publish_signal(name="test", value=value, ts_event=0)

        # Assert
        msg = handler[0]
        assert isinstance(msg, Data)
        assert msg.ts_event == 0
        assert msg.ts_init == 0
        assert msg.value == value

    @pytest.mark.skipif(sys.platform == "win32", reason="test path broken on Windows")
    def test_publish_data_persist(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )
        catalog = data_catalog_setup(protocol="memory")

        writer = StreamingFeatherWriter(
            path=catalog.path,
            fs_protocol=catalog.fs_protocol,
            logger=LoggerAdapter(
                component_name="Actor",
                logger=self.logger,
            ),
            replace=True,
        )
        self.msgbus.subscribe("data*", writer.write)

        # Act
        actor.publish_signal(name="Test", value=5.0, ts_event=0)

        # Assert
        assert catalog.fs.exists(f"{catalog.path}/genericdata_SignalTest.feather")

    def test_subscribe_bars(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        bar_type = TestDataStubs.bartype_audusd_1min_bid()

        # Act
        actor.subscribe_bars(bar_type)

        # Assert
        assert self.data_engine.subscribed_bars() == [bar_type]
        assert self.data_engine.command_count == 1

    def test_unsubscribe_bars(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        bar_type = TestDataStubs.bartype_audusd_1min_bid()

        actor.subscribe_bars(bar_type)

        # Act
        actor.unsubscribe_bars(bar_type)

        # Assert
        assert self.data_engine.subscribed_bars() == []
        assert self.data_engine.command_count == 2

    def test_subscribe_venue_status_updates(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        actor.subscribe_venue_status_updates(Venue("NYMEX"))

        # Assert
        # TODO(cs): DataEngine.subscribed_venue_status_updates()

    def test_request_data_sends_request_to_data_engine(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        data_type = DataType(NewsEvent, {"type": "NEWS_WIRE", "topic": "Earthquakes"})

        # Act
        actor.request_data(ClientId("BLOOMBERG-01"), data_type)

        # Assert
        assert self.data_engine.request_count == 1

    def test_request_quote_ticks_sends_request_to_data_engine(self):
        # Arrange
        actor = MockActor()
        actor.register_base(
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
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        bar_type = TestDataStubs.bartype_audusd_1min_bid()

        # Act
        actor.request_bars(bar_type)

        # Assert
        assert self.data_engine.request_count == 1

    @pytest.mark.parametrize(
        ("start", "stop"),
        [
            (UNIX_EPOCH, UNIX_EPOCH),
            (UNIX_EPOCH + timedelta(milliseconds=1), UNIX_EPOCH),
        ],
    )
    def test_request_bars_with_invalid_params_raises_value_error(self, start, stop):
        # Arrange
        actor = MockActor()
        actor.register_base(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        bar_type = TestDataStubs.bartype_audusd_1min_bid()

        # Act, Assert
        with pytest.raises(ValueError):
            actor.request_bars(bar_type, start, stop)
