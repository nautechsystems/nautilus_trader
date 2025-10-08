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

import pytest

from nautilus_trader.backtest.data_client import BacktestMarketDataClient
from nautilus_trader.common.actor import Actor
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.component import TestClock
from nautilus_trader.common.enums import ComponentState
from nautilus_trader.common.executor import TaskId
from nautilus_trader.common.messages import ShutdownSystem
from nautilus_trader.common.signal import generate_signal_class
from nautilus_trader.config import ActorConfig
from nautilus_trader.config import ImportableActorConfig
from nautilus_trader.core.data import Data
from nautilus_trader.core.message import Command
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.data.messages import DataResponse
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.model.currencies import EUR
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import DataType
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AggregationSource
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.events import OrderDenied
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ComponentId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.persistence.writer import StreamingFeatherWriter
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.test_kit.mocks.actors import KaboomActor
from nautilus_trader.test_kit.mocks.actors import MockActor
from nautilus_trader.test_kit.mocks.data import setup_catalog
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.data import UNIX_EPOCH
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.execution import TestExecStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from nautilus_trader.trading.filters import NewsEvent
from nautilus_trader.trading.filters import NewsImpact


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
GBPUSD_SIM = TestInstrumentProvider.default_fx_ccy("GBP/USD")
USDJPY_SIM = TestInstrumentProvider.default_fx_ccy("USD/JPY")


def _create_composite_bar_type() -> BarType:
    """
    Create an internally aggregated composite bar type for testing
    request_aggregated_bars.
    """
    return BarType.new_composite(
        instrument_id=AUDUSD_SIM.id,
        bar_spec=TestDataStubs.bar_spec_1min_bid(),
        aggregation_source=AggregationSource.INTERNAL,
        composite_step=2,
        composite_aggregation=BarAggregation.MINUTE,
        composite_aggregation_source=AggregationSource.INTERNAL,
    )


class TestActor:
    @pytest.fixture(autouse=True)
    def setup_method(self, tmp_path) -> None:
        self.tmp_path = tmp_path
        self.clock = TestClock()

        self.trader_id = TestIdStubs.trader_id()
        self.account_id = TestIdStubs.account_id()
        self.component_id = "MyComponent-001"

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

        self.data_engine = DataEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.exec_engine = ExecutionEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.data_client = BacktestMarketDataClient(
            client_id=ClientId("SIM"),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
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

    def test_actor_fully_qualified_name(self) -> None:
        # Arrange
        config = ActorConfig(component_id="ALPHA-01")
        actor = Actor(config=config)

        # Act
        result = actor.to_importable_config()

        # Assert
        assert isinstance(result, ImportableActorConfig)
        assert result.actor_path == "nautilus_trader.common.actor:Actor"
        assert result.config_path == "nautilus_trader.common.config:ActorConfig"
        assert result.config == {
            "component_id": "ALPHA-01",
            "log_events": True,
            "log_commands": True,
        }

    def test_id(self) -> None:
        # Arrange, Act
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Assert
        assert actor.id == ComponentId(self.component_id)

    def test_pre_initialization(self) -> None:
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))

        # Act, Assert
        assert actor.state == ComponentState.PRE_INITIALIZED
        assert not actor.is_initialized

    def test_initialization(self) -> None:
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act, Assert
        assert actor.state == ComponentState.READY
        assert actor.is_initialized
        assert not actor.has_pending_requests()
        assert not actor.is_pending_request(UUID4())
        assert actor.pending_requests() == set()

    def test_register_warning_event(self) -> None:
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act, Assert (exception not raised)
        actor.register_warning_event(OrderDenied)

    def test_deregister_warning_event(self) -> None:
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        actor.register_warning_event(OrderDenied)

        # Act, Assert (exception not raised)
        actor.deregister_warning_event(OrderDenied)

    def test_handle_event(self) -> None:
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        event = TestEventStubs.cash_account_state()

        # Act, Assert (exception not raised)
        actor.handle_event(event)

    def test_on_start_when_not_overridden_does_nothing(self) -> None:
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act, Assert (exception not raised)
        actor.on_start()

    def test_on_stop_when_not_overridden_does_nothing(self) -> None:
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act, Assert (exception not raised)
        actor.on_stop()

    def test_on_resume_when_not_overridden_does_nothing(self) -> None:
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act, Assert (exception not raised)
        actor.on_resume()

    def test_on_reset_when_not_overridden_does_nothing(self) -> None:
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act, Assert (exception not raised)
        actor.on_reset()

    def test_on_dispose_when_not_overridden_does_nothing(self) -> None:
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act, Assert (exception not raised)
        actor.on_dispose()

    def test_on_degrade_when_not_overridden_does_nothing(self) -> None:
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act, Assert (exception not raised)
        actor.on_degrade()

    def test_on_fault_when_not_overridden_does_nothing(self) -> None:
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act, Assert (exception not raised)
        actor.on_fault()

    def test_on_instrument_when_not_overridden_does_nothing(self) -> None:
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act, Assert (exception not raised)
        actor.on_instrument(TestInstrumentProvider.btcusdt_binance())

    def test_on_order_book_when_not_overridden_does_nothing(self) -> None:
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act, Assert (exception not raised)
        actor.on_order_book(TestDataStubs.order_book())

    def test_on_order_book_delta_when_not_overridden_does_nothing(self) -> None:
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act, Assert (exception not raised)
        actor.on_order_book_deltas(TestDataStubs.order_book_snapshot())

    def test_on_instrument_status_when_not_overridden_does_nothing(self) -> None:
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act, Assert (exception not raised)
        actor.on_instrument_status(TestDataStubs.instrument_status())

    def test_on_event_when_not_overridden_does_nothing(self) -> None:
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act, Assert (exception not raised)
        actor.on_event(TestEventStubs.cash_account_state())

    def test_on_quote_tick_when_not_overridden_does_nothing(self) -> None:
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        tick = TestDataStubs.quote_tick()

        # Act, Assert (exception not raised)
        actor.on_quote_tick(tick)

    def test_on_trade_tick_when_not_overridden_does_nothing(self) -> None:
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        tick = TestDataStubs.trade_tick()

        # Act, Assert (exception not raised)
        actor.on_trade_tick(tick)

    def test_on_bar_when_not_overridden_does_nothing(self) -> None:
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        bar = TestDataStubs.bar_5decimal()

        # Act, Assert (exception not raised)
        actor.on_bar(bar)

    def test_on_historical_data_when_not_overridden_does_nothing(self) -> None:
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        bar = TestDataStubs.bar_5decimal()

        # Act, Assert (exception not raised)
        actor.on_historical_data(bar)

    def test_on_data_when_not_overridden_does_nothing(self) -> None:
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        news_event = NewsEvent(
            impact=NewsImpact.HIGH,
            name="Unemployment Rate",
            currency=EUR,
            ts_event=0,
            ts_init=0,
        )

        # Act, Assert (exception not raised)
        actor.on_data(news_event)

    def test_on_signal_when_not_overridden_does_nothing(self) -> None:
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        signal_cls = generate_signal_class("example", value_type=float)
        signal = signal_cls(1.0, 0, 0)

        # Act, Assert (exception not raised)
        actor.on_signal(signal)

    def test_start_when_invalid_state_does_not_start(self) -> None:
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act
        actor.start()

        # Assert
        assert actor.state == ComponentState.RUNNING

    def test_stop_when_invalid_state_does_not_stop(self) -> None:
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act
        actor.stop()

        # Assert
        assert actor.state == ComponentState.READY

    def test_resume_when_invalid_state_does_not_resume(self) -> None:
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act
        actor.resume()

        # Assert
        assert actor.state == ComponentState.READY

    def test_reset_when_invalid_state_does_not_reset(self) -> None:
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act
        actor.reset()

        # Assert
        assert actor.state == ComponentState.READY

    def test_dispose_when_invalid_state_does_not_dispose(self) -> None:
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act
        actor.dispose()

        # Assert
        assert actor.state == ComponentState.DISPOSED

    def test_degrade_when_invalid_state_does_not_degrade(self) -> None:
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act
        actor.degrade()

        # Assert
        assert actor.state == ComponentState.READY

    def test_fault_when_invalid_state_does_not_fault(self) -> None:
        # Arrange
        actor = Actor(config=ActorConfig(component_id=self.component_id))
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act
        actor.fault()

        # Assert
        assert actor.state == ComponentState.READY

    def test_start_when_user_code_raises_error_logs_and_reraises(self) -> None:
        # Arrange
        actor = KaboomActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act, Assert
        with pytest.raises(RuntimeError):
            actor.start()
        assert actor.state == ComponentState.STARTING

    def test_stop_when_user_code_raises_error_logs_and_reraises(self) -> None:
        # Arrange
        actor = KaboomActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        actor.set_explode_on_start(False)
        actor.start()

        # Act, Assert
        with pytest.raises(RuntimeError):
            actor.stop()
        assert actor.state == ComponentState.STOPPING

    def test_resume_when_user_code_raises_error_logs_and_reraises(self) -> None:
        # Arrange
        actor = KaboomActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        actor.set_explode_on_start(False)
        actor.set_explode_on_stop(False)
        actor.start()
        actor.stop()

        # Act, Assert
        with pytest.raises(RuntimeError):
            actor.resume()
        assert actor.state == ComponentState.RESUMING

    def test_reset_when_user_code_raises_error_logs_and_reraises(self) -> None:
        # Arrange
        actor = KaboomActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act, Assert
        with pytest.raises(RuntimeError):
            actor.reset()
        assert actor.state == ComponentState.RESETTING

    def test_dispose_when_user_code_raises_error_logs_and_reraises(self) -> None:
        # Arrange
        actor = KaboomActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act, Assert
        with pytest.raises(RuntimeError):
            actor.dispose()
        assert actor.state == ComponentState.DISPOSING

    def test_degrade_when_user_code_raises_error_logs_and_reraises(self) -> None:
        # Arrange
        actor = KaboomActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        actor.set_explode_on_start(False)
        actor.start()

        # Act, Assert
        with pytest.raises(RuntimeError):
            actor.degrade()
        assert actor.state == ComponentState.DEGRADING

    def test_fault_when_user_code_raises_error_logs_and_reraises(self) -> None:
        # Arrange
        actor = KaboomActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        actor.set_explode_on_start(False)
        actor.start()

        # Act, Assert
        with pytest.raises(RuntimeError):
            actor.fault()
        assert actor.state == ComponentState.FAULTING

    def test_handle_quote_tick_when_user_code_raises_exception_logs_and_reraises(self) -> None:
        # Arrange
        actor = KaboomActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        actor.set_explode_on_start(False)
        actor.start()

        tick = TestDataStubs.quote_tick()

        # Act, Assert
        with pytest.raises(RuntimeError):
            actor.handle_quote_tick(tick)

    def test_handle_trade_tick_when_user_code_raises_exception_logs_and_reraises(self) -> None:
        # Arrange
        actor = KaboomActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        actor.set_explode_on_start(False)
        actor.start()

        tick = TestDataStubs.trade_tick()

        # Act, Assert
        with pytest.raises(RuntimeError):
            actor.handle_trade_tick(tick)

    def test_handle_bar_when_user_code_raises_exception_logs_and_reraises(self) -> None:
        # Arrange
        actor = KaboomActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        actor.set_explode_on_start(False)
        actor.start()

        bar = TestDataStubs.bar_5decimal()

        # Act, Assert
        with pytest.raises(RuntimeError):
            actor.handle_bar(bar)

    def test_handle_data_when_user_code_raises_exception_logs_and_reraises(self) -> None:
        # Arrange
        actor = KaboomActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
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

    def test_handle_event_when_user_code_raises_exception_logs_and_reraises(self) -> None:
        # Arrange
        actor = KaboomActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        actor.set_explode_on_start(False)
        actor.start()

        event = TestEventStubs.cash_account_state(account_id=AccountId("TEST-000"))

        # Act, Assert
        with pytest.raises(RuntimeError):
            actor.on_event(event)

    def test_start(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act
        actor.start()

        # Assert
        assert "on_start" in actor.calls
        assert actor.state == ComponentState.RUNNING

    def test_stop(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act
        actor.start()
        actor.stop()

        # Assert
        assert "on_stop" in actor.calls
        assert actor.state == ComponentState.STOPPED

    def test_resume(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        actor.start()
        actor.stop()

        # Act
        actor.resume()

        # Assert
        assert "on_resume" in actor.calls
        assert actor.state == ComponentState.RUNNING

    def test_reset(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act
        actor.reset()

        # Assert
        assert "on_reset" in actor.calls
        assert actor.state == ComponentState.READY

    def test_dispose(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        actor.reset()

        # Act
        actor.dispose()

        # Assert
        assert "on_dispose" in actor.calls
        assert actor.state == ComponentState.DISPOSED

    def test_degrade(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        actor.start()

        # Act
        actor.degrade()

        # Assert
        assert "on_degrade" in actor.calls
        assert actor.state == ComponentState.DEGRADED

    def test_fault(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        actor.start()

        # Act
        actor.fault()

        # Assert
        assert "on_fault" in actor.calls
        assert actor.state == ComponentState.FAULTED

    def test_shutdown_system(self) -> None:
        # Arrange
        handler: list[Command] = []
        self.msgbus.subscribe("commands.system.shutdown", handler.append)

        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        actor.start()

        # Act
        actor.shutdown_system("test")

        # Assert
        assert len(handler) == 1
        assert isinstance(handler[0], ShutdownSystem)
        assert handler[0].reason == "test"

    def test_handle_instrument_with_blow_up_logs_exception(self) -> None:
        # Arrange
        actor = KaboomActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        actor.set_explode_on_start(False)
        actor.start()

        # Act, Assert
        with pytest.raises(RuntimeError):
            actor.handle_instrument(AUDUSD_SIM)

    def test_handle_instrument_when_not_running_does_not_send_to_on_instrument(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act
        actor.handle_instrument(AUDUSD_SIM)

        # Assert
        assert actor.calls == []
        assert actor.store == []

    def test_handle_instrument_when_running_sends_to_on_instrument(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        actor.start()

        # Act
        actor.handle_instrument(AUDUSD_SIM)

        # Assert
        assert actor.calls == ["on_start", "on_instrument"]
        assert actor.store[0] == AUDUSD_SIM

    def test_handle_instruments_when_running_sends_to_on_instruments(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        actor.start()

        # Act
        actor.handle_instruments([AUDUSD_SIM])

        # Assert
        assert actor.calls == ["on_start", "on_instrument"]
        assert actor.store[0] == AUDUSD_SIM

    def test_handle_instruments_when_not_running_does_not_send_to_on_instrument(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act
        actor.handle_instruments([AUDUSD_SIM])

        # Assert
        assert actor.calls == []
        assert actor.store == []

    def test_handle_ticker_when_not_running_does_not_send_to_on_quote_tick(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        tick = TestDataStubs.quote_tick()

        # Act
        actor.handle_quote_tick(tick)

        # Assert
        assert actor.calls == []
        assert actor.store == []

    def test_handle_quote_tick_when_not_running_does_not_send_to_on_quote_tick(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        quote = TestDataStubs.quote_tick()

        # Act
        actor.handle_quote_tick(quote)

        # Assert
        assert actor.calls == []
        assert actor.store == []

    def test_handle_quote_tick_when_running_sends_to_on_quote_tick(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        actor.start()

        tick = TestDataStubs.quote_tick()

        # Act
        actor.handle_quote_tick(tick)

        # Assert
        assert actor.calls == ["on_start", "on_quote_tick"]
        assert actor.store[0] == tick

    def test_handle_trade_tick_when_not_running_does_not_send_to_on_trade_tick(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        tick = TestDataStubs.trade_tick()

        # Act
        actor.handle_trade_tick(tick)

        # Assert
        assert actor.calls == []
        assert actor.store == []

    def test_handle_trade_tick_when_running_sends_to_on_trade_tick(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        actor.start()

        tick = TestDataStubs.trade_tick()

        # Act
        actor.handle_trade_tick(tick)

        # Assert
        assert actor.calls == ["on_start", "on_trade_tick"]
        assert actor.store == [tick]

    def test_handle_bar_when_not_running_does_not_send_to_on_bar(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        bar = TestDataStubs.bar_5decimal()

        # Act
        actor.handle_bar(bar)

        # Assert
        assert actor.calls == []
        assert actor.store == []

    def test_handle_bar_when_running_sends_to_on_bar(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        actor.start()

        bar = TestDataStubs.bar_5decimal()

        # Act
        actor.handle_bar(bar)

        # Assert
        assert actor.calls == ["on_start", "on_bar"]
        assert actor.store[0] == bar

    def test_handle_bars(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        result: list[Bar] = []
        actor.on_historical_data = result.append

        actor.start()

        bars = [TestDataStubs.bar_5decimal(), TestDataStubs.bar_5decimal()]

        # Act
        actor.handle_bars(bars)

        # Assert
        assert result == bars

    def test_handle_data_when_not_running_does_not_send_to_on_data(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
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

    def test_handle_data_when_running_sends_to_on_data(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
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

    def test_handle_signal_when_not_running_does_not_send_to_on_signal(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        signal_cls = generate_signal_class("example", value_type=float)
        signal = signal_cls(1.0, 0, 0)

        # Act
        actor.handle_signal(signal)

        # Assert
        assert actor.calls == []
        assert actor.store == []

    def test_handle_signal_when_running_sends_to_on_signal(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        actor.start()

        signal_cls = generate_signal_class("example", value_type=float)
        signal = signal_cls(1.0, 0, 0)

        # Act
        actor.handle_signal(signal)

        # Assert
        assert actor.calls == ["on_start", "on_signal"]
        assert actor.store[0] == signal

    def test_add_synthetic_instrument_when_already_exists(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        synthetic = TestInstrumentProvider.synthetic_instrument()
        actor.add_synthetic(synthetic)

        # Act, Assert
        with pytest.raises(ValueError):
            actor.add_synthetic(synthetic)

    def test_add_synthetic_instrument_when_no_synthetic(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        synthetic = TestInstrumentProvider.synthetic_instrument()

        # Act
        actor.add_synthetic(synthetic)

        # Assert
        assert actor.cache.synthetic(synthetic.id) == synthetic

    def test_update_synthetic_instrument_when_no_synthetic(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        synthetic = TestInstrumentProvider.synthetic_instrument()

        # Act, Assert
        with pytest.raises(ValueError):
            actor.update_synthetic(synthetic)

    def test_update_synthetic_instrument(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        synthetic = TestInstrumentProvider.synthetic_instrument()
        original_formula = synthetic.formula
        actor.add_synthetic(synthetic)

        new_formula = "(BTCUSDT.BINANCE + ETHUSDT.BINANCE) / 4"
        synthetic.change_formula(new_formula)
        actor.update_synthetic(synthetic)

        # Act
        assert new_formula != original_formula
        assert actor.cache.synthetic(synthetic.id).formula == new_formula

    def test_queued_task_ids_when_no_executor(self) -> None:
        """
        Test should return empty list.
        """
        # Arrange
        actor = MockActor()

        # Act, Assert
        assert actor.queued_task_ids() == []

    def test_active_task_ids_when_no_executor(self) -> None:
        """
        Test should return empty list.
        """
        # Arrange
        actor = MockActor()

        # Act, Assert
        assert actor.active_task_ids() == []

    def test_has_queued_tasks_when_no_executor(self) -> None:
        """
        Test should return false.
        """
        # Arrange
        actor = MockActor()

        # Act, Assert
        assert not actor.has_queued_tasks()

    def test_has_active_tasks_when_no_executor(self) -> None:
        """
        Test should return false.
        """
        # Arrange
        actor = MockActor()

        # Act, Assert
        assert not actor.has_active_tasks()

    def test_has_any_tasks_when_no_executor(self) -> None:
        """
        Test should return false.
        """
        # Arrange
        actor = MockActor()

        # Act, Assert
        assert not actor.has_any_tasks()

    def test_cancel_task_when_no_executor(self) -> None:
        """
        Test should do nothing and log a warning.
        """
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        unknown = TaskId.create()

        # Act, Assert
        actor.cancel_task(unknown)

    def test_cancel_all_tasks_when_no_executor(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act, Assert
        actor.cancel_all_tasks()

    def test_run_in_executor_when_no_executor(self) -> None:
        """
        Test should immediately execute the function and return a task ID.
        """
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        handler: list[str] = []
        func = handler.append
        msg = "a"

        # Act
        task_id: TaskId = actor.run_in_executor(func, (msg,))

        # Assert
        assert msg in handler
        assert len(task_id.value) == 36

    def test_queue_for_executor_when_no_executor(self) -> None:
        """
        Test should immediately execute the function and return a task ID.
        """
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        handler: list[str] = []
        func = handler.append
        msg = "a"

        # Act
        task_id: TaskId = actor.queue_for_executor(func, (msg,))

        # Assert
        assert msg in handler
        assert len(task_id.value) == 36

    def test_subscribe_custom_data(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        data_type = DataType(NewsEvent, {"type": "NEWS_WIRE", "topic": "Earthquake"})

        # Act
        actor.subscribe_data(data_type)

        # Assert
        assert self.data_engine.command_count == 0
        assert (
            actor.msgbus.subscriptions()[4].topic
            == "data.NewsEvent.type=NEWS_WIRE.topic=Earthquake"
        )

    def test_subscribe_custom_data_with_client_id(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        data_type = DataType(NewsEvent, {"type": "NEWS_WIRE", "topic": "Earthquake"})

        # Act
        actor.subscribe_data(data_type, ClientId("QUANDL"))

        # Assert
        assert self.data_engine.command_count == 1
        assert (
            actor.msgbus.subscriptions()[4].topic
            == "data.NewsEvent.type=NEWS_WIRE.topic=Earthquake"
        )

    def test_unsubscribe_custom_data(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        data_type = DataType(NewsEvent, {"type": "NEWS_WIRE", "topic": "Earthquake"})
        actor.subscribe_data(data_type)

        # Act
        actor.unsubscribe_data(data_type)

        # Assert
        assert self.data_engine.command_count == 0
        assert len(actor.msgbus.subscriptions()) == 4  # Portfolio subscriptions only

    def test_unsubscribe_custom_data_with_client_id(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        data_type = DataType(NewsEvent, {"type": "NEWS_WIRE", "topic": "Earthquake"})
        actor.subscribe_data(data_type, ClientId("QUANDL"))

        # Act
        actor.unsubscribe_data(data_type, ClientId("QUANDL"))

        # Assert
        assert self.data_engine.command_count == 2
        assert len(actor.msgbus.subscriptions()) == 4  # Portfolio subscriptions only

    def test_subscribe_order_book_deltas(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act
        actor.subscribe_order_book_deltas(AUDUSD_SIM.id, book_type=BookType.L2_MBP)

        # Assert
        assert self.data_engine.command_count == 1

    def test_unsubscribe_order_book_deltas(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        actor.subscribe_order_book_deltas(AUDUSD_SIM.id)

        # Act
        actor.unsubscribe_order_book_deltas(AUDUSD_SIM.id)

        # Assert
        assert self.data_engine.command_count == 2

    def test_subscribe_order_book_depth(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act
        actor.subscribe_order_book_depth(AUDUSD_SIM.id, book_type=BookType.L2_MBP, depth=10)

        # Assert
        assert self.data_engine.command_count == 1

    def test_unsubscribe_order_book_depth(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        actor.subscribe_order_book_depth(AUDUSD_SIM.id, book_type=BookType.L2_MBP, depth=10)

        # Act
        actor.unsubscribe_order_book_depth(AUDUSD_SIM.id)

        # Assert
        assert self.data_engine.command_count == 2

    def test_subscribe_order_book_at_interval(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act
        actor.subscribe_order_book_at_interval(AUDUSD_SIM.id, book_type=BookType.L2_MBP)

        # Assert
        assert self.data_engine.command_count == 1

    def test_unsubscribe_order_book_at_interval(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        actor.subscribe_order_book_at_interval(AUDUSD_SIM.id, book_type=BookType.L2_MBP)

        # Act
        actor.unsubscribe_order_book_at_interval(AUDUSD_SIM.id)

        # Assert
        assert self.data_engine.command_count == 2

    def test_subscribe_instruments(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
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

    def test_unsubscribe_instruments(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act
        actor.unsubscribe_instruments(Venue("SIM"))

        # Assert
        assert self.data_engine.command_count == 1
        assert self.data_engine.subscribed_instruments() == []

    def test_subscribe_instrument(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act
        actor.subscribe_instrument(AUDUSD_SIM.id)

        # Assert
        expected_instrument = InstrumentId(Symbol("AUD/USD"), Venue("SIM"))
        assert self.data_engine.command_count == 1
        assert self.data_engine.subscribed_instruments() == [expected_instrument]

    def test_unsubscribe_instrument(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        actor.subscribe_instrument(AUDUSD_SIM.id)

        # Act
        actor.unsubscribe_instrument(AUDUSD_SIM.id)

        # Assert
        assert self.data_engine.subscribed_instruments() == []
        assert self.data_engine.command_count == 2

    def test_subscribe_quote_ticks(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act
        actor.subscribe_quote_ticks(AUDUSD_SIM.id)

        # Assert
        expected_instrument = InstrumentId(Symbol("AUD/USD"), Venue("SIM"))
        assert self.data_engine.subscribed_quote_ticks() == [expected_instrument]
        assert self.data_engine.command_count == 1

    def test_unsubscribe_quote_ticks(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        actor.subscribe_quote_ticks(AUDUSD_SIM.id)

        # Act
        actor.unsubscribe_quote_ticks(AUDUSD_SIM.id)

        # Assert
        assert self.data_engine.subscribed_quote_ticks() == []
        assert self.data_engine.command_count == 2

    def test_subscribe_trade_ticks(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act
        actor.subscribe_trade_ticks(AUDUSD_SIM.id)

        # Assert
        expected_instrument = InstrumentId(Symbol("AUD/USD"), Venue("SIM"))
        assert self.data_engine.subscribed_trade_ticks() == [expected_instrument]
        assert self.data_engine.command_count == 1

    def test_unsubscribe_trade_ticks(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        actor.subscribe_trade_ticks(AUDUSD_SIM.id)

        # Act
        actor.unsubscribe_trade_ticks(AUDUSD_SIM.id)

        # Assert
        assert self.data_engine.subscribed_trade_ticks() == []
        assert self.data_engine.command_count == 2

    def test_publish_data_sends_to_subscriber(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        handler: list[Data] = []
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

    def test_publish_signal_warns_invalid_type(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act, Assert
        with pytest.raises(KeyError):
            actor.publish_signal(name="test", value={"a": 1}, ts_event=0)

    def test_publish_signal_sends_to_subscriber(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        handler: list[Data] = []
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
        assert msg.is_signal()
        assert msg.is_signal("test")
        assert msg.ts_event == 0
        assert msg.ts_init == 0
        assert msg.value == value

    def test_subscribe_signal(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act
        actor.subscribe_signal("test")

        # Assert
        assert self.data_engine.command_count == 0
        assert actor.msgbus.subscriptions()[4].topic == "data.SignalTest*"

    def test_subscribe_all_signals(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act
        actor.subscribe_signal()

        # Assert
        assert self.data_engine.command_count == 0
        assert actor.msgbus.subscriptions()[4].topic == "data.Signal*"

    def test_publish_data_persist(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.clock.set_time(1704067205000000000)
        catalog = setup_catalog(protocol="memory", path=self.tmp_path / "catalog")

        writer = StreamingFeatherWriter(
            path=catalog.path,
            cache=self.cache,
            clock=self.clock,
            fs_protocol=catalog.fs_protocol,
            replace=True,
        )
        self.msgbus.subscribe("data*", writer.write)

        # Act
        actor.publish_signal(name="Test", value=5.0, ts_event=0)

        # Assert
        assert catalog.fs.exists(f"{catalog.path}/custom_signal_test_1704067205000000000.feather")

    def test_subscribe_bars(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        bar_type = TestDataStubs.bartype_audusd_1min_bid()

        # Act
        actor.subscribe_bars(bar_type)

        # Assert
        assert self.data_engine.subscribed_bars() == [bar_type]
        assert self.data_engine.command_count == 1

    def test_unsubscribe_bars(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        bar_type = TestDataStubs.bartype_audusd_1min_bid()

        actor.subscribe_bars(bar_type)

        # Act
        actor.unsubscribe_bars(bar_type)

        # Assert
        assert self.data_engine.subscribed_bars() == []
        assert self.data_engine.command_count == 2

    def test_subscribe_order_fills(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act
        actor.subscribe_order_fills(AUDUSD_SIM.id)

        # Assert
        # Order fills are msgbus-only subscriptions (no data engine command)
        subscriptions = self.msgbus.subscriptions(f"events.fills.{AUDUSD_SIM.id}")
        assert len(subscriptions) == 1

    def test_unsubscribe_order_fills(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        actor.subscribe_order_fills(AUDUSD_SIM.id)

        # Act
        actor.unsubscribe_order_fills(AUDUSD_SIM.id)

        # Assert
        subscriptions = self.msgbus.subscriptions(f"events.fills.{AUDUSD_SIM.id}")
        assert len(subscriptions) == 0

    def test_handle_order_filled_when_not_running_does_not_send_to_on_order_filled(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        actor.subscribe_order_fills(AUDUSD_SIM.id)

        order = TestExecStubs.market_order()
        fill = TestEventStubs.order_filled(order, AUDUSD_SIM)

        # Act
        self.msgbus.publish(
            topic=f"events.fills.{AUDUSD_SIM.id}",
            msg=fill,
        )

        # Assert
        assert actor.calls == []
        assert actor.store == []

    def test_handle_order_filled_when_running_sends_to_on_order_filled(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        actor.subscribe_order_fills(AUDUSD_SIM.id)
        actor.start()

        order = TestExecStubs.market_order()
        fill = TestEventStubs.order_filled(order, AUDUSD_SIM)

        # Act
        self.msgbus.publish(
            topic=f"events.fills.{AUDUSD_SIM.id}",
            msg=fill,
        )

        # Assert
        assert actor.calls == ["on_start", "on_order_filled"]
        assert actor.store[0] == fill

    def assert_successful_request(self, actor, request_id, method_name):
        """
        Do assert the request is successful.

        Helper method for common request assertions.

        """
        method_info = f" for method '{method_name}'"
        assert request_id is not None, f"Request ID should not be None{method_info}"
        assert (
            self.data_engine.request_count == 1
        ), f"Expected 1 request in data engine{method_info}, was {self.data_engine.request_count}"
        assert (
            not actor.has_pending_requests()
        ), f"Actor should not have pending requests{method_info}"
        assert not actor.is_pending_request(
            request_id,
        ), f"Request {request_id} should not be pending{method_info}"
        assert (
            request_id not in actor.pending_requests()
        ), f"Request {request_id} should not be in pending requests list{method_info}"

    def test_request_data_sends_request_to_data_engine(self) -> None:
        # Arrange
        handler: list[NewsEvent] = []
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        data_type = DataType(NewsEvent, {"type": "NEWS_WIRE", "topic": "Earthquakes"})

        # Act
        start_time = self.clock.utc_now() - timedelta(hours=1)
        end_time = self.clock.utc_now()
        request_id = actor.request_data(
            data_type,
            ClientId("BLOOMBERG-01"),
            start=start_time,
            end=end_time,
            callback=handler.append,
        )

        # Assert
        self.assert_successful_request(actor, request_id, "request_data")

    def test_request_order_book_snapshots_sends_request_to_data_engine(self) -> None:
        # Arrange
        handler: list[OrderBookDeltas] = []
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        data_type = DataType(
            OrderBookDeltas,
            metadata={
                "instrument_id": AUDUSD_SIM.id,
                "limit": 5,
            },
        )

        # Act
        start_time = self.clock.utc_now() - timedelta(hours=1)
        end_time = self.clock.utc_now()
        request_id = actor.request_data(
            data_type,
            ClientId("BLOOMBERG-01"),
            start=start_time,
            end=end_time,
            callback=handler.append,
        )

        # Assert
        self.assert_successful_request(actor, request_id, "request_data")

    def test_request_quote_ticks_sends_request_to_data_engine(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act
        start_time = self.clock.utc_now() - timedelta(hours=1)
        end_time = self.clock.utc_now()
        request_id = actor.request_quote_ticks(AUDUSD_SIM.id, start=start_time, end=end_time)

        # Assert
        self.assert_successful_request(actor, request_id, "request_quote_ticks")

    def test_request_quote_ticks_with_registered_callback(self) -> None:
        # Arrange
        handler: list[QuoteTick] = []
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        tick = TestDataStubs.quote_tick()

        # Act
        start_time = self.clock.utc_now() - timedelta(hours=1)
        end_time = self.clock.utc_now()
        request_id = actor.request_quote_ticks(
            AUDUSD_SIM.id,
            start=start_time,
            end=end_time,
            callback=handler.append,
        )

        response = DataResponse(
            client_id=ClientId("SIM"),
            venue=Venue("SIM"),
            data_type=DataType(QuoteTick),
            data=[tick],
            correlation_id=request_id,
            response_id=UUID4(),
            start=start_time,
            end=end_time,
            ts_init=self.clock.timestamp_ns(),
        )

        self.msgbus.response(response)

        # Assert
        self.assert_successful_request(actor, request_id, "request_quote_ticks")
        assert request_id in handler

    def test_request_trade_ticks_sends_request_to_data_engine(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act
        start_time = self.clock.utc_now() - timedelta(hours=1)
        end_time = self.clock.utc_now()
        request_id = actor.request_trade_ticks(AUDUSD_SIM.id, start=start_time, end=end_time)

        # Assert
        self.assert_successful_request(actor, request_id, "request_trade_ticks")

    def test_request_trade_ticks_with_registered_callback(self) -> None:
        # Arrange
        handler: list[TradeTick] = []
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        tick = TestDataStubs.trade_tick()

        # Act
        start_time = self.clock.utc_now() - timedelta(hours=1)
        end_time = self.clock.utc_now()
        request_id = actor.request_trade_ticks(
            AUDUSD_SIM.id,
            start=start_time,
            end=end_time,
            callback=handler.append,
        )

        response = DataResponse(
            client_id=ClientId("SIM"),
            venue=Venue("SIM"),
            data_type=DataType(TradeTick),
            data=[tick],
            correlation_id=request_id,
            response_id=UUID4(),
            start=start_time,
            end=end_time,
            ts_init=self.clock.timestamp_ns(),
        )

        self.msgbus.response(response)
        # Assert
        self.assert_successful_request(actor, request_id, "request_trade_ticks")
        assert request_id in handler

    def test_request_bars_sends_request_to_data_engine(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        bar_type = TestDataStubs.bartype_audusd_1min_bid()

        # Act
        start_time = self.clock.utc_now() - timedelta(hours=1)
        end_time = self.clock.utc_now()
        request_id = actor.request_bars(bar_type, start=start_time, end=end_time)

        # Assert
        self.assert_successful_request(actor, request_id, "request_bars")

    def test_request_order_book_depth_sends_request_to_data_engine(self) -> None:
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act
        start_time = self.clock.utc_now() - timedelta(hours=1)
        end_time = self.clock.utc_now()
        request_id = actor.request_order_book_depth(
            AUDUSD_SIM.id,
            depth=10,
            start=start_time,
            end=end_time,
        )

        # Assert
        self.assert_successful_request(actor, request_id, "request_order_book_depth")

    def test_request_bars_with_registered_callback(self) -> None:
        # Arrange
        handler: list[Bar] = []
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        bar_type = TestDataStubs.bartype_audusd_1min_bid()
        bar = TestDataStubs.bar_5decimal()

        # Act
        start_time = self.clock.utc_now() - timedelta(hours=1)
        end_time = self.clock.utc_now()
        request_id = actor.request_bars(
            bar_type,
            start=start_time,
            end=end_time,
            callback=handler.append,
        )

        response = DataResponse(
            client_id=ClientId("SIM"),
            venue=Venue("SIM"),
            data_type=DataType(Bar),
            data=[bar],
            correlation_id=request_id,
            response_id=UUID4(),
            start=start_time,
            end=end_time,
            ts_init=self.clock.timestamp_ns(),
        )

        self.msgbus.response(response)

        # Assert
        self.assert_successful_request(actor, request_id, "request_bars")
        assert request_id in handler

    def test_request_aggregated_bars_with_registered_callback(self) -> None:
        # Arrange
        handler: list[Bar] = []
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        bar_type = BarType.from_str("AUDUSD.SIM-1-MINUTE-BID-INTERNAL")
        bar = TestDataStubs.bar_5decimal()

        # Act
        start = self.clock.utc_now() - timedelta(hours=1)
        end = self.clock.utc_now()
        request_id = actor.request_aggregated_bars(
            [bar_type],
            start=start,
            end=end,
            callback=handler.append,
        )

        response = DataResponse(
            client_id=ClientId("SIM"),
            venue=Venue("SIM"),
            data_type=DataType(Bar),
            data={"bars": {bar_type: [bar]}, "quote_ticks": [], "trade_ticks": []},
            correlation_id=request_id,
            response_id=UUID4(),
            start=start,
            end=end,
            ts_init=self.clock.timestamp_ns(),
        )

        self.msgbus.response(response)

        # Assert
        self.assert_successful_request(actor, request_id, "request_aggregated_bars")
        assert request_id in handler

    @pytest.mark.parametrize(
        ("start", "stop"),
        [
            (UNIX_EPOCH + timedelta(milliseconds=1), UNIX_EPOCH),
        ],
    )
    def test_request_bars_with_invalid_params_raises_value_error(self, start, stop):
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        bar_type = TestDataStubs.bartype_audusd_1min_bid()

        # Act, Assert
        with pytest.raises(ValueError):
            actor.request_bars(bar_type, start, stop)

    # Parameter definitions for tests
    REQUEST_METHODS_INSTRUMENT = [
        ("request_instrument", {"instrument_id": AUDUSD_SIM.id}),
        ("request_instruments", {"venue": AUDUSD_SIM.id.venue}),
    ]

    REQUEST_METHODS_WITHOUT_NONE_START_HANDLING = [
        (
            "request_data",
            {
                "data_type": DataType(NewsEvent, {"type": "NEWS_WIRE", "topic": "Earthquake"}),
                "client_id": ClientId("SIM"),
            },
        ),
        ("request_quote_ticks", {"instrument_id": AUDUSD_SIM.id}),
        ("request_trade_ticks", {"instrument_id": AUDUSD_SIM.id}),
        ("request_order_book_depth", {"instrument_id": AUDUSD_SIM.id, "depth": 10}),
        ("request_bars", {"bar_type": TestDataStubs.bartype_audusd_1min_bid()}),
        ("request_aggregated_bars", {"bar_types": [_create_composite_bar_type()]}),
    ]

    # Derived from combining the above two lists
    REQUEST_METHODS_ALL = REQUEST_METHODS_INSTRUMENT + REQUEST_METHODS_WITHOUT_NONE_START_HANDLING

    @pytest.mark.parametrize("method_name,args", REQUEST_METHODS_INSTRUMENT)
    def test_none_start_and_end_replaced_with_current_time(self, method_name, args):
        """
        Test that None start and end values are replaced with current time.
        """
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        request_method = getattr(actor, method_name)

        # Act
        request_id = request_method(**args, start=None, end=None)

        # Assert
        self.assert_successful_request(actor, request_id, method_name)

    @pytest.mark.parametrize("method_name,args", REQUEST_METHODS_INSTRUMENT)
    def test_none_start_replaced_with_current_time(self, method_name, args):
        """
        Test that None start value is replaced with current time.
        """
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        request_method = getattr(actor, method_name)

        # Act
        current_time = self.clock.utc_now()
        request_id = request_method(**args, start=None, end=current_time)

        # Assert
        self.assert_successful_request(actor, request_id, method_name)

    @pytest.mark.parametrize("method_name,args", REQUEST_METHODS_INSTRUMENT)
    def test_none_end_replaced_with_current_time(self, method_name, args):
        """
        Test that None end value is replaced with current time.
        """
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        specific_start_time = self.clock.utc_now() - timedelta(hours=1)
        request_method = getattr(actor, method_name)

        # Act
        request_id = request_method(**args, start=specific_start_time, end=None)

        # Assert
        self.assert_successful_request(actor, request_id, method_name)

    @pytest.mark.parametrize(
        "request_method_name,method_args",
        REQUEST_METHODS_WITHOUT_NONE_START_HANDLING,
    )
    def test_start_parameter_none_causes_failure(self, request_method_name, method_args):
        """
        Test that None start parameter causes failure for non-instrument request
        methods.
        """
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        request_method = getattr(actor, request_method_name)
        end_time = self.clock.utc_now() - timedelta(hours=1)

        # Act & Assert
        with pytest.raises(TypeError, match=r"'start' argument was `None`"):
            request_method(**method_args, start=None, end=end_time)

    @pytest.mark.parametrize("request_method_name,method_args", REQUEST_METHODS_ALL)
    def test_start_parameter_future_time_causes_failure(self, request_method_name, method_args):
        """
        Test that future start times cause failure for all request methods.
        """
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        future_start = self.clock.utc_now() + timedelta(hours=1)
        end_time = self.clock.utc_now() - timedelta(hours=1)
        request_method = getattr(actor, request_method_name)

        # Act & Assert
        with pytest.raises(ValueError, match="start was > now"):
            request_method(**method_args, start=future_start, end=end_time)

    @pytest.mark.parametrize("request_method_name,method_args", REQUEST_METHODS_ALL)
    def test_end_parameter_none_succeeds(self, request_method_name, method_args):
        """
        Test that end=None succeeds for all request methods (replaces with current
        time).
        """
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        start_time = self.clock.utc_now() - timedelta(hours=1)
        request_method = getattr(actor, request_method_name)

        # Act
        request_id = request_method(**method_args, start=start_time, end=None)
        # Assert
        self.assert_successful_request(actor, request_id, request_method_name)

    @pytest.mark.parametrize("request_method_name,method_args", REQUEST_METHODS_ALL)
    def test_end_parameter_future_time_causes_failure(self, request_method_name, method_args):
        """
        Test that future end times cause failure for all request methods.
        """
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        start_time = self.clock.utc_now() - timedelta(hours=1)
        future_end = self.clock.utc_now() + timedelta(hours=1)
        request_method = getattr(actor, request_method_name)

        # Act & Assert
        with pytest.raises(ValueError, match="end was > now"):
            request_method(**method_args, start=start_time, end=future_end)

    @pytest.mark.parametrize("request_method_name,method_args", REQUEST_METHODS_ALL)
    def test_start_after_end_causes_failure(self, request_method_name, method_args):
        """
        Test that start > end causes failure for all request methods.
        """
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        # Create start time after end time, both in the past
        end_time = self.clock.utc_now() - timedelta(hours=2)
        start_time = self.clock.utc_now() - timedelta(hours=1)
        request_method = getattr(actor, request_method_name)

        # Act & Assert
        with pytest.raises(ValueError, match="start was > end"):
            request_method(**method_args, start=start_time, end=end_time)

    @pytest.mark.parametrize("request_method_name,method_args", REQUEST_METHODS_ALL)
    def test_start_equals_end_succeeds(self, request_method_name, method_args):
        """
        Test that start == end succeeds for all request methods.
        """
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        # Create same time for start and end, in the past
        same_time = self.clock.utc_now() - timedelta(hours=1)
        request_method = getattr(actor, request_method_name)

        # Act
        request_id = request_method(**method_args, start=same_time, end=same_time)
        # Assert
        self.assert_successful_request(actor, request_id, request_method_name)

    # Additional start/stop relationship validation test
    @pytest.mark.parametrize("request_method_name,method_args", REQUEST_METHODS_ALL)
    def test_start_before_stop_succeeds(self, request_method_name, method_args):
        """
        Test that start < stop succeeds for all request methods.
        """
        # Arrange
        actor = MockActor()
        actor.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        # Create start time before stop time, both in the past
        start_time = self.clock.utc_now() - timedelta(hours=2)
        stop_time = self.clock.utc_now() - timedelta(hours=1)
        request_method = getattr(actor, request_method_name)

        # Act
        request_id = request_method(**method_args, start=start_time, end=stop_time)
        # Assert
        self.assert_successful_request(actor, request_id, request_method_name)
