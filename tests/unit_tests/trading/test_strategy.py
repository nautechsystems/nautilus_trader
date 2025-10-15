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

from datetime import datetime
from datetime import timedelta
from decimal import Decimal

import pandas as pd
import pytest
import pytz

from nautilus_trader.backtest.data_client import BacktestMarketDataClient
from nautilus_trader.backtest.engine import SimulatedExchange
from nautilus_trader.backtest.execution_client import BacktestExecClient
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.backtest.models import LatencyModel
from nautilus_trader.backtest.models import MakerTakerFeeModel
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.component import TestClock
from nautilus_trader.common.enums import ComponentState
from nautilus_trader.config import ImportableStrategyConfig
from nautilus_trader.config import StrategyConfig
from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.indicators import ExponentialMovingAverage
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.data import Bar
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import ContingencyType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import MarketOrder
from nautilus_trader.model.orders import OrderList
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.risk.engine import RiskEngine
from nautilus_trader.test_kit.mocks.strategies import KaboomStrategy
from nautilus_trader.test_kit.mocks.strategies import MockStrategy
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.data import UNIX_EPOCH
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from nautilus_trader.trading.strategy import Strategy


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
GBPUSD_SIM = TestInstrumentProvider.default_fx_ccy("GBP/USD")
_USDJPY_SIM = TestInstrumentProvider.default_fx_ccy("USD/JPY")


class TestStrategy:
    def setup(self) -> None:
        # Fixture Setup
        self.clock = TestClock()
        self.trader_id = TestIdStubs.trader_id()

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

        self.risk_engine = RiskEngine(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.exchange = SimulatedExchange(
            venue=Venue("SIM"),
            oms_type=OmsType.HEDGING,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            starting_balances=[Money(1_000_000, USD)],
            default_leverage=Decimal(50),
            leverages={},
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            modules=[],
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            clock=self.clock,
            latency_model=LatencyModel(0),
            support_contingent_orders=False,
            use_reduce_only=False,
        )
        self.exchange.add_instrument(_USDJPY_SIM)

        self.data_client = BacktestMarketDataClient(
            client_id=ClientId("SIM"),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.exec_client = BacktestExecClient(
            exchange=self.exchange,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Wire up components
        self.exchange.register_client(self.exec_client)
        self.data_engine.register_client(self.data_client)
        self.exec_engine.register_client(self.exec_client)
        self.exchange.reset()

        # Add instruments
        self.data_engine.process(AUDUSD_SIM)
        self.data_engine.process(GBPUSD_SIM)
        self.data_engine.process(_USDJPY_SIM)
        self.cache.add_instrument(AUDUSD_SIM)
        self.cache.add_instrument(GBPUSD_SIM)
        self.cache.add_instrument(_USDJPY_SIM)

        # Prepare market
        self.exchange.process_quote_tick(
            TestDataStubs.quote_tick(
                instrument=_USDJPY_SIM,
                bid_price=90.001,
                ask_price=90.002,
            ),
        )

        self.data_engine.start()
        self.exec_engine.start()

    def test_strategy_to_importable_config_with_no_specific_config(self) -> None:
        # Arrange
        config = StrategyConfig()

        strategy = Strategy(config=config)

        # Act
        result = strategy.to_importable_config()

        # Assert
        assert isinstance(result, ImportableStrategyConfig)
        assert result.strategy_path == "nautilus_trader.trading.strategy:Strategy"
        assert result.config_path == "nautilus_trader.trading.config:StrategyConfig"
        assert result.config == {
            "strategy_id": None,
            "order_id_tag": None,
            "use_uuid_client_order_ids": False,
            "use_hyphens_in_client_order_ids": True,
            "oms_type": None,
            "external_order_claims": None,
            "manage_contingent_orders": False,
            "manage_gtd_expiry": False,
            "log_events": True,
            "log_commands": True,
            "log_rejected_due_post_only_as_warning": True,
        }

    def test_strategy_to_importable_config(self) -> None:
        # Arrange
        config = StrategyConfig(
            strategy_id="ALPHA-01",
            order_id_tag="001",
            external_order_claims=["ETHUSDT-PERP.DYDX"],
            manage_contingent_orders=True,
            manage_gtd_expiry=True,
            log_events=False,
        )

        strategy = Strategy(config=config)

        # Act
        result = strategy.to_importable_config()

        # Assert
        assert isinstance(result, ImportableStrategyConfig)
        assert result.strategy_path == "nautilus_trader.trading.strategy:Strategy"
        assert result.config_path == "nautilus_trader.trading.config:StrategyConfig"
        assert result.config == {
            "strategy_id": "ALPHA-01",
            "order_id_tag": "001",
            "use_uuid_client_order_ids": False,
            "use_hyphens_in_client_order_ids": True,
            "oms_type": None,
            "external_order_claims": ["ETHUSDT-PERP.DYDX"],
            "manage_contingent_orders": True,
            "manage_gtd_expiry": True,
            "log_events": False,
            "log_commands": True,
            "log_rejected_due_post_only_as_warning": True,
        }

    def test_strategy_equality(self) -> None:
        # Arrange
        strategy1 = Strategy(config=StrategyConfig(order_id_tag="AUD/USD-001"))
        strategy2 = Strategy(config=StrategyConfig(order_id_tag="AUD/USD-001"))
        strategy3 = Strategy(config=StrategyConfig(order_id_tag="AUD/USD-002"))

        # Act, Assert
        assert strategy1 == strategy1
        assert strategy1 == strategy2
        assert strategy2 != strategy3

    def test_str_and_repr(self) -> None:
        # Arrange
        strategy = Strategy(config=StrategyConfig(order_id_tag="GBP/USD-MM"))

        # Act, Assert
        assert str(strategy) == "Strategy-GBP/USD-MM"
        assert repr(strategy) == "Strategy(Strategy-GBP/USD-MM)"

    def test_id(self) -> None:
        # Arrange
        strategy = Strategy()

        # Act, Assert
        assert strategy.id == StrategyId("Strategy-None")

    def test_initialization(self) -> None:
        # Arrange
        strategy = Strategy(config=StrategyConfig(order_id_tag="001"))
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act, Assert
        assert strategy.state == ComponentState.READY
        assert not strategy.indicators_initialized()

    def test_on_save_when_not_overridden_does_nothing(self) -> None:
        # Arrange
        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act
        strategy.on_save()

        # Assert
        assert True  # Exception not raised

    def test_on_load_when_not_overridden_does_nothing(self) -> None:
        # Arrange
        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act
        strategy.on_load({})

        # Assert
        assert True  # Exception not raised

    def test_save_when_not_registered_logs_error(self) -> None:
        # Arrange
        config = StrategyConfig()

        strategy = Strategy(config)
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        strategy.save()

        # Assert
        assert True  # Exception not raised

    def test_save_when_user_code_raises_error_logs_and_reraises(self) -> None:
        # Arrange
        strategy = KaboomStrategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act, Assert
        with pytest.raises(RuntimeError):
            strategy.save()

    def test_load_when_user_code_raises_error_logs_and_reraises(self) -> None:
        # Arrange
        strategy = KaboomStrategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act, Assert
        with pytest.raises(RuntimeError):
            strategy.load({"something": b"123456"})

    def test_load(self) -> None:
        # Arrange
        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        state: dict[str, bytes] = {}

        # Act
        strategy.load(state)

        # Assert
        # TODO: Write a users custom save method
        assert True

    def test_reset(self) -> None:
        # Arrange
        bar_type = TestDataStubs.bartype_audusd_1min_bid()
        strategy = MockStrategy(bar_type)
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        bar = Bar(
            bar_type,
            Price.from_str("1.00001"),
            Price.from_str("1.00004"),
            Price.from_str("1.00000"),
            Price.from_str("1.00003"),
            Quantity.from_int(100_000),
            0,
            0,
        )

        strategy.handle_bar(bar)

        # Act
        strategy.reset()

        # Assert
        assert "on_reset" in strategy.calls
        assert strategy.is_initialized
        assert strategy.ema1.count == 0
        assert strategy.ema2.count == 0

    def test_dispose(self) -> None:
        # Arrange
        bar_type = TestDataStubs.bartype_audusd_1min_bid()
        strategy = MockStrategy(bar_type)
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        strategy.reset()

        # Act
        strategy.dispose()

        # Assert
        assert "on_dispose" in strategy.calls
        assert strategy.is_disposed

    def test_dispose_cancels_all_timers(self) -> None:
        # Arrange
        bar_type = TestDataStubs.bartype_audusd_1min_bid()
        strategy = MockStrategy(bar_type)
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        start_time = datetime.now(pytz.utc) + timedelta(milliseconds=100)
        strategy.clock.set_timer(
            "test_timer1",
            timedelta(milliseconds=100),
            start_time,
            stop_time=None,
        )
        strategy.clock.set_timer(
            "test_timer2",
            timedelta(milliseconds=200),
            start_time,
            stop_time=None,
        )

        # Act
        strategy.dispose()

        # Assert
        assert strategy.clock.timer_count == 0
        assert strategy.is_disposed

    def test_save_load(self) -> None:
        # Arrange
        bar_type = TestDataStubs.bartype_audusd_1min_bid()
        strategy = MockStrategy(bar_type)
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Act
        state = strategy.save()
        strategy.load(state)

        # Assert
        assert state == {"UserState": b"1"}
        assert "on_save" in strategy.calls
        assert strategy.is_initialized

    def test_register_indicator_for_quote_ticks_when_already_registered(self) -> None:
        # Arrange
        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        ema1 = ExponentialMovingAverage(10, price_type=PriceType.MID)
        ema2 = ExponentialMovingAverage(10, price_type=PriceType.MID)

        # Act
        strategy.register_indicator_for_quote_ticks(AUDUSD_SIM.id, ema1)
        strategy.register_indicator_for_quote_ticks(AUDUSD_SIM.id, ema2)
        strategy.register_indicator_for_quote_ticks(AUDUSD_SIM.id, ema2)

        assert len(strategy.registered_indicators) == 2
        assert ema1 in strategy.registered_indicators
        assert ema2 in strategy.registered_indicators

    def test_register_indicator_for_trade_ticks_when_already_registered(self) -> None:
        # Arrange
        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        ema1 = ExponentialMovingAverage(10)
        ema2 = ExponentialMovingAverage(10)

        # Act
        strategy.register_indicator_for_trade_ticks(AUDUSD_SIM.id, ema1)
        strategy.register_indicator_for_trade_ticks(AUDUSD_SIM.id, ema2)
        strategy.register_indicator_for_trade_ticks(AUDUSD_SIM.id, ema2)

        assert len(strategy.registered_indicators) == 2
        assert ema1 in strategy.registered_indicators
        assert ema2 in strategy.registered_indicators

    def test_register_indicator_for_bars_when_already_registered(self) -> None:
        # Arrange
        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        ema1 = ExponentialMovingAverage(10)
        ema2 = ExponentialMovingAverage(10)
        bar_type = TestDataStubs.bartype_audusd_1min_bid()

        # Act
        strategy.register_indicator_for_bars(bar_type, ema1)
        strategy.register_indicator_for_bars(bar_type, ema2)
        strategy.register_indicator_for_bars(bar_type, ema2)  # Logs error

        assert len(strategy.registered_indicators) == 2
        assert ema1 in strategy.registered_indicators
        assert ema2 in strategy.registered_indicators

    def test_register_indicator_for_multiple_data_sources(self) -> None:
        # Arrange
        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        ema = ExponentialMovingAverage(10)
        bar_type = TestDataStubs.bartype_audusd_1min_bid()

        # Act
        strategy.register_indicator_for_quote_ticks(AUDUSD_SIM.id, ema)
        strategy.register_indicator_for_quote_ticks(GBPUSD_SIM.id, ema)
        strategy.register_indicator_for_trade_ticks(AUDUSD_SIM.id, ema)
        strategy.register_indicator_for_bars(bar_type, ema)

        assert len(strategy.registered_indicators) == 1
        assert ema in strategy.registered_indicators

    def test_handle_quote_tick_updates_indicator_registered_for_quote_ticks(self) -> None:
        # Arrange
        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        ema = ExponentialMovingAverage(10, price_type=PriceType.MID)
        strategy.register_indicator_for_quote_ticks(AUDUSD_SIM.id, ema)

        tick = TestDataStubs.quote_tick(AUDUSD_SIM)

        # Act
        strategy.handle_quote_tick(tick)
        strategy.handle_quote_tick(tick)

        # Assert
        assert ema.count == 2

    def test_handle_quote_ticks_with_no_ticks_logs_and_continues(self) -> None:
        # Arrange
        strategy = KaboomStrategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        ema = ExponentialMovingAverage(10, price_type=PriceType.MID)
        strategy.register_indicator_for_quote_ticks(AUDUSD_SIM.id, ema)

        # Act
        strategy.handle_quote_ticks([])

        # Assert
        assert ema.count == 0

    def test_handle_quote_ticks_updates_indicator_registered_for_quote_ticks(self) -> None:
        # Arrange
        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        ema = ExponentialMovingAverage(10, price_type=PriceType.MID)
        strategy.register_indicator_for_quote_ticks(AUDUSD_SIM.id, ema)

        tick = TestDataStubs.quote_tick(AUDUSD_SIM)

        # Act
        strategy.handle_quote_ticks([tick])

        # Assert
        assert ema.count == 1

    def test_handle_trade_tick_updates_indicator_registered_for_trade_ticks(self) -> None:
        # Arrange
        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        ema = ExponentialMovingAverage(10)
        strategy.register_indicator_for_trade_ticks(AUDUSD_SIM.id, ema)

        tick = TestDataStubs.trade_tick(AUDUSD_SIM)

        # Act
        strategy.handle_trade_tick(tick)
        strategy.handle_trade_tick(tick)

        # Assert
        assert ema.count == 2

    def test_handle_trade_ticks_updates_indicator_registered_for_trade_ticks(self) -> None:
        # Arrange
        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        ema = ExponentialMovingAverage(10)
        strategy.register_indicator_for_trade_ticks(AUDUSD_SIM.id, ema)

        tick = TestDataStubs.trade_tick(AUDUSD_SIM)

        # Act
        strategy.handle_trade_ticks([tick])

        # Assert
        assert ema.count == 1

    def test_handle_trade_ticks_with_no_ticks_logs_and_continues(self) -> None:
        # Arrange
        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        ema = ExponentialMovingAverage(10)
        strategy.register_indicator_for_trade_ticks(AUDUSD_SIM.id, ema)

        # Act
        strategy.handle_trade_ticks([])

        # Assert
        assert ema.count == 0

    def test_handle_bar_updates_indicator_registered_for_bars(self) -> None:
        # Arrange
        bar_type = TestDataStubs.bartype_audusd_1min_bid()
        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        ema = ExponentialMovingAverage(10)
        strategy.register_indicator_for_bars(bar_type, ema)
        bar = TestDataStubs.bar_5decimal()

        # Act
        strategy.handle_bar(bar)
        strategy.handle_bar(bar)

        # Assert
        assert ema.count == 2

    def test_handle_bars_updates_indicator_registered_for_bars(self) -> None:
        # Arrange
        bar_type = TestDataStubs.bartype_audusd_1min_bid()
        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        ema = ExponentialMovingAverage(10)
        strategy.register_indicator_for_bars(bar_type, ema)
        bar = TestDataStubs.bar_5decimal()

        # Act
        strategy.handle_bars([bar])

        # Assert
        assert ema.count == 1

    def test_handle_bars_with_no_bars_logs_and_continues(self) -> None:
        # Arrange
        bar_type = TestDataStubs.bartype_gbpusd_1sec_mid()
        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        ema = ExponentialMovingAverage(10)
        strategy.register_indicator_for_bars(bar_type, ema)

        # Act
        strategy.handle_bars([])

        # Assert
        assert ema.count == 0

    def test_stop_cancels_a_running_time_alert(self) -> None:
        # Arrange
        bar_type = TestDataStubs.bartype_audusd_1min_bid()
        strategy = MockStrategy(bar_type)
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        alert_time = datetime.now(pytz.utc) + timedelta(milliseconds=200)
        strategy.clock.set_time_alert("test_alert1", alert_time)

        # Act
        strategy.start()
        strategy.stop()

        # Assert
        assert strategy.clock.timer_count == 0

    def test_stop_cancels_a_running_timer(self) -> None:
        # Arrange
        bar_type = TestDataStubs.bartype_audusd_1min_bid()
        strategy = MockStrategy(bar_type)
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        start_time = datetime.now(pytz.utc) + timedelta(milliseconds=100)
        strategy.clock.set_timer(
            "test_timer",
            timedelta(milliseconds=100),
            start_time,
            stop_time=None,
        )

        # Act
        strategy.start()
        strategy.stop()

        # Assert
        assert strategy.clock.timer_count == 0

    def test_start_when_manage_gtd_reactivates_timers(self) -> None:
        # Arrange
        config = StrategyConfig(manage_gtd_expiry=True)
        strategy = Strategy(config)
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order1 = strategy.order_factory.limit(
            _USDJPY_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
            _USDJPY_SIM.make_price(100.000),
            time_in_force=TimeInForce.GTD,
            expire_time=self.clock.utc_now() + pd.Timedelta(minutes=10),
        )
        order2 = strategy.order_factory.limit(
            _USDJPY_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
            _USDJPY_SIM.make_price(101.000),
            time_in_force=TimeInForce.GTD,
            expire_time=self.clock.utc_now() + pd.Timedelta(minutes=11),
        )

        strategy.submit_order(order1)
        strategy.submit_order(order2)
        self.exchange.process(0)

        # Act
        strategy.clock.cancel_timers()  # <-- Simulate restart
        strategy.start()

        # Assert
        assert strategy.clock.timer_count == 2
        assert strategy.clock.timer_names == [
            "GTD-EXPIRY:O-19700101-000000-000-None-1",
            "GTD-EXPIRY:O-19700101-000000-000-None-2",
        ]

    def test_start_when_manage_gtd_and_order_past_expiration_then_cancels(self) -> None:
        # Arrange
        config = StrategyConfig(manage_gtd_expiry=True)
        strategy = Strategy(config)
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order1 = strategy.order_factory.limit(
            _USDJPY_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
            _USDJPY_SIM.make_price(100.000),
            time_in_force=TimeInForce.GTD,
            expire_time=self.clock.utc_now() + pd.Timedelta(minutes=10),
        )

        strategy.submit_order(order1)
        self.exchange.process(0)

        strategy.clock.cancel_timers()  # <-- Simulate restart
        self.clock.set_time(dt_to_unix_nanos(order1.expire_time + pd.Timedelta(minutes=1)))

        # Act
        strategy.start()

        # Assert
        assert strategy.clock.timer_count == 0
        assert order1.is_pending_cancel

    def test_submit_order_when_duplicate_id_then_denies(self) -> None:
        # Arrange
        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order1 = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        order2 = MarketOrder(
            self.trader_id,
            strategy.id,
            AUDUSD_SIM.id,
            order1.client_order_id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            UUID4(),
            0,
            TimeInForce.DAY,
        )
        strategy.submit_order(order1)

        # Act
        strategy.submit_order(order2)

        # Assert
        assert order2.status == OrderStatus.DENIED

    def test_submit_order_with_valid_order_successfully_submits(self) -> None:
        # Arrange
        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order = strategy.order_factory.market(
            _USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        # Act
        strategy.submit_order(order)
        self.exchange.process(0)

        # Assert
        assert order in strategy.cache.orders()
        assert strategy.cache.orders()[0].status == OrderStatus.FILLED
        assert order.client_order_id not in strategy.cache.orders_open()
        assert not strategy.cache.is_order_open(order.client_order_id)
        assert strategy.cache.is_order_closed(order.client_order_id)

    def test_submit_order_with_managed_gtd_starts_timer(self) -> None:
        # Arrange
        config = StrategyConfig(manage_gtd_expiry=True)
        strategy = Strategy(config)
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order = strategy.order_factory.limit(
            _USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            price=Price.from_str("100.000"),
            time_in_force=TimeInForce.GTD,
            expire_time=UNIX_EPOCH + timedelta(minutes=1),
        )

        # Act
        strategy.submit_order(order)

        # Assert
        assert strategy.clock.timer_count == 1
        assert strategy.clock.timer_names == ["GTD-EXPIRY:O-19700101-000000-000-None-1"]

    def test_submit_order_with_managed_gtd_when_immediately_filled_cancels_timer(self) -> None:
        # Arrange
        config = StrategyConfig(manage_gtd_expiry=True)
        strategy = Strategy(config)
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order = strategy.order_factory.limit(
            _USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            price=Price.from_str("100.000"),
            time_in_force=TimeInForce.GTD,
            expire_time=UNIX_EPOCH + timedelta(minutes=1),
        )

        # Act
        strategy.submit_order(order)
        self.exchange.process(0)

        # Assert
        assert strategy.clock.timer_count == 0
        assert order.status == OrderStatus.FILLED

    def test_submit_order_list_with_duplicate_order_list_id_then_denies(self) -> None:
        # Arrange
        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        bracket1 = strategy.order_factory.bracket(
            instrument_id=AUDUSD_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(100_000),
            sl_trigger_price=Price.from_str("1.00000"),
            tp_price=Price.from_str("1.00100"),
            emulation_trigger=TriggerType.BID_ASK,
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

        bracket2 = OrderList(
            order_list_id=bracket1.id,
            orders=[entry, stop_loss, take_profit],
        )

        strategy.submit_order_list(bracket1)

        # Act
        strategy.submit_order_list(bracket2)

        # Assert
        assert self.cache.order_exists(entry.client_order_id)
        assert self.cache.order_exists(stop_loss.client_order_id)
        assert self.cache.order_exists(take_profit.client_order_id)
        assert self.cache.order_list_exists(bracket1.id)
        assert self.cache.order_list_exists(bracket2.id)
        assert bracket1.orders[0].status == OrderStatus.INITIALIZED
        assert bracket1.orders[1].status == OrderStatus.INITIALIZED
        assert bracket1.orders[2].status == OrderStatus.INITIALIZED
        assert entry.status == OrderStatus.DENIED
        assert stop_loss.status == OrderStatus.DENIED
        assert take_profit.status == OrderStatus.DENIED

    def test_submit_order_list_with_duplicate_order_id_then_denies(self) -> None:
        # Arrange
        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        bracket1 = strategy.order_factory.bracket(
            instrument_id=AUDUSD_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(100_000),
            sl_trigger_price=Price.from_str("1.00000"),
            tp_price=Price.from_str("1.00100"),
            emulation_trigger=TriggerType.BID_ASK,
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

        bracket2 = OrderList(
            order_list_id=strategy.order_factory.generate_order_list_id(),
            orders=[bracket1.orders[0], stop_loss, take_profit],
        )

        strategy.submit_order_list(bracket1)

        # Act
        strategy.submit_order_list(bracket2)

        # Assert
        assert self.cache.order_exists(bracket1.orders[0].client_order_id)
        assert self.cache.order_exists(bracket1.orders[1].client_order_id)
        assert self.cache.order_exists(bracket1.orders[2].client_order_id)
        assert self.cache.order_exists(stop_loss.client_order_id)
        assert self.cache.order_exists(take_profit.client_order_id)
        assert self.cache.order_list_exists(bracket1.id)
        assert self.cache.order_list_exists(bracket2.id)
        assert bracket1.orders[0].status == OrderStatus.DENIED
        assert stop_loss.status == OrderStatus.DENIED
        assert take_profit.status == OrderStatus.DENIED

    def test_submit_order_list_with_valid_order_successfully_submits(self) -> None:
        # Arrange
        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        bracket = strategy.order_factory.bracket(
            _USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            entry_price=Price.from_str("80.000"),
            sl_trigger_price=Price.from_str("90.000"),
            tp_price=Price.from_str("90.500"),
            entry_order_type=OrderType.LIMIT,
        )

        # Act
        strategy.submit_order_list(bracket)
        self.exchange.process(0)

        # Assert
        entry = bracket.first
        assert bracket.orders[0] in strategy.cache.orders()
        assert bracket.orders[1] in strategy.cache.orders()
        assert bracket.orders[2] in strategy.cache.orders()
        assert entry.status == OrderStatus.ACCEPTED
        assert entry in strategy.cache.orders_open()

    def test_submit_order_list_with_managed_gtd_starts_timer(self) -> None:
        # Arrange
        config = StrategyConfig(manage_gtd_expiry=True)
        strategy = Strategy(config)
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        bracket = strategy.order_factory.bracket(
            _USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            entry_price=Price.from_str("80.000"),
            sl_trigger_price=Price.from_str("70.000"),
            tp_price=Price.from_str("90.500"),
            entry_order_type=OrderType.LIMIT,
            time_in_force=TimeInForce.GTD,
            expire_time=UNIX_EPOCH + timedelta(minutes=1),
        )

        # Act
        strategy.submit_order_list(bracket)
        self.exchange.process(0)

        # Assert
        assert strategy.clock.timer_count == 1
        assert strategy.clock.timer_names == ["GTD-EXPIRY:O-19700101-000000-000-None-1"]

    def test_submit_order_list_with_managed_gtd_when_immediately_filled_cancels_timer(self) -> None:
        # Arrange
        config = StrategyConfig(manage_gtd_expiry=True)
        strategy = Strategy(config)
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        bracket = strategy.order_factory.bracket(
            _USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            entry_price=Price.from_str("90.100"),
            sl_trigger_price=Price.from_str("70.000"),
            tp_price=Price.from_str("90.500"),
            entry_order_type=OrderType.LIMIT,
            time_in_force=TimeInForce.GTD,
            expire_time=UNIX_EPOCH + timedelta(minutes=1),
        )

        # Act
        strategy.submit_order_list(bracket)
        self.exchange.process(0)

        # Assert
        assert strategy.clock.timer_count == 0
        assert bracket.orders[0].status == OrderStatus.FILLED
        assert bracket.orders[1].status == OrderStatus.ACCEPTED
        assert bracket.orders[2].status == OrderStatus.ACCEPTED

    def test_cancel_gtd_expiry(self) -> None:
        # Arrange
        config = StrategyConfig(manage_gtd_expiry=True)
        strategy = Strategy(config)
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order = strategy.order_factory.limit(
            _USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            price=Price.from_str("100.000"),
            time_in_force=TimeInForce.GTD,
            expire_time=UNIX_EPOCH + timedelta(minutes=1),
        )

        strategy.submit_order(order)

        # Act
        strategy.cancel_gtd_expiry(order)

        # Assert
        assert strategy.clock.timer_count == 0

    def test_cancel_order(self) -> None:
        # Arrange
        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order = strategy.order_factory.stop_market(
            _USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("90.006"),
        )

        strategy.submit_order(order)
        self.exchange.process(0)

        # Act
        strategy.cancel_order(order)
        self.exchange.process(0)

        # Assert
        assert order in strategy.cache.orders()
        assert strategy.cache.orders()[0].status == OrderStatus.CANCELED
        assert order.client_order_id == strategy.cache.orders_closed()[0].client_order_id
        assert order not in strategy.cache.orders_open()
        assert strategy.cache.order_exists(order.client_order_id)
        assert not strategy.cache.is_order_open(order.client_order_id)
        assert strategy.cache.is_order_closed(order.client_order_id)

    def test_cancel_order_when_pending_cancel_does_not_submit_command(self) -> None:
        # Arrange
        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order = strategy.order_factory.stop_market(
            _USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("90.006"),
        )

        strategy.submit_order(order)
        self.exchange.process(0)
        self.exec_engine.process(TestEventStubs.order_pending_cancel(order))

        # Act
        strategy.cancel_order(order)
        self.exchange.process(0)

        # Assert
        assert strategy.cache.orders()[0].status == OrderStatus.PENDING_CANCEL
        assert order in strategy.cache.orders_open()
        assert strategy.cache.order_exists(order.client_order_id)
        assert strategy.cache.is_order_open(order.client_order_id)
        assert not strategy.cache.is_order_closed(order.client_order_id)

    def test_cancel_order_when_closed_does_not_submit_command(self) -> None:
        # Arrange
        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order = strategy.order_factory.stop_market(
            _USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("90.006"),
        )

        strategy.submit_order(order)
        self.exchange.process(0)
        self.exec_engine.process(TestEventStubs.order_expired(order))

        # Act
        strategy.cancel_order(order)
        self.exchange.process(0)

        # Assert
        assert strategy.cache.orders()[0].status == OrderStatus.EXPIRED
        assert order not in strategy.cache.orders_open()
        assert strategy.cache.order_exists(order.client_order_id)
        assert not strategy.cache.is_order_open(order.client_order_id)
        assert strategy.cache.is_order_closed(order.client_order_id)

    def test_modify_order_when_pending_cancel_does_not_submit_command(self) -> None:
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
            _USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("90.001"),
        )

        strategy.submit_order(order)
        self.exchange.process(0)
        self.exec_engine.process(TestEventStubs.order_pending_cancel(order))

        # Act
        strategy.modify_order(
            order=order,
            quantity=Quantity.from_int(100_000),
            price=Price.from_str("90.000"),
        )
        self.exchange.process(0)

        # Assert
        assert self.exec_engine.command_count == 1

    def test_modify_order_when_closed_does_not_submit_command(self) -> None:
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
            _USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("90.001"),
        )

        strategy.submit_order(order)
        self.exchange.process(0)
        self.exec_engine.process(TestEventStubs.order_expired(order))

        # Act
        strategy.modify_order(
            order=order,
            quantity=Quantity.from_int(100_000),
            price=Price.from_str("90.000"),
        )
        self.exchange.process(0)

        # Assert
        assert self.exec_engine.command_count == 1

    def test_modify_order_when_no_changes_does_not_submit_command(self) -> None:
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
            _USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("90.001"),
        )

        strategy.submit_order(order)

        # Act
        strategy.modify_order(
            order=order,
            quantity=Quantity.from_int(100_000),
            price=Price.from_str("90.001"),
        )

        # Assert
        assert self.exec_engine.command_count == 1

    def test_modify_order(self) -> None:
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
            _USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("90.000"),
        )

        strategy.submit_order(order)
        self.exchange.process(0)

        # Act
        strategy.modify_order(
            order=order,
            quantity=Quantity.from_int(110000),
            price=Price.from_str("90.001"),
        )
        self.exchange.process(0)

        # Assert
        assert strategy.cache.orders()[0] == order
        assert strategy.cache.orders()[0].status == OrderStatus.ACCEPTED
        assert strategy.cache.orders()[0].quantity == Quantity.from_int(110_000)
        assert strategy.cache.orders()[0].price == Price.from_str("90.001")
        assert strategy.cache.order_exists(order.client_order_id)
        assert strategy.cache.is_order_open(order.client_order_id)
        assert not strategy.cache.is_order_closed(order.client_order_id)
        assert strategy.portfolio.is_flat(order.instrument_id)

    def test_cancel_orders(self) -> None:
        # Arrange
        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order1 = strategy.order_factory.stop_market(
            _USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("90.007"),
        )

        order2 = strategy.order_factory.stop_market(
            _USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("90.006"),
        )

        strategy.submit_order(order1)
        self.exchange.process(0)
        strategy.submit_order(order2)
        self.exchange.process(0)

        # Act
        strategy.cancel_orders([order1, order2])
        self.exchange.process(0)

        # Assert
        # TODO: WIP!

    def test_cancel_all_orders(self) -> None:
        # Arrange
        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order1 = strategy.order_factory.stop_market(
            _USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("90.007"),
        )

        order2 = strategy.order_factory.stop_market(
            _USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("90.006"),
        )

        strategy.submit_order(order1)
        self.exchange.process(0)
        strategy.submit_order(order2)
        self.exchange.process(0)

        # Act
        strategy.cancel_all_orders(_USDJPY_SIM.id)
        self.exchange.process(0)

        # Assert
        assert order1 in self.cache.orders()
        assert order2 in self.cache.orders()
        assert self.cache.orders()[0].status == OrderStatus.CANCELED
        assert self.cache.orders()[1].status == OrderStatus.CANCELED
        assert order1 in self.cache.orders_closed()
        assert order2 in strategy.cache.orders_closed()

    def test_close_position_when_position_already_closed_does_nothing(self) -> None:
        # Arrange
        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order1 = strategy.order_factory.market(
            _USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        order2 = strategy.order_factory.market(
            _USDJPY_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
        )

        strategy.submit_order(order1)
        self.exchange.process(0)

        position_id = strategy.cache.positions_open()[0].id  # Generated by exchange
        strategy.submit_order(order2, position_id)
        self.exchange.process(0)

        position = strategy.cache.positions_closed()[0]

        # Act
        strategy.close_position(position)
        self.exchange.process(0)

        # Assert
        assert strategy.portfolio.is_completely_flat()

    def test_close_position(self) -> None:
        # Arrange
        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        order = strategy.order_factory.market(
            _USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        strategy.submit_order(order)
        self.exchange.process(0)

        position = self.cache.positions_open()[0]

        # Act
        strategy.close_position(position, tags=["EXIT"])
        self.exchange.process(0)

        # Assert
        assert order.status == OrderStatus.FILLED
        assert strategy.portfolio.is_completely_flat()
        orders = self.cache.orders(instrument_id=_USDJPY_SIM.id)
        for order in orders:
            if order.side == OrderSide.SELL:
                assert order.tags == ["EXIT"]

    def test_close_all_positions(self) -> None:
        # Arrange
        strategy = Strategy()
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        strategy.start()

        order1 = strategy.order_factory.market(
            _USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        order2 = strategy.order_factory.market(
            _USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        strategy.submit_order(order1)
        self.exchange.process(0)
        strategy.submit_order(order2)
        self.exchange.process(0)

        # Act
        strategy.close_all_positions(_USDJPY_SIM.id, tags=["EXIT"])
        self.exchange.process(0)

        # Assert
        assert order1.status == OrderStatus.FILLED
        assert order2.status == OrderStatus.FILLED
        assert strategy.portfolio.is_completely_flat()
        orders = self.cache.orders(instrument_id=_USDJPY_SIM.id)
        for order in orders:
            if order.side == OrderSide.SELL:
                assert order.tags == ["EXIT"]

    @pytest.mark.parametrize(
        ("contingency_type"),
        [
            ContingencyType.OCO,
            ContingencyType.OUO,
        ],
    )
    def test_managed_contingencies_when_canceled_entry_then_cancels_oto_orders(
        self,
        contingency_type: ContingencyType,
    ) -> None:
        # Arrange
        config = StrategyConfig(
            manage_contingent_orders=True,
            manage_gtd_expiry=True,
        )
        strategy = Strategy(config=config)
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        strategy.start()

        bracket = strategy.order_factory.bracket(
            _USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            entry_price=Price.from_str("80.000"),
            sl_trigger_price=Price.from_str("90.000"),
            tp_price=Price.from_str("90.500"),
            entry_order_type=OrderType.LIMIT,
            contingency_type=contingency_type,
        )

        strategy.submit_order_list(bracket)
        self.exchange.process(0)

        # Act
        strategy.cancel_order(bracket.first)
        self.exchange.process(0)

        # Assert
        assert bracket.orders[0].status == OrderStatus.CANCELED
        assert bracket.orders[1].status == OrderStatus.PENDING_CANCEL
        assert bracket.orders[2].status == OrderStatus.PENDING_CANCEL

    @pytest.mark.parametrize(
        ("contingency_type"),
        [
            ContingencyType.OCO,
            ContingencyType.OUO,
        ],
    )
    def test_managed_contingencies_when_canceled_bracket_then_cancels_contingent_order(
        self,
        contingency_type: ContingencyType,
    ) -> None:
        # Arrange
        config = StrategyConfig(
            manage_contingent_orders=True,
            manage_gtd_expiry=True,
        )
        strategy = Strategy(config=config)
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        strategy.start()

        bracket = strategy.order_factory.bracket(
            _USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            sl_trigger_price=Price.from_str("90.000"),
            tp_price=Price.from_str("90.500"),
            entry_order_type=OrderType.MARKET,
            contingency_type=contingency_type,
        )

        strategy.submit_order_list(bracket)
        self.exchange.process(0)

        # Act
        strategy.cancel_order(bracket.orders[1])
        self.exchange.process(0)

        # Assert
        assert bracket.orders[0].status == OrderStatus.FILLED
        assert bracket.orders[1].status == OrderStatus.CANCELED
        assert bracket.orders[2].status == OrderStatus.PENDING_CANCEL

    def test_managed_contingencies_when_modify_bracket_then_modifies_ouo_order(
        self,
    ) -> None:
        # Arrange
        config = StrategyConfig(
            manage_contingent_orders=True,
            manage_gtd_expiry=True,
        )
        strategy = Strategy(config=config)
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        strategy.start()

        bracket = strategy.order_factory.bracket(
            _USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            sl_trigger_price=Price.from_str("90.000"),
            tp_price=Price.from_str("90.500"),
            entry_order_type=OrderType.MARKET,
            contingency_type=ContingencyType.OUO,
        )

        strategy.submit_order_list(bracket)
        self.exchange.process(0)

        # Act
        new_quantity = Quantity.from_int(50_000)
        strategy.modify_order(bracket.orders[1], new_quantity)
        self.exchange.process(0)

        # Assert
        assert bracket.orders[0].status == OrderStatus.FILLED
        assert bracket.orders[1].status == OrderStatus.ACCEPTED
        assert bracket.orders[2].status == OrderStatus.PENDING_UPDATE
        assert bracket.orders[1].quantity == new_quantity

    @pytest.mark.parametrize(
        ("contingency_type"),
        [
            ContingencyType.OCO,
            ContingencyType.OUO,
        ],
    )
    def test_managed_contingencies_when_filled_sl_then_cancels_contingent_order(
        self,
        contingency_type: ContingencyType,
    ) -> None:
        # Arrange
        config = StrategyConfig(
            manage_contingent_orders=True,
            manage_gtd_expiry=True,
        )
        strategy = Strategy(config=config)
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        strategy.start()

        bracket = strategy.order_factory.bracket(
            _USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            entry_trigger_price=Price.from_str("90.101"),
            entry_price=Price.from_str("90.100"),
            sl_trigger_price=Price.from_str("90.000"),
            tp_price=Price.from_str("90.500"),
            entry_order_type=OrderType.LIMIT_IF_TOUCHED,
            contingency_type=contingency_type,
        )

        entry_order = bracket.orders[0]
        sl_order = bracket.orders[1]
        tp_order = bracket.orders[2]

        strategy.submit_order_list(bracket)

        self.exec_engine.process(TestEventStubs.order_filled(entry_order, _USDJPY_SIM))
        self.exec_engine.process(
            TestEventStubs.order_filled(sl_order, _USDJPY_SIM, venue_order_id=VenueOrderId("2")),
        )
        self.exchange.process(0)

        # Assert
        assert entry_order.status == OrderStatus.FILLED
        assert sl_order.status == OrderStatus.FILLED
        assert tp_order.status == OrderStatus.CANCELED

    @pytest.mark.parametrize(
        ("contingency_type"),
        [
            ContingencyType.OCO,
            ContingencyType.OUO,
        ],
    )
    def test_managed_contingencies_when_filled_tp_then_cancels_contingent_order(
        self,
        contingency_type: ContingencyType,
    ) -> None:
        # Arrange
        config = StrategyConfig(
            manage_contingent_orders=True,
            manage_gtd_expiry=True,
        )
        strategy = Strategy(config=config)
        strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        strategy.start()

        bracket = strategy.order_factory.bracket(
            _USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            entry_trigger_price=Price.from_str("90.101"),
            entry_price=Price.from_str("90.100"),
            sl_trigger_price=Price.from_str("90.000"),
            tp_price=Price.from_str("90.500"),
            entry_order_type=OrderType.LIMIT_IF_TOUCHED,
            contingency_type=contingency_type,
        )

        entry_order = bracket.orders[0]
        sl_order = bracket.orders[1]
        tp_order = bracket.orders[2]

        strategy.submit_order_list(bracket)

        self.exec_engine.process(TestEventStubs.order_filled(entry_order, _USDJPY_SIM))
        self.exec_engine.process(
            TestEventStubs.order_filled(tp_order, _USDJPY_SIM, venue_order_id=VenueOrderId("2")),
        )
        self.exchange.process(0)

        # Assert
        assert entry_order.status == OrderStatus.FILLED
        assert tp_order.status == OrderStatus.FILLED
        assert sl_order.status == OrderStatus.CANCELED
