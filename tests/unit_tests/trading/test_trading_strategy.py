# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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
import time
import unittest

import pytz

from nautilus_trader.analysis.performance import PerformanceAnalyzer
from nautilus_trader.backtest.config import BacktestConfig
from nautilus_trader.backtest.execution import BacktestExecClient
from nautilus_trader.backtest.logging import TestLogger
from nautilus_trader.backtest.market import SimulatedMarket
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.market import GenericCommissionModel
from nautilus_trader.common.portfolio import Portfolio
from nautilus_trader.common.uuid import TestUUIDFactory
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.execution.database import BypassExecutionDatabase
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.model.bar import Bar
from nautilus_trader.model.enums import ComponentState
from nautilus_trader.model.enums import Maker
from nautilus_trader.model.enums import OMSType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderState
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.identifiers import MatchId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.position import Position
from nautilus_trader.model.tick import QuoteTick
from nautilus_trader.model.tick import TradeTick
from nautilus_trader.trading.strategy import TradingStrategy
from tests.test_kit.strategies import TestStrategy1
from tests.test_kit.stubs import TestStubs

USDJPY_FXCM = Symbol('USD/JPY', Venue('FXCM'))
AUDUSD_FXCM = Symbol('AUD/USD', Venue('FXCM'))


class TradingStrategyTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.clock = TestClock()
        self.uuid_factory = TestUUIDFactory()
        self.logger = TestLogger(self.clock)

        self.data_engine = DataEngine(
            tick_capacity=1000,
            bar_capacity=1000,
            clock=self.clock,
            uuid_factory=self.uuid_factory,
            logger=self.logger,
        )

        self.data_engine.set_use_previous_close(False)

        self.portfolio = Portfolio(
            clock=self.clock,
            uuid_factory=self.uuid_factory,
            logger=self.logger,
        )

        self.analyzer = PerformanceAnalyzer()

        trader_id = TraderId('TESTER', '000')
        account_id = TestStubs.account_id()

        self.exec_db = BypassExecutionDatabase(
            trader_id=trader_id,
            logger=self.logger,
        )

        self.exec_engine = ExecutionEngine(
            database=self.exec_db,
            portfolio=self.portfolio,
            clock=self.clock,
            uuid_factory=self.uuid_factory,
            logger=self.logger,
        )

        usdjpy = TestStubs.instrument_usdjpy()

        self.market = SimulatedMarket(
            venue=Venue("FXCM"),
            oms_type=OMSType.HEDGING,
            generate_position_ids=True,
            exec_cache=self.exec_engine.cache,
            instruments={usdjpy.symbol: usdjpy},
            config=BacktestConfig(),
            fill_model=FillModel(),
            commission_model=GenericCommissionModel(),
            clock=self.clock,
            uuid_factory=TestUUIDFactory(),
            logger=self.logger,
        )

        self.exec_client = BacktestExecClient(
            market=self.market,
            account_id=account_id,
            engine=self.exec_engine,
            logger=self.logger,
        )

        self.exec_engine.register_client(self.exec_client)
        self.market.register_client(self.exec_client)
        self.exec_engine.process(TestStubs.account_event())

        self.market.process_tick(TestStubs.quote_tick_3decimal(usdjpy.symbol))  # Prepare market

        self.strategy = TradingStrategy(order_id_tag="001")
        self.strategy.register_trader(
            trader_id=TraderId("TESTER", "000"),
            clock=self.clock,
            uuid_factory=self.uuid_factory,
            logger=self.logger,
        )

        self.strategy.register_data_engine(self.data_engine)
        self.strategy.register_execution_engine(self.exec_engine)

        print("\n")

    def test_strategy_equality(self):
        # Arrange
        strategy1 = TradingStrategy(order_id_tag="001")
        strategy2 = TradingStrategy(order_id_tag="AUD/USD-001")
        strategy3 = TradingStrategy(order_id_tag="AUD/USD-002")

        # Act
        result1 = strategy1 == strategy1
        result2 = strategy1 == strategy2
        result3 = strategy2 == strategy3
        result4 = strategy1 != strategy1
        result5 = strategy1 != strategy2
        result6 = strategy2 != strategy3

        # Assert
        self.assertTrue(result1)
        self.assertFalse(result2)
        self.assertFalse(result3)
        self.assertFalse(result4)
        self.assertTrue(result5)
        self.assertTrue(result6)

    def test_strategy_is_hashable(self):
        # Arrange
        # Act
        result = self.strategy.__hash__()

        # Assert
        # If this passes then result must be an int
        self.assertTrue(result != 0)

    def test_strategy_str_and_repr(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="GBP/USD-MM")

        # Act
        result1 = str(strategy)
        result2 = repr(strategy)

        # Assert
        self.assertEqual("TradingStrategy(TradingStrategy-GBP/USD-MM)", result1)
        self.assertTrue(result2.startswith("<TradingStrategy(TradingStrategy-GBP/USD-MM) object at"))
        self.assertTrue(result2.endswith(">"))

    def test_get_strategy_id(self):
        # Arrange
        # Act
        # Assert
        self.assertEqual(StrategyId("TradingStrategy", "001"), self.strategy.id)

    def test_get_current_time(self):
        # Arrange
        # Act
        result = self.strategy.clock.utc_now()

        # Assert
        self.assertEqual(pytz.utc, result.tzinfo)

    def test_initialization(self):
        # Arrange
        bar_type = TestStubs.bartype_gbpusd_1sec_mid()
        strategy = TestStrategy1(bar_type)

        # Act
        # Assert
        self.assertFalse(strategy.indicators_initialized())

    def test_get_tick_count_for_unknown_symbol_returns_zero(self):
        # Arrange
        # Act
        result = self.strategy.quote_tick_count(AUDUSD_FXCM)

        # Assert
        self.assertEqual(0, result)

    def test_get_ticks_for_unknown_symbol_raises_exception(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ValueError, self.strategy.quote_ticks, AUDUSD_FXCM)

    def test_get_bar_count_for_unknown_bar_type_returns_zero(self):
        # Arrange
        bar_type = TestStubs.bartype_gbpusd_1sec_mid()

        # Act
        result = self.strategy.bar_count(bar_type)

        # Assert
        self.assertEqual(0, result)

    def test_get_bars_for_unknown_bar_type_raises_exception(self):
        # Arrange
        bar_type = TestStubs.bartype_gbpusd_1sec_mid()

        # Act
        # Assert
        self.assertRaises(ValueError, self.strategy.bars, bar_type)

    def test_bars(self):
        # Arrange
        bar_type = TestStubs.bartype_gbpusd_1sec_mid()
        bar = Bar(
            Price("1.00001"),
            Price("1.00004"),
            Price("1.00002"),
            Price("1.00003"),
            Quantity(100000),
            datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc),
        )

        self.data_engine.handle_bar(bar_type, bar)

        # Act
        result = self.strategy.bars(bar_type)

        # Assert
        self.assertTrue(bar, result[0])

    def test_getting_bar_for_unknown_bar_type_raises_exception(self):
        # Arrange
        unknown_bar_type = TestStubs.bartype_gbpusd_1sec_mid()

        # Act
        # Assert
        self.assertRaises(ValueError, self.strategy.bar, unknown_bar_type, 0)

    def test_getting_bar_at_out_of_range_index_raises_exception(self):
        # Arrange
        bar_type = TestStubs.bartype_gbpusd_1sec_mid()
        bar = Bar(
            Price("1.00001"),
            Price("1.00004"),
            Price("1.00002"),
            Price("1.00003"),
            Quantity(100000),
            datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc),
        )

        self.data_engine.handle_bar(bar_type, bar)

        # Act
        # Assert
        self.assertRaises(IndexError, self.strategy.bar, bar_type, -2)

    def test_get_bar(self):
        bar_type = TestStubs.bartype_gbpusd_1sec_mid()
        bar = Bar(
            Price("1.00001"),
            Price("1.00004"),
            Price("1.00002"),
            Price("1.00003"),
            Quantity(100000),
            datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc),
        )

        self.data_engine.handle_bar(bar_type, bar)

        # Act
        result = self.strategy.bar(bar_type, 0)

        # Assert
        self.assertEqual(bar, result)

    def test_getting_tick_with_unknown_tick_type_raises_exception(self):
        # Act
        # Assert
        self.assertRaises(ValueError, self.strategy.quote_tick, AUDUSD_FXCM, 0)

    def test_get_quote_tick(self):
        tick = QuoteTick(
            AUDUSD_FXCM,
            Price("1.00000"),
            Price("1.00001"),
            Quantity(1),
            Quantity(1),
            datetime(2018, 1, 1, 19, 59, 1, 0, pytz.utc),
        )

        self.data_engine.handle_quote_tick(tick)

        # Act
        result = self.strategy.quote_tick(tick.symbol, 0)

        # Assert
        self.assertEqual(tick, result)

    def test_get_trade_tick(self):
        tick = TradeTick(
            AUDUSD_FXCM,
            Price("1.00000"),
            Quantity(10000),
            Maker.BUYER,
            MatchId("123456789"),
            datetime(2018, 1, 1, 19, 59, 1, 0, pytz.utc),
        )

        self.data_engine.handle_trade_tick(tick)

        # Act
        result = self.strategy.trade_tick(tick.symbol, 0)

        # Assert
        self.assertEqual(tick, result)

    def test_start_strategy(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = TestStrategy1(bar_type)
        strategy.register_trader(
            TraderId("TESTER", "000"),
            clock=self.clock,
            uuid_factory=self.uuid_factory,
            logger=self.logger,
        )
        self.data_engine.register_strategy(strategy)
        self.exec_engine.register_strategy(strategy)

        result1 = strategy.state()

        # Act
        strategy.start()
        result2 = strategy.state()

        # Assert
        self.assertEqual(ComponentState.INITIALIZED, result1)
        self.assertEqual(ComponentState.RUNNING, result2)
        self.assertTrue("custom start logic" in strategy.object_storer.get_store())

    def test_stop_strategy(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = TestStrategy1(bar_type)
        strategy.register_trader(
            TraderId("TESTER", "000"),
            clock=self.clock,
            uuid_factory=self.uuid_factory,
            logger=self.logger,
        )
        self.data_engine.register_strategy(strategy)
        self.exec_engine.register_strategy(strategy)

        # Act
        strategy.start()
        strategy.stop()

        # Assert
        self.assertEqual(ComponentState.STOPPED, strategy.state())
        self.assertTrue("custom stop logic" in strategy.object_storer.get_store())

    def test_reset_strategy(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = TestStrategy1(bar_type)
        strategy.register_trader(
            TraderId("TESTER", "000"),
            clock=self.clock,
            uuid_factory=self.uuid_factory,
            logger=self.logger,
        )
        bar_type = TestStubs.bartype_gbpusd_1sec_mid()

        bar = Bar(
            Price("1.00001"),
            Price("1.00004"),
            Price("1.00002"),
            Price("1.00003"),
            Quantity(100000),
            datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc),
        )

        strategy.handle_bar(bar_type, bar)

        # Act
        strategy.reset()

        # Assert
        self.assertEqual(ComponentState.INITIALIZED, strategy.state())
        self.assertEqual(0, strategy.ema1.count)
        self.assertEqual(0, strategy.ema2.count)
        self.assertTrue("custom reset logic" in strategy.object_storer.get_store())

    def test_register_indicator_with_strategy(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = TestStrategy1(bar_type)
        strategy.register_trader(
            TraderId("TESTER", "000"),
            clock=self.clock,
            uuid_factory=self.uuid_factory,
            logger=self.logger,
        )

        # Act
        result = strategy.registered_indicators()

        # Assert
        self.assertEqual([strategy.ema1, strategy.ema2], result)

    def test_register_strategy_with_exec_client(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            clock=self.clock,
            uuid_factory=self.uuid_factory,
            logger=self.logger,
        )

        # Act
        self.exec_engine.register_strategy(strategy)

        # Assert
        self.assertIsNotNone(strategy.execution)

    def test_stopping_a_strategy_cancels_a_running_time_alert(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = TestStrategy1(bar_type)
        strategy.register_trader(
            TraderId("TESTER", "000"),
            clock=self.clock,
            uuid_factory=self.uuid_factory,
            logger=self.logger,
        )
        self.data_engine.register_strategy(strategy)
        self.exec_engine.register_strategy(strategy)

        alert_time = datetime.now(pytz.utc) + timedelta(milliseconds=200)
        strategy.clock.set_time_alert("test_alert1", alert_time)

        # Act
        strategy.start()
        time.sleep(0.1)
        strategy.stop()

        # Assert
        self.assertEqual(2, strategy.object_storer.count)

    def test_stopping_a_strategy_cancels_a_running_timer(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = TestStrategy1(bar_type)
        strategy.register_trader(
            TraderId("TESTER", "000"),
            clock=self.clock,
            uuid_factory=self.uuid_factory,
            logger=self.logger,
        )
        self.data_engine.register_strategy(strategy)
        self.exec_engine.register_strategy(strategy)

        start_time = datetime.now(pytz.utc) + timedelta(milliseconds=100)
        strategy.clock.set_timer("test_timer3", timedelta(milliseconds=100), start_time, stop_time=None)

        # Act
        strategy.start()
        time.sleep(0.1)
        strategy.stop()

        # Assert
        self.assertEqual(2, strategy.object_storer.count)

    def test_strategy_can_submit_order(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            clock=self.clock,
            uuid_factory=self.uuid_factory,
            logger=self.logger,
        )
        self.exec_engine.register_strategy(strategy)

        order = strategy.order_factory.market(
            USDJPY_FXCM,
            OrderSide.BUY,
            Quantity(100000),
        )

        # Act
        strategy.submit_order(order)

        # Assert
        self.assertTrue(order in strategy.execution.orders())
        self.assertEqual(OrderState.FILLED, strategy.execution.orders()[0].state())
        self.assertTrue(order.cl_ord_id not in strategy.execution.orders_working())
        self.assertFalse(strategy.execution.is_order_working(order.cl_ord_id))
        self.assertTrue(strategy.execution.is_order_completed(order.cl_ord_id))

    def test_cancel_order(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            clock=self.clock,
            uuid_factory=self.uuid_factory,
            logger=self.logger,
        )
        self.exec_engine.register_strategy(strategy)

        order = strategy.order_factory.stop(
            USDJPY_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price("90.005"),
        )

        strategy.submit_order(order)

        # Act
        strategy.cancel_order(order)

        # Assert
        self.assertTrue(order in strategy.execution.orders())
        self.assertEqual(OrderState.CANCELLED, strategy.execution.orders()[0].state())
        self.assertEqual(order.cl_ord_id, strategy.execution.orders_completed()[0].cl_ord_id)
        self.assertTrue(order.cl_ord_id not in strategy.execution.orders_working())
        self.assertTrue(strategy.execution.order_exists(order.cl_ord_id))
        self.assertFalse(strategy.execution.is_order_working(order.cl_ord_id))
        self.assertTrue(strategy.execution.is_order_completed(order.cl_ord_id))

    def test_modify_order(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            clock=self.clock,
            uuid_factory=self.uuid_factory,
            logger=self.logger)
        self.exec_engine.register_strategy(strategy)

        order = strategy.order_factory.limit(
            USDJPY_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price("90.001"),
        )

        strategy.submit_order(order)

        # Act
        strategy.modify_order(order, Quantity(110000), Price("90.002"))

        # Assert
        self.assertEqual(order, strategy.execution.orders()[0])
        self.assertEqual(OrderState.WORKING, strategy.execution.orders()[0].state())
        self.assertEqual(Quantity(110000), strategy.execution.orders()[0].quantity)
        self.assertEqual(Price("90.002"), strategy.execution.orders()[0].price)
        self.assertTrue(strategy.execution.is_flat())
        self.assertTrue(strategy.execution.order_exists(order.cl_ord_id))
        self.assertTrue(strategy.execution.is_order_working(order.cl_ord_id))
        self.assertFalse(strategy.execution.is_order_completed(order.cl_ord_id))

    def test_cancel_all_orders(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            clock=self.clock,
            uuid_factory=self.uuid_factory,
            logger=self.logger)
        self.exec_engine.register_strategy(strategy)

        order1 = strategy.order_factory.stop(
            USDJPY_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price("90.003"),
        )

        order2 = strategy.order_factory.stop(
            USDJPY_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price("90.005"),
        )

        strategy.submit_order(order1)
        strategy.submit_order(order2)

        # Act
        strategy.cancel_all_orders(USDJPY_FXCM)

        # Assert
        self.assertTrue(order1 in strategy.execution.orders())
        self.assertTrue(order2 in strategy.execution.orders())
        self.assertEqual(OrderState.CANCELLED, strategy.execution.orders()[0].state())
        self.assertEqual(OrderState.CANCELLED, strategy.execution.orders()[1].state())
        self.assertTrue(order1 in strategy.execution.orders_completed())
        self.assertTrue(order2 in strategy.execution.orders_completed())

    def test_flatten_position(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            clock=self.clock,
            uuid_factory=self.uuid_factory,
            logger=self.logger,
        )
        self.exec_engine.register_strategy(strategy)

        order = strategy.order_factory.market(
            USDJPY_FXCM,
            OrderSide.BUY,
            Quantity(100000),
        )

        strategy.submit_order(order)

        filled = TestStubs.event_order_filled(
            order,
            position_id=PositionId("B-USD/JPY-1"),
            strategy_id=strategy.id,
        )
        position = Position(filled)

        # Act
        strategy.flatten_position(position)

        # Assert
        self.assertTrue(order in strategy.execution.orders())
        self.assertEqual(OrderState.FILLED, strategy.execution.orders()[0].state())
        self.assertEqual(PositionSide.FLAT, strategy.execution.positions()[0].side)
        self.assertTrue(strategy.execution.positions()[0].is_closed())
        self.assertTrue(PositionId("B-USD/JPY-1") in strategy.execution.position_closed_ids())
        self.assertTrue(strategy.execution.is_completely_flat())

    def test_flatten_all_positions(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            clock=self.clock,
            uuid_factory=self.uuid_factory,
            logger=self.logger,
        )
        self.exec_engine.register_strategy(strategy)

        order1 = strategy.order_factory.market(
            USDJPY_FXCM,
            OrderSide.BUY,
            Quantity(100000),
        )

        order2 = strategy.order_factory.market(
            USDJPY_FXCM,
            OrderSide.BUY,
            Quantity(100000),
        )

        strategy.submit_order(order1)
        strategy.submit_order(order2)

        filled1 = TestStubs.event_order_filled(
            order1,
            position_id=PositionId("B-USD/JPY-1"),
            strategy_id=strategy.id,
        )

        filled2 = TestStubs.event_order_filled(
            order2,
            position_id=PositionId("B-USD/JPY-2"),
            strategy_id=strategy.id,
        )

        position1 = Position(filled1)
        position2 = Position(filled2)

        # Act
        strategy.flatten_all_positions(USDJPY_FXCM)

        # Assert
        self.assertTrue(order1 in strategy.execution.orders())
        self.assertTrue(order2 in strategy.execution.orders())
        self.assertEqual(OrderState.FILLED, strategy.execution.orders()[0].state())
        self.assertEqual(OrderState.FILLED, strategy.execution.orders()[1].state())
        self.assertEqual(PositionSide.FLAT, strategy.execution.positions()[0].side)
        self.assertEqual(PositionSide.FLAT, strategy.execution.positions()[1].side)
        self.assertTrue(position1.id in strategy.execution.position_closed_ids())
        self.assertTrue(position2.id in strategy.execution.position_closed_ids())
        self.assertTrue(strategy.execution.is_completely_flat())

    def test_update_indicators(self):
        # Arrange
        bar_type = TestStubs.bartype_gbpusd_1sec_mid()
        strategy = TestStrategy1(bar_type)
        strategy.register_trader(
            TraderId("TESTER", "000"),
            clock=self.clock,
            uuid_factory=self.uuid_factory,
            logger=self.logger,
        )

        bar = Bar(
            Price("1.00001"),
            Price("1.00004"),
            Price("1.00002"),
            Price("1.00003"),
            Quantity(100000),
            datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc),
        )

        # Act
        strategy.handle_bar(bar_type, bar)

        # Assert
        self.assertEqual(1, strategy.ema1.count)
        self.assertEqual(1, strategy.ema2.count)

    def test_can_track_orders_for_an_opened_position(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = TestStrategy1(bar_type)
        strategy.register_trader(
            TraderId("TESTER", "000"),
            clock=self.clock,
            uuid_factory=self.uuid_factory,
            logger=self.logger,
        )
        self.exec_engine.register_strategy(strategy)

        order = strategy.order_factory.market(
            USDJPY_FXCM,
            OrderSide.BUY,
            Quantity(100000),
        )

        strategy.submit_order(order)

        # Act
        # Assert
        self.assertTrue(order in strategy.execution.orders())
        self.assertTrue(PositionId("B-USD/JPY-1") in strategy.execution.position_ids())
        self.assertEqual(0, len(strategy.execution.orders_working()))
        self.assertTrue(order in strategy.execution.orders_completed())
        self.assertEqual(0, len(strategy.execution.positions_closed()))
        self.assertTrue(order in strategy.execution.orders_completed())
        self.assertTrue(PositionId("B-USD/JPY-1") in strategy.execution.position_open_ids())
        self.assertFalse(strategy.execution.is_completely_flat())

    def test_can_track_orders_for_a_closing_position(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = TestStrategy1(bar_type)
        strategy.register_trader(
            TraderId("TESTER", "000"),
            clock=self.clock,
            uuid_factory=self.uuid_factory,
            logger=self.logger,
        )
        self.exec_engine.register_strategy(strategy)

        order1 = strategy.order_factory.market(
            USDJPY_FXCM,
            OrderSide.BUY,
            Quantity(100000),
        )

        order2 = strategy.order_factory.market(
            USDJPY_FXCM,
            OrderSide.SELL,
            Quantity(100000),
        )

        strategy.submit_order(order1)
        strategy.submit_order(order2, PositionId("B-USD/JPY-1"))  # Position identifier generated by exchange

        # Act
        print(self.exec_engine.cache.orders())
        # Assert
        self.assertEqual(0, len(self.exec_engine.cache.orders_working()))
        self.assertTrue(order1 in self.exec_engine.cache.orders_completed())
        self.assertTrue(order2 in self.exec_engine.cache.orders_completed())
        self.assertEqual(1, len(self.exec_engine.cache.positions_closed()))
        self.assertEqual(0, len(self.exec_engine.cache.positions_open()))
        self.assertTrue(self.exec_engine.cache.is_completely_flat())
