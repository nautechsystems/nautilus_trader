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

from datetime import datetime
from datetime import timedelta
import unittest

from parameterized import parameterized
import pytz

from nautilus_trader.backtest.data_client import BacktestMarketDataClient
from nautilus_trader.backtest.exchange import SimulatedExchange
from nautilus_trader.backtest.execution import BacktestExecClient
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.enums import ComponentState
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.core.fsm import InvalidStateTrigger
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.indicators.average.ema import ExponentialMovingAverage
from nautilus_trader.model.bar import Bar
from nautilus_trader.model.currencies import EUR
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.data import DataType
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OMSType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderState
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.enums import VenueType
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.risk.engine import RiskEngine
from nautilus_trader.trading.filters import NewsEvent
from nautilus_trader.trading.filters import NewsImpact
from nautilus_trader.trading.portfolio import Portfolio
from nautilus_trader.trading.strategy import TradingStrategy
from tests.test_kit.mocks import KaboomStrategy
from tests.test_kit.mocks import MockStrategy
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs
from tests.test_kit.stubs import UNIX_EPOCH


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
GBPUSD_SIM = TestInstrumentProvider.default_fx_ccy("GBP/USD")
USDJPY_SIM = TestInstrumentProvider.default_fx_ccy("USD/JPY")


class TradingStrategyTests(unittest.TestCase):
    def setUp(self):
        # Fixture Setup
        self.clock = TestClock()
        self.uuid_factory = UUIDFactory()
        self.logger = Logger(self.clock)

        self.cache = TestStubs.cache()

        self.portfolio = Portfolio(
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.data_engine = DataEngine(
            portfolio=self.portfolio,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
            config={
                "use_previous_close": False
            },  # To correctly reproduce historical data bars
        )

        account_id = TestStubs.account_id()

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
        )

        self.exec_engine.register_risk_engine(self.risk_engine)

        self.exchange = SimulatedExchange(
            venue=Venue("SIM"),
            venue_type=VenueType.ECN,
            oms_type=OMSType.HEDGING,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            starting_balances=[Money(1_000_000, USD)],
            is_frozen_account=False,
            cache=self.exec_engine.cache,
            instruments=[USDJPY_SIM],
            modules=[],
            fill_model=FillModel(),
            clock=self.clock,
            logger=self.logger,
        )

        self.data_client = BacktestMarketDataClient(
            client_id=ClientId("SIM"),
            engine=self.data_engine,
            clock=self.clock,
            logger=self.logger,
        )

        self.exec_client = BacktestExecClient(
            exchange=self.exchange,
            account_id=account_id,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            engine=self.exec_engine,
            clock=self.clock,
            logger=self.logger,
        )

        # Wire up components
        self.exchange.register_client(self.exec_client)
        self.data_engine.register_client(self.data_client)
        self.exec_engine.register_client(self.exec_client)
        self.exec_engine.process(TestStubs.event_account_state())

        # Add instruments
        self.data_engine.process(AUDUSD_SIM)
        self.data_engine.process(GBPUSD_SIM)
        self.data_engine.process(USDJPY_SIM)
        self.cache.add_instrument(AUDUSD_SIM)
        self.cache.add_instrument(GBPUSD_SIM)
        self.cache.add_instrument(USDJPY_SIM)

        self.exchange.process_tick(
            TestStubs.quote_tick_3decimal(USDJPY_SIM.id)
        )  # Prepare market

        self.data_engine.start()
        self.exec_engine.start()

    def test_strategy_equality(self):
        # Arrange
        strategy1 = TradingStrategy(order_id_tag="AUD/USD-001")
        strategy2 = TradingStrategy(order_id_tag="AUD/USD-001")
        strategy3 = TradingStrategy(order_id_tag="AUD/USD-002")

        # Act
        # Assert
        self.assertTrue(strategy1 == strategy1)
        self.assertTrue(strategy1 == strategy2)
        self.assertTrue(strategy2 != strategy3)

    def test_str_and_repr(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="GBP/USD-MM")

        # Act
        # Assert
        self.assertEqual("TradingStrategy-GBP/USD-MM", str(strategy))
        self.assertEqual("TradingStrategy-GBP/USD-MM", repr(strategy))

    def test_id(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="001")

        # Act
        # Assert
        self.assertEqual(StrategyId("TradingStrategy-001"), strategy.id)

    def test_initialization(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="001")

        # Act
        # Assert
        self.assertTrue(ComponentState.INITIALIZED, strategy.state)
        self.assertFalse(strategy.indicators_initialized())

    def test_handle_event(self):
        # Arrange
        strategy = TradingStrategy("000")

        event = TestStubs.event_account_state()

        # Act
        strategy.handle_event(event)

        # Assert
        self.assertTrue(True)  # Exception not raised

    def test_on_start_when_not_overridden_does_nothing(self):
        # Arrange
        strategy = TradingStrategy("000")

        # Act
        strategy.on_start()

        # Assert
        self.assertTrue(True)  # Exception not raised

    def test_on_stop_when_not_overridden_does_nothing(self):
        # Arrange
        strategy = TradingStrategy("000")

        # Act
        strategy.on_stop()

        # Assert
        self.assertTrue(True)  # Exception not raised

    def test_on_resume_when_not_overridden_does_nothing(self):
        # Arrange
        strategy = TradingStrategy("000")

        # Act
        strategy.on_resume()

        # Assert
        self.assertTrue(True)  # Exception not raised

    def test_on_reset_when_not_overridden_does_nothing(self):
        # Arrange
        strategy = TradingStrategy("000")

        # Act
        strategy.on_reset()

        # Assert
        self.assertTrue(True)  # Exception not raised

    def test_on_save_when_not_overridden_does_nothing(self):
        # Arrange
        strategy = TradingStrategy("000")

        # Act
        strategy.on_save()

        # Assert
        self.assertTrue(True)  # Exception not raised

    def test_on_load_when_not_overridden_does_nothing(self):
        # Arrange
        strategy = TradingStrategy("000")

        # Act
        strategy.on_load({})

        # Assert
        self.assertTrue(True)  # Exception not raised

    def test_on_dispose_when_not_overridden_does_nothing(self):
        # Arrange
        strategy = TradingStrategy("000")

        # Act
        strategy.on_load({})

        # Assert
        self.assertTrue(True)  # Exception not raised

    def test_on_quote_tick_when_not_overridden_does_nothing(self):
        # Arrange
        strategy = TradingStrategy("000")

        tick = TestStubs.quote_tick_5decimal()

        # Act
        strategy.on_quote_tick(tick)

        # Assert
        self.assertTrue(True)  # Exception not raised

    def test_on_trade_tick_when_not_overridden_does_nothing(self):
        # Arrange
        strategy = TradingStrategy("000")

        tick = TestStubs.trade_tick_5decimal()

        # Act
        strategy.on_trade_tick(tick)

        # Assert
        self.assertTrue(True)  # Exception not raised

    def test_on_bar_when_not_overridden_does_nothing(self):
        # Arrange
        strategy = TradingStrategy("000")

        bar = TestStubs.bar_5decimal()

        # Act
        strategy.on_bar(bar)

        # Assert
        self.assertTrue(True)  # Exception not raised

    def test_on_data_when_not_overridden_does_nothing(self):
        # Arrange
        strategy = TradingStrategy("000")
        news_event = NewsEvent(
            impact=NewsImpact.HIGH,
            name="Unemployment Rate",
            currency=EUR,
            ts_event_ns=0,
            ts_recv_ns=0,
        )

        # Act
        strategy.on_data(news_event)

        # Assert
        self.assertTrue(True)  # Exception not raised

    def test_on_event_when_not_overridden_does_nothing(self):
        # Arrange
        strategy = TradingStrategy("000")
        event = TestStubs.event_account_state(AccountId("SIM", "000"))

        # Act
        strategy.on_event(event)

        # Assert
        self.assertTrue(True)  # Exception not raised

    def test_start_when_not_registered_with_trader_raises_runtime_error(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="001")

        # Act
        # Assert
        self.assertRaises(RuntimeError, strategy.start)

    def test_stop_when_not_registered_with_trader_raises_runtime_error(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="001")

        try:
            strategy.start()
        except RuntimeError:
            # Normally a bad practice but allows strategy to be put into
            # the needed state to run the test.
            pass

        # Act
        # Assert
        self.assertRaises(RuntimeError, strategy.stop)

    def test_resume_when_not_registered_with_trader_raises_runtime_error(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="001")

        try:
            strategy.start()
        except RuntimeError:
            # Normally a bad practice but allows strategy to be put into
            # the needed state to run the test.
            pass

        try:
            strategy.stop()
        except RuntimeError:
            # Normally a bad practice but allows strategy to be put into
            # the needed state to run the test.
            pass

        # Act
        # Assert
        self.assertRaises(RuntimeError, strategy.resume)

    def test_reset_when_not_registered_with_trader_raises_runtime_error(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="001")

        # Act
        # Assert
        self.assertRaises(RuntimeError, strategy.reset)

    def test_dispose_when_not_registered_with_trader_raises_runtime_error(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="001")

        # Act
        # Assert
        self.assertRaises(RuntimeError, strategy.dispose)

    def test_save_when_not_registered_with_trader_raises_runtime_error(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="001")

        # Act
        # Assert
        self.assertRaises(RuntimeError, strategy.save)

    def test_load_when_not_registered_with_trader_raises_runtime_error(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="001")

        # Act
        # Assert
        self.assertRaises(RuntimeError, strategy.load, {})

    def test_start_when_not_in_valid_state_raises_invalid_state_trigger(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        strategy.dispose()  # Always a final state

        # Act
        # Assert
        self.assertRaises(InvalidStateTrigger, strategy.start)

    def test_stop_when_not_in_valid_state_raises_invalid_state_trigger(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        strategy.dispose()  # Always a final state

        # Act
        # Assert
        self.assertRaises(InvalidStateTrigger, strategy.stop)

    def test_resume_when_not_in_valid_state_raises_invalid_state_trigger(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        strategy.dispose()  # Always a final state

        # Act
        # Assert
        self.assertRaises(InvalidStateTrigger, strategy.resume)

    def test_reset_when_not_in_valid_state_raises_invalid_state_trigger(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        strategy.dispose()  # Always a final state

        # Act
        # Assert
        self.assertRaises(InvalidStateTrigger, strategy.reset)

    def test_dispose_when_not_in_valid_state_raises_invalid_state_trigger(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        strategy.dispose()  # Always a final state

        # Act
        # Assert
        self.assertRaises(InvalidStateTrigger, strategy.dispose)

    def test_start_when_user_code_raises_error_logs_and_reraises(self):
        # Arrange
        strategy = KaboomStrategy()
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        # Act
        # Assert
        self.assertRaises(RuntimeError, strategy.start)
        self.assertEqual(ComponentState.RUNNING, strategy.state)

    def test_stop_when_user_code_raises_error_logs_and_reraises(self):
        # Arrange
        strategy = KaboomStrategy()
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        strategy.set_explode_on_start(False)
        strategy.start()

        # Act
        # Assert
        self.assertRaises(RuntimeError, strategy.stop)
        self.assertEqual(ComponentState.STOPPED, strategy.state)

    def test_resume_when_user_code_raises_error_logs_and_reraises(self):
        # Arrange
        strategy = KaboomStrategy()
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        strategy.set_explode_on_start(False)
        strategy.set_explode_on_stop(False)
        strategy.start()
        strategy.stop()

        # Act
        # Assert
        self.assertRaises(RuntimeError, strategy.resume)
        self.assertEqual(ComponentState.RUNNING, strategy.state)

    def test_reset_when_user_code_raises_error_logs_and_reraises(self):
        # Arrange
        strategy = KaboomStrategy()
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        # Act
        # Assert
        self.assertRaises(RuntimeError, strategy.reset)
        self.assertEqual(ComponentState.INITIALIZED, strategy.state)

    def test_dispose_when_user_code_raises_error_logs_and_reraises(self):
        # Arrange
        strategy = KaboomStrategy()
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        # Act
        # Assert
        self.assertRaises(RuntimeError, strategy.dispose)
        self.assertEqual(ComponentState.DISPOSED, strategy.state)

    def test_save_when_user_code_raises_error_logs_and_reraises(self):
        # Arrange
        strategy = KaboomStrategy()
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        # Act
        # Assert
        self.assertRaises(RuntimeError, strategy.save)

    def test_load_when_user_code_raises_error_logs_and_reraises(self):
        # Arrange
        strategy = KaboomStrategy()
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        # Act
        # Assert
        self.assertRaises(RuntimeError, strategy.load, {"something": b"123456"})

    def test_load(self):
        # Arrange
        strategy = TradingStrategy("000")
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        state = {}

        # Act
        strategy.load(state)

        # Assert
        # TODO: Write a users custom save method
        self.assertTrue(True)

    def test_handle_quote_tick_when_user_code_raises_exception_logs_and_reraises(self):
        # Arrange
        strategy = KaboomStrategy()
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        strategy.set_explode_on_start(False)
        strategy.start()

        tick = TestStubs.quote_tick_5decimal(AUDUSD_SIM.id)

        # Act
        # Assert
        self.assertRaises(RuntimeError, strategy.handle_quote_tick, tick)

    def test_handle_trade_tick_when_user_code_raises_exception_logs_and_reraises(self):
        # Arrange
        strategy = KaboomStrategy()
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        strategy.set_explode_on_start(False)
        strategy.start()

        tick = TestStubs.trade_tick_5decimal(AUDUSD_SIM.id)

        # Act
        # Assert
        self.assertRaises(RuntimeError, strategy.handle_trade_tick, tick)

    def test_handle_bar_when_user_code_raises_exception_logs_and_reraises(self):
        # Arrange
        strategy = KaboomStrategy()
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        strategy.set_explode_on_start(False)
        strategy.start()

        bar = TestStubs.bar_5decimal()

        # Act
        # Assert
        self.assertRaises(RuntimeError, strategy.handle_bar, bar)

    def test_handle_data_when_user_code_raises_exception_logs_and_reraises(self):
        # Arrange
        strategy = KaboomStrategy()
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        strategy.set_explode_on_start(False)
        strategy.start()

        # Act
        # Assert
        self.assertRaises(
            RuntimeError,
            strategy.handle_data,
            NewsEvent(
                impact=NewsImpact.HIGH,
                name="Unemployment Rate",
                currency=USD,
                ts_event_ns=0,
                ts_recv_ns=0,
            ),
        )

    def test_handle_event_when_user_code_raises_exception_logs_and_reraises(self):
        # Arrange
        strategy = KaboomStrategy()
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        strategy.set_explode_on_start(False)
        strategy.start()

        event = TestStubs.event_account_state(AccountId("TEST", "000"))

        # Act
        # Assert
        self.assertRaises(RuntimeError, strategy.on_event, event)

    def test_register_data_engine(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        # Act
        strategy.register_data_engine(self.data_engine)

        # Assert
        self.assertIsNotNone(strategy.cache)

    def test_register_risk_engine(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        # Act
        strategy.register_risk_engine(self.risk_engine)

        # Assert
        self.assertIsNotNone(strategy.cache)

    def test_register_portfolio(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        # Act
        strategy.register_portfolio(self.portfolio)

        # Assert
        self.assertIsNotNone(strategy.portfolio)

    def test_start(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = MockStrategy(bar_type)
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        self.data_engine.register_strategy(strategy)
        self.exec_engine.register_strategy(strategy)

        # Act
        strategy.start()

        # Assert
        self.assertTrue("on_start" in strategy.calls)
        self.assertEqual(ComponentState.RUNNING, strategy.state)

    def test_stop(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = MockStrategy(bar_type)
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        self.data_engine.register_strategy(strategy)
        self.exec_engine.register_strategy(strategy)

        # Act
        strategy.start()
        strategy.stop()

        # Assert
        self.assertTrue("on_stop" in strategy.calls)
        self.assertEqual(ComponentState.STOPPED, strategy.state)

    def test_resume(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = MockStrategy(bar_type)
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        self.data_engine.register_strategy(strategy)
        self.exec_engine.register_strategy(strategy)

        strategy.start()
        strategy.stop()

        # Act
        strategy.resume()

        # Assert
        self.assertTrue("on_resume" in strategy.calls)
        self.assertEqual(ComponentState.RUNNING, strategy.state)

    def test_reset(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = MockStrategy(bar_type)
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        bar = Bar(
            bar_type,
            Price.from_str("1.00001"),
            Price.from_str("1.00004"),
            Price.from_str("1.00002"),
            Price.from_str("1.00003"),
            Quantity.from_int(100000),
            0,
            0,
        )

        strategy.handle_bar(bar)

        # Act
        strategy.reset()

        # Assert
        self.assertTrue("on_reset" in strategy.calls)
        self.assertEqual(ComponentState.INITIALIZED, strategy.state)
        self.assertEqual(0, strategy.ema1.count)
        self.assertEqual(0, strategy.ema2.count)

    def test_dispose(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = MockStrategy(bar_type)
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        strategy.reset()

        # Act
        strategy.dispose()

        # Assert
        self.assertTrue("on_dispose" in strategy.calls)
        self.assertEqual(ComponentState.DISPOSED, strategy.state)

    def test_save_load(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = MockStrategy(bar_type)
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        # Act
        state = strategy.save()
        strategy.load(state)

        # Assert
        self.assertEqual({"UserState": 1}, state)
        self.assertTrue("on_save" in strategy.calls)
        self.assertEqual(ComponentState.INITIALIZED, strategy.state)

    def test_register_indicator_for_quote_ticks_when_already_registered(self):
        # Arrange
        strategy = TradingStrategy("000")
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        ema1 = ExponentialMovingAverage(10, price_type=PriceType.MID)
        ema2 = ExponentialMovingAverage(10, price_type=PriceType.MID)

        # Act
        strategy.register_indicator_for_quote_ticks(AUDUSD_SIM.id, ema1)
        strategy.register_indicator_for_quote_ticks(AUDUSD_SIM.id, ema2)
        strategy.register_indicator_for_quote_ticks(AUDUSD_SIM.id, ema2)

        self.assertEqual(2, len(strategy.registered_indicators))
        self.assertIn(ema1, strategy.registered_indicators)
        self.assertIn(ema2, strategy.registered_indicators)

    def test_register_indicator_for_trade_ticks_when_already_registered(self):
        # Arrange
        strategy = TradingStrategy("000")
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        ema1 = ExponentialMovingAverage(10)
        ema2 = ExponentialMovingAverage(10)

        # Act
        strategy.register_indicator_for_trade_ticks(AUDUSD_SIM.id, ema1)
        strategy.register_indicator_for_trade_ticks(AUDUSD_SIM.id, ema2)
        strategy.register_indicator_for_trade_ticks(AUDUSD_SIM.id, ema2)

        self.assertEqual(2, len(strategy.registered_indicators))
        self.assertIn(ema1, strategy.registered_indicators)
        self.assertIn(ema2, strategy.registered_indicators)

    def test_register_indicator_for_bars_when_already_registered(self):
        # Arrange
        strategy = TradingStrategy("000")
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        ema1 = ExponentialMovingAverage(10)
        ema2 = ExponentialMovingAverage(10)
        bar_type = TestStubs.bartype_audusd_1min_bid()

        # Act
        strategy.register_indicator_for_bars(bar_type, ema1)
        strategy.register_indicator_for_bars(bar_type, ema2)
        strategy.register_indicator_for_bars(bar_type, ema2)

        self.assertEqual(2, len(strategy.registered_indicators))
        self.assertIn(ema1, strategy.registered_indicators)
        self.assertIn(ema2, strategy.registered_indicators)

    def test_register_indicator_for_multiple_data_sources(self):
        # Arrange
        strategy = TradingStrategy("000")
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        ema = ExponentialMovingAverage(10)
        bar_type = TestStubs.bartype_audusd_1min_bid()

        # Act
        strategy.register_indicator_for_quote_ticks(AUDUSD_SIM.id, ema)
        strategy.register_indicator_for_quote_ticks(GBPUSD_SIM.id, ema)
        strategy.register_indicator_for_trade_ticks(AUDUSD_SIM.id, ema)
        strategy.register_indicator_for_bars(bar_type, ema)

        self.assertEqual(1, len(strategy.registered_indicators))
        self.assertIn(ema, strategy.registered_indicators)

    def test_handle_quote_tick_updates_indicator_registered_for_quote_ticks(self):
        # Arrange
        strategy = TradingStrategy("000")
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        ema = ExponentialMovingAverage(10, price_type=PriceType.MID)
        strategy.register_indicator_for_quote_ticks(AUDUSD_SIM.id, ema)

        tick = TestStubs.quote_tick_5decimal(AUDUSD_SIM.id)

        # Act
        strategy.handle_quote_tick(tick)
        strategy.handle_quote_tick(tick, True)

        # Assert
        self.assertEqual(2, ema.count)

    def test_handle_instrument_with_blow_up_logs_exception(self):
        # Arrange
        strategy = KaboomStrategy()
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        strategy.set_explode_on_start(False)
        strategy.start()

        # Act
        # Assert
        self.assertRaises(RuntimeError, strategy.handle_instrument, AUDUSD_SIM)

    def test_handle_instrument_when_not_running_does_not_send_to_on_instrument(self):
        # Arrange
        strategy = MockStrategy(TestStubs.bartype_audusd_1min_bid())
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        # Act
        strategy.handle_instrument(AUDUSD_SIM)

        # Assert
        self.assertEqual([], strategy.calls)
        self.assertEqual([], strategy.object_storer.get_store())

    def test_handle_instrument_when_running_sends_to_on_instrument(self):
        # Arrange
        strategy = MockStrategy(TestStubs.bartype_audusd_1min_bid())
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        strategy.start()

        # Act
        strategy.handle_instrument(AUDUSD_SIM)

        # Assert
        self.assertEqual(["on_start", "on_instrument"], strategy.calls)
        self.assertEqual(AUDUSD_SIM, strategy.object_storer.get_store()[0])

    def test_handle_quote_tick_when_not_running_does_not_send_to_on_quote_tick(self):
        # Arrange
        strategy = MockStrategy(TestStubs.bartype_audusd_1min_bid())
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        tick = TestStubs.quote_tick_5decimal(AUDUSD_SIM.id)

        # Act
        strategy.handle_quote_tick(tick)

        # Assert
        self.assertEqual([], strategy.calls)
        self.assertEqual([], strategy.object_storer.get_store())

    def test_handle_quote_tick_when_running_sends_to_on_quote_tick(self):
        # Arrange
        strategy = MockStrategy(TestStubs.bartype_audusd_1min_bid())
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        strategy.start()

        tick = TestStubs.quote_tick_5decimal(AUDUSD_SIM.id)

        # Act
        strategy.handle_quote_tick(tick)

        # Assert
        self.assertEqual(["on_start", "on_quote_tick"], strategy.calls)
        self.assertEqual(tick, strategy.object_storer.get_store()[0])

    def test_handle_quote_ticks_with_no_ticks_logs_and_continues(self):
        # Arrange
        strategy = KaboomStrategy()
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        ema = ExponentialMovingAverage(10, price_type=PriceType.MID)
        strategy.register_indicator_for_quote_ticks(AUDUSD_SIM.id, ema)

        # Act
        strategy.handle_quote_ticks([])

        # Assert
        self.assertEqual(0, ema.count)

    def test_handle_quote_ticks_updates_indicator_registered_for_quote_ticks(self):
        # Arrange
        strategy = TradingStrategy("000")
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        ema = ExponentialMovingAverage(10, price_type=PriceType.MID)
        strategy.register_indicator_for_quote_ticks(AUDUSD_SIM.id, ema)

        tick = TestStubs.quote_tick_5decimal(AUDUSD_SIM.id)

        # Act
        strategy.handle_quote_ticks([tick])

        # Assert
        self.assertEqual(1, ema.count)

    def test_handle_trade_tick_when_not_running_does_not_send_to_on_trade_tick(self):
        # Arrange
        strategy = MockStrategy(TestStubs.bartype_audusd_1min_bid())
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        tick = TestStubs.trade_tick_5decimal(AUDUSD_SIM.id)

        # Act
        strategy.handle_trade_tick(tick)

        # Assert
        self.assertEqual([], strategy.calls)
        self.assertEqual([], strategy.object_storer.get_store())

    def test_handle_trade_tick_when_running_sends_to_on_trade_tick(self):
        # Arrange
        strategy = MockStrategy(TestStubs.bartype_audusd_1min_bid())
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        strategy.start()

        tick = TestStubs.trade_tick_5decimal(AUDUSD_SIM.id)

        # Act
        strategy.handle_trade_tick(tick)

        # Assert
        self.assertEqual(["on_start", "on_trade_tick"], strategy.calls)
        self.assertEqual(tick, strategy.object_storer.get_store()[0])

    def test_handle_trade_tick_updates_indicator_registered_for_trade_ticks(self):
        # Arrange
        strategy = TradingStrategy("000")
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        ema = ExponentialMovingAverage(10)
        strategy.register_indicator_for_trade_ticks(AUDUSD_SIM.id, ema)

        tick = TestStubs.trade_tick_5decimal(AUDUSD_SIM.id)

        # Act
        strategy.handle_trade_tick(tick)
        strategy.handle_trade_tick(tick, True)

        # Assert
        self.assertEqual(2, ema.count)

    def test_handle_trade_ticks_updates_indicator_registered_for_trade_ticks(self):
        # Arrange
        strategy = TradingStrategy("000")
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        ema = ExponentialMovingAverage(10)
        strategy.register_indicator_for_trade_ticks(AUDUSD_SIM.id, ema)

        tick = TestStubs.trade_tick_5decimal(AUDUSD_SIM.id)

        # Act
        strategy.handle_trade_ticks([tick])

        # Assert
        self.assertEqual(1, ema.count)

    def test_handle_trade_ticks_with_no_ticks_logs_and_continues(self):
        # Arrange
        strategy = TradingStrategy("000")
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        ema = ExponentialMovingAverage(10)
        strategy.register_indicator_for_trade_ticks(AUDUSD_SIM.id, ema)

        # Act
        strategy.handle_trade_ticks([])

        # Assert
        self.assertEqual(0, ema.count)

    def test_handle_bar_updates_indicator_registered_for_bars(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = TradingStrategy("000")
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        ema = ExponentialMovingAverage(10)
        strategy.register_indicator_for_bars(bar_type, ema)
        bar = TestStubs.bar_5decimal()

        # Act
        strategy.handle_bar(bar)
        strategy.handle_bar(bar, True)

        # Assert
        self.assertEqual(2, ema.count)

    def test_handle_bar_when_not_running_does_not_send_to_on_bar(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = MockStrategy(bar_type)
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        bar = TestStubs.bar_5decimal()

        # Act
        strategy.handle_bar(bar)

        # Assert
        self.assertEqual([], strategy.calls)
        self.assertEqual([], strategy.object_storer.get_store())

    def test_handle_bar_when_running_sends_to_on_bar(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = MockStrategy(bar_type)
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        strategy.start()

        bar = TestStubs.bar_5decimal()

        # Act
        strategy.handle_bar(bar)

        # Assert
        self.assertEqual(["on_start", "on_bar"], strategy.calls)
        self.assertEqual(bar, strategy.object_storer.get_store()[0])

    def test_handle_bars_updates_indicator_registered_for_bars(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = TradingStrategy("000")
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        ema = ExponentialMovingAverage(10)
        strategy.register_indicator_for_bars(bar_type, ema)
        bar = TestStubs.bar_5decimal()

        # Act
        strategy.handle_bars([bar])

        # Assert
        self.assertEqual(1, ema.count)

    def test_handle_bars_with_no_bars_logs_and_continues(self):
        # Arrange
        bar_type = TestStubs.bartype_gbpusd_1sec_mid()
        strategy = TradingStrategy("000")
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        ema = ExponentialMovingAverage(10)
        strategy.register_indicator_for_bars(bar_type, ema)

        # Act
        strategy.handle_bars([])

        # Assert
        self.assertEqual(0, ema.count)

    def test_handle_data_when_not_running_does_not_send_to_on_data(self):
        strategy = MockStrategy(TestStubs.bartype_audusd_1min_bid())
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        data = NewsEvent(
            impact=NewsImpact.HIGH,
            name="Unemployment Rate",
            currency=USD,
            ts_event_ns=0,
            ts_recv_ns=0,
        )

        # Act
        strategy.handle_data(data)

        # Assert
        self.assertEqual([], strategy.calls)
        self.assertEqual([], strategy.object_storer.get_store())

    def test_handle_data_when_running_sends_to_on_data(self):
        strategy = MockStrategy(TestStubs.bartype_audusd_1min_bid())
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        strategy.start()

        data = NewsEvent(
            impact=NewsImpact.HIGH,
            name="Unemployment Rate",
            currency=USD,
            ts_event_ns=0,
            ts_recv_ns=0,
        )

        # Act
        strategy.handle_data(data)

        # Assert
        self.assertEqual(["on_start", "on_data"], strategy.calls)
        self.assertEqual(data, strategy.object_storer.get_store()[0])

    def test_stop_cancels_a_running_time_alert(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = MockStrategy(bar_type)
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        self.data_engine.register_strategy(strategy)
        self.exec_engine.register_strategy(strategy)

        alert_time = datetime.now(pytz.utc) + timedelta(milliseconds=200)
        strategy.clock.set_time_alert("test_alert1", alert_time)

        # Act
        strategy.start()
        strategy.stop()

        # Assert
        self.assertEqual(0, len(strategy.clock.timer_names()))

    def test_stop_cancels_a_running_timer(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = MockStrategy(bar_type)
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        self.data_engine.register_strategy(strategy)
        self.exec_engine.register_strategy(strategy)

        start_time = datetime.now(pytz.utc) + timedelta(milliseconds=100)
        strategy.clock.set_timer(
            "test_timer", timedelta(milliseconds=100), start_time, stop_time=None
        )

        # Act
        strategy.start()
        strategy.stop()

        # Assert
        self.assertEqual(0, len(strategy.clock.timer_names()))

    def test_subscribe_custom_data(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = MockStrategy(bar_type)
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        self.data_engine.register_strategy(strategy)
        self.exec_engine.register_strategy(strategy)

        data_type = DataType(str, {"type": "NEWS_WIRE", "topic": "Earthquake"})

        # Act
        strategy.subscribe_data(ClientId("QUANDL"), data_type)

        # Assert
        self.assertEqual(1, self.data_engine.command_count)

    def test_unsubscribe_custom_data(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = MockStrategy(bar_type)
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        self.data_engine.register_strategy(strategy)
        self.exec_engine.register_strategy(strategy)

        data_type = DataType(str, {"type": "NEWS_WIRE", "topic": "Earthquake"})
        strategy.subscribe_data(ClientId("QUANDL"), data_type)

        # Act
        strategy.unsubscribe_data(ClientId("QUANDL"), data_type)

        # Assert
        self.assertEqual(2, self.data_engine.command_count)

    def test_subscribe_order_book(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = MockStrategy(bar_type)
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        self.data_engine.register_strategy(strategy)
        self.exec_engine.register_strategy(strategy)

        # Act
        strategy.subscribe_order_book(AUDUSD_SIM.id, level=2)

        # Assert
        self.assertEqual(1, self.data_engine.command_count)

    def test_unsubscribe_order_book(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = MockStrategy(bar_type)
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        self.data_engine.register_strategy(strategy)
        self.exec_engine.register_strategy(strategy)

        strategy.subscribe_order_book(AUDUSD_SIM.id, level=2)

        # Act
        strategy.unsubscribe_order_book(AUDUSD_SIM.id)

        # Assert
        self.assertEqual(2, self.data_engine.command_count)

    def test_subscribe_order_book_data(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = MockStrategy(bar_type)
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        self.data_engine.register_strategy(strategy)
        self.exec_engine.register_strategy(strategy)

        # Act
        strategy.subscribe_order_book_deltas(AUDUSD_SIM.id, level=2)

        # Assert
        self.assertEqual(1, self.data_engine.command_count)

    def test_unsubscribe_order_book_data(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = MockStrategy(bar_type)
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        self.data_engine.register_strategy(strategy)
        self.exec_engine.register_strategy(strategy)

        strategy.unsubscribe_order_book_deltas(AUDUSD_SIM.id)

        # Act
        strategy.unsubscribe_order_book(AUDUSD_SIM.id)

        # Assert
        self.assertEqual(2, self.data_engine.command_count)

    def test_subscribe_instrument(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = MockStrategy(bar_type)
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        self.data_engine.register_strategy(strategy)
        self.exec_engine.register_strategy(strategy)

        # Act
        strategy.subscribe_instrument(AUDUSD_SIM.id)

        # Assert
        expected_instrument = InstrumentId(Symbol("AUD/USD"), Venue("SIM"))
        self.assertEqual([expected_instrument], self.data_engine.subscribed_instruments)
        self.assertEqual(1, self.data_engine.command_count)

    def test_unsubscribe_instrument(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = MockStrategy(bar_type)
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        self.data_engine.register_strategy(strategy)
        self.exec_engine.register_strategy(strategy)

        strategy.subscribe_instrument(AUDUSD_SIM.id)

        # Act
        strategy.unsubscribe_instrument(AUDUSD_SIM.id)

        # Assert
        self.assertEqual([], self.data_engine.subscribed_instruments)
        self.assertEqual(2, self.data_engine.command_count)

    def test_subscribe_quote_ticks(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = MockStrategy(bar_type)
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        self.data_engine.register_strategy(strategy)
        self.exec_engine.register_strategy(strategy)

        # Act
        strategy.subscribe_quote_ticks(AUDUSD_SIM.id)

        # Assert
        expected_instrument = InstrumentId(Symbol("AUD/USD"), Venue("SIM"))
        self.assertEqual([expected_instrument], self.data_engine.subscribed_quote_ticks)
        self.assertEqual(1, self.data_engine.command_count)

    def test_unsubscribe_quote_ticks(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = MockStrategy(bar_type)
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        self.data_engine.register_strategy(strategy)
        self.exec_engine.register_strategy(strategy)

        strategy.subscribe_quote_ticks(AUDUSD_SIM.id)

        # Act
        strategy.unsubscribe_quote_ticks(AUDUSD_SIM.id)

        # Assert
        self.assertEqual([], self.data_engine.subscribed_quote_ticks)
        self.assertEqual(2, self.data_engine.command_count)

    def test_subscribe_trade_ticks(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = MockStrategy(bar_type)
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        self.data_engine.register_strategy(strategy)
        self.exec_engine.register_strategy(strategy)

        # Act
        strategy.subscribe_trade_ticks(AUDUSD_SIM.id)

        # Assert
        expected_instrument = InstrumentId(Symbol("AUD/USD"), Venue("SIM"))
        self.assertEqual([expected_instrument], self.data_engine.subscribed_trade_ticks)
        self.assertEqual(1, self.data_engine.command_count)

    def test_unsubscribe_trade_ticks(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = MockStrategy(bar_type)
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        self.data_engine.register_strategy(strategy)
        self.exec_engine.register_strategy(strategy)

        strategy.subscribe_trade_ticks(AUDUSD_SIM.id)

        # Act
        strategy.unsubscribe_trade_ticks(AUDUSD_SIM.id)

        # Assert
        self.assertEqual([], self.data_engine.subscribed_trade_ticks)
        self.assertEqual(2, self.data_engine.command_count)

    def test_subscribe_bars(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = MockStrategy(bar_type)
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        self.data_engine.register_strategy(strategy)
        self.exec_engine.register_strategy(strategy)

        # Act
        strategy.subscribe_bars(bar_type)

        # Assert
        self.assertEqual([bar_type], self.data_engine.subscribed_bars)
        self.assertEqual(1, self.data_engine.command_count)

    def test_unsubscribe_bars(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = MockStrategy(bar_type)
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        self.data_engine.register_strategy(strategy)
        self.exec_engine.register_strategy(strategy)

        strategy.subscribe_bars(bar_type)

        # Act
        strategy.unsubscribe_bars(bar_type)

        # Assert
        self.assertEqual([], self.data_engine.subscribed_bars)
        self.assertEqual(2, self.data_engine.command_count)

    def test_request_data_sends_request_to_data_engine(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = MockStrategy(bar_type)
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        self.data_engine.register_strategy(strategy)
        self.exec_engine.register_strategy(strategy)

        data_type = DataType(str, {"type": "NEWS_WIRE", "topic": "Earthquakes"})

        # Act
        strategy.request_data(ClientId("BLOOMBERG-01"), data_type)

        # Assert
        self.assertEqual(1, self.data_engine.request_count)

    def test_request_quote_ticks_sends_request_to_data_engine(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = MockStrategy(bar_type)
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        self.data_engine.register_strategy(strategy)
        self.exec_engine.register_strategy(strategy)

        # Act
        strategy.request_quote_ticks(AUDUSD_SIM.id)

        # Assert
        self.assertEqual(1, self.data_engine.request_count)

    def test_request_trade_ticks_sends_request_to_data_engine(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = MockStrategy(bar_type)
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        self.data_engine.register_strategy(strategy)
        self.exec_engine.register_strategy(strategy)

        # Act
        strategy.request_trade_ticks(AUDUSD_SIM.id)

        # Assert
        self.assertEqual(1, self.data_engine.request_count)

    def test_request_bars_sends_request_to_data_engine(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = MockStrategy(bar_type)
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        self.data_engine.register_strategy(strategy)
        self.exec_engine.register_strategy(strategy)

        # Act
        strategy.request_bars(bar_type)

        # Assert
        self.assertEqual(1, self.data_engine.request_count)

    @parameterized.expand(
        [
            [UNIX_EPOCH, UNIX_EPOCH],
            [UNIX_EPOCH + timedelta(milliseconds=1), UNIX_EPOCH],
        ]
    )
    def test_request_bars_with_invalid_params_raises_value_error(self, start, stop):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = MockStrategy(bar_type)
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        self.data_engine.register_strategy(strategy)
        self.exec_engine.register_strategy(strategy)

        # Act
        # Assert
        self.assertRaises(ValueError, strategy.request_bars, bar_type, start, stop)

    def test_submit_order_with_valid_order_successfully_submits(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        order = strategy.order_factory.market(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        # Act
        strategy.submit_order(order)

        # Assert
        self.assertIn(order, strategy.cache.orders())
        self.assertEqual(OrderState.FILLED, strategy.cache.orders()[0].state)
        self.assertNotIn(order.client_order_id, strategy.cache.orders_working())
        self.assertFalse(strategy.cache.is_order_working(order.client_order_id))
        self.assertTrue(strategy.cache.is_order_completed(order.client_order_id))

    def test_submit_bracket_order_with_valid_order_successfully_submits(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        entry = strategy.order_factory.stop_market(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            price=Price.from_str("90.100"),
        )

        order = strategy.order_factory.bracket(
            entry_order=entry,
            stop_loss=Price.from_str("90.000"),
            take_profit=Price.from_str("90.500"),
        )

        # Act
        strategy.submit_bracket_order(order)

        # Assert
        self.assertIn(entry, strategy.cache.orders())
        self.assertEqual(OrderState.ACCEPTED, entry.state)
        self.assertIn(entry, strategy.cache.orders_working())
        self.assertTrue(strategy.cache.is_order_working(entry.client_order_id))
        self.assertFalse(strategy.cache.is_order_completed(entry.client_order_id))

    def test_cancel_order(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        order = strategy.order_factory.stop_market(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("90.006"),
        )

        strategy.submit_order(order)

        # Act
        strategy.cancel_order(order)

        # Assert
        self.assertIn(order, strategy.cache.orders())
        self.assertEqual(OrderState.CANCELED, strategy.cache.orders()[0].state)
        self.assertEqual(
            order.client_order_id,
            strategy.cache.orders_completed()[0].client_order_id,
        )
        self.assertNotIn(order.client_order_id, strategy.cache.orders_working())
        self.assertTrue(strategy.cache.order_exists(order.client_order_id))
        self.assertFalse(strategy.cache.is_order_working(order.client_order_id))
        self.assertTrue(strategy.cache.is_order_completed(order.client_order_id))

    def test_update_order_when_no_changes_does_not_submit_command(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        order = strategy.order_factory.limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("90.001"),
        )

        strategy.submit_order(order)

        # Act
        strategy.update_order(
            order, Quantity.from_int(100000), Price.from_str("90.001")
        )

        # Assert
        self.assertEqual(1, self.exec_engine.command_count)

    def test_update_order(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        order = strategy.order_factory.limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("90.000"),
        )

        strategy.submit_order(order)

        # Act
        strategy.update_order(
            order, Quantity.from_int(110000), Price.from_str("90.001")
        )

        # Assert
        self.assertEqual(order, strategy.cache.orders()[0])
        self.assertEqual(OrderState.ACCEPTED, strategy.cache.orders()[0].state)
        self.assertEqual(Quantity.from_int(110000), strategy.cache.orders()[0].quantity)
        self.assertEqual(Price.from_str("90.001"), strategy.cache.orders()[0].price)
        self.assertTrue(strategy.cache.order_exists(order.client_order_id))
        self.assertTrue(strategy.cache.is_order_working(order.client_order_id))
        self.assertFalse(strategy.cache.is_order_completed(order.client_order_id))
        self.assertTrue(strategy.portfolio.is_flat(order.instrument_id))

    def test_cancel_all_orders(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        order1 = strategy.order_factory.stop_market(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("90.007"),
        )

        order2 = strategy.order_factory.stop_market(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("90.006"),
        )

        strategy.submit_order(order1)
        strategy.submit_order(order2)

        # Act
        strategy.cancel_all_orders(USDJPY_SIM.id)

        # Assert
        self.assertIn(order1, self.cache.orders())
        self.assertIn(order2, self.cache.orders())
        self.assertEqual(OrderState.CANCELED, self.cache.orders()[0].state)
        self.assertEqual(OrderState.CANCELED, self.cache.orders()[1].state)
        self.assertIn(order1, self.cache.orders_completed())
        self.assertIn(order2, strategy.cache.orders_completed())

    def test_flatten_position_when_position_already_flat_does_nothing(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        # Wire strategy into system
        self.data_engine.register_strategy(strategy)
        self.exec_engine.register_strategy(strategy)

        order1 = strategy.order_factory.market(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        order2 = strategy.order_factory.market(
            USDJPY_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100000),
        )

        strategy.submit_order(order1)
        strategy.submit_order(order2, PositionId("1-001"))  # Generated by exchange

        position = strategy.cache.positions_closed()[0]

        # Act
        strategy.flatten_position(position)

        # Assert
        self.assertTrue(strategy.portfolio.is_completely_flat())

    def test_flatten_position(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        # Wire strategy into system
        self.data_engine.register_strategy(strategy)
        self.exec_engine.register_strategy(strategy)

        order = strategy.order_factory.market(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        strategy.submit_order(order)

        position = self.cache.positions_open()[0]

        # Act
        strategy.flatten_position(position)

        # Assert
        self.assertEqual(OrderState.FILLED, order.state)
        self.assertTrue(strategy.portfolio.is_completely_flat())

    def test_flatten_all_positions(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER-000"),
            self.clock,
            self.logger,
        )

        # Wire strategy into system
        self.data_engine.register_strategy(strategy)
        self.exec_engine.register_strategy(strategy)

        # Start strategy and submit orders to open positions
        strategy.start()

        order1 = strategy.order_factory.market(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        order2 = strategy.order_factory.market(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        strategy.submit_order(order1)
        strategy.submit_order(order2)

        # Act
        strategy.flatten_all_positions(USDJPY_SIM.id)

        # Assert
        self.assertEqual(OrderState.FILLED, order1.state)
        self.assertEqual(OrderState.FILLED, order2.state)
        self.assertTrue(strategy.portfolio.is_completely_flat())
