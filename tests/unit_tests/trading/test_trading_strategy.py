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
from nautilus_trader.backtest.exchange import SimulatedExchange
from nautilus_trader.backtest.execution import BacktestExecClient
from nautilus_trader.backtest.loaders import InstrumentLoader
from nautilus_trader.backtest.logging import TestLogger
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.enums import ComponentState
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.execution.database import BypassExecutionDatabase
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.model.bar import Bar
from nautilus_trader.model.enums import OMSType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderState
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.trading.portfolio import Portfolio
from nautilus_trader.trading.strategy import TradingStrategy
from tests.test_kit.strategies import TestStrategy
from tests.test_kit.stubs import TestStubs


USDJPY_FXCM = Symbol('USD/JPY', Venue('FXCM'))
AUDUSD_FXCM = Symbol('AUD/USD', Venue('FXCM'))


class TradingStrategyTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.clock = TestClock()
        self.uuid_factory = UUIDFactory()
        self.logger = TestLogger(self.clock)

        self.portfolio = Portfolio(
            clock=self.clock,
            uuid_factory=self.uuid_factory,
            logger=self.logger,
        )

        self.data_engine = DataEngine(
            portfolio=self.portfolio,
            clock=self.clock,
            uuid_factory=self.uuid_factory,
            logger=self.logger,
            config={'use_previous_close': False},  # To correctly reproduce historical data bars
        )
        self.portfolio.register_cache(self.data_engine.cache)

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

        usdjpy = InstrumentLoader.default_fx_ccy(TestStubs.symbol_usdjpy_fxcm())

        self.market = SimulatedExchange(
            venue=Venue("FXCM"),
            oms_type=OMSType.HEDGING,
            generate_position_ids=True,
            exec_cache=self.exec_engine.cache,
            instruments={usdjpy.symbol: usdjpy},
            config=BacktestConfig(),
            fill_model=FillModel(),
            clock=self.clock,
            uuid_factory=UUIDFactory(),
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
        self.exec_engine.process(TestStubs.event_account_state())

        self.market.process_tick(TestStubs.quote_tick_3decimal(usdjpy.symbol))  # Prepare market

    def test_strategy_equality(self):
        # Arrange
        strategy1 = TradingStrategy(order_id_tag="001")
        strategy2 = TradingStrategy(order_id_tag="AUD/USD-001")
        strategy3 = TradingStrategy(order_id_tag="AUD/USD-002")

        # Act
        # Assert
        self.assertTrue(strategy1 == strategy1)
        self.assertFalse(strategy1 == strategy2)
        self.assertFalse(strategy2 == strategy3)
        self.assertFalse(strategy1 != strategy1)
        self.assertTrue(strategy1 != strategy2)
        self.assertTrue(strategy2 != strategy3)

    def test_str_and_repr(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="GBP/USD-MM")

        # Act
        # Assert
        self.assertEqual("TradingStrategy(id=TradingStrategy-GBP/USD-MM)", str(strategy))
        self.assertEqual("TradingStrategy(id=TradingStrategy-GBP/USD-MM)", repr(strategy))

    def test_get_strategy_id(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="001")

        # Act
        # Assert
        self.assertEqual(StrategyId.from_string("TradingStrategy-001"), strategy.id)

    def test_initialization(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="001")

        # Act
        # Assert
        self.assertTrue(ComponentState.INITIALIZED, strategy.state)
        self.assertEqual([], strategy.registered_indicators())
        self.assertFalse(strategy.indicators_initialized())

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

    def test_registered_indicators(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = TestStrategy(bar_type)
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

    def test_start(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = TestStrategy(bar_type)
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

        # Assert
        self.assertEqual(ComponentState.RUNNING, strategy.state)
        self.assertIn("custom start logic", strategy.object_storer.get_store())

    def test_stop(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = TestStrategy(bar_type)
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
        self.assertEqual(ComponentState.STOPPED, strategy.state)
        self.assertIn("custom stop logic", strategy.object_storer.get_store())

    def test_resume(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = TestStrategy(bar_type)
        strategy.register_trader(
            TraderId("TESTER", "000"),
            clock=self.clock,
            uuid_factory=self.uuid_factory,
            logger=self.logger,
        )
        self.data_engine.register_strategy(strategy)
        self.exec_engine.register_strategy(strategy)

        strategy.start()
        strategy.stop()

        # Act
        strategy.resume()

        # Assert
        self.assertEqual(ComponentState.RUNNING, strategy.state)
        self.assertIn("custom start logic", strategy.object_storer.get_store())

    def test_reset(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = TestStrategy(bar_type)
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
        self.assertEqual(ComponentState.INITIALIZED, strategy.state)
        self.assertEqual(0, strategy.ema1.count)
        self.assertEqual(0, strategy.ema2.count)
        self.assertIn("custom reset logic", strategy.object_storer.get_store())

    def test_dispose(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = TestStrategy(bar_type)
        strategy.register_trader(
            TraderId("TESTER", "000"),
            clock=self.clock,
            uuid_factory=self.uuid_factory,
            logger=self.logger,
        )

        strategy.reset()

        # Act
        strategy.dispose()

        # Assert
        self.assertEqual(ComponentState.DISPOSED, strategy.state)

    def test_handle_bar_updates_indicators(self):
        # Arrange
        bar_type = TestStubs.bartype_gbpusd_1sec_mid()
        strategy = TestStrategy(bar_type)
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

    def test_stop_cancels_a_running_time_alert(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = TestStrategy(bar_type)
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

    def test_stop_cancels_a_running_timer(self):
        # Arrange
        bar_type = TestStubs.bartype_audusd_1min_bid()
        strategy = TestStrategy(bar_type)
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

    def test_submit_order_with_valid_order_successfully_submits(self):
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
        self.assertIn(order, strategy.execution.orders())
        self.assertEqual(OrderState.FILLED, strategy.execution.orders()[0].state)
        self.assertNotIn(order.cl_ord_id, strategy.execution.orders_working())
        self.assertFalse(strategy.execution.is_order_working(order.cl_ord_id))
        self.assertTrue(strategy.execution.is_order_completed(order.cl_ord_id))

    def test_submit_bracket_order_with_valid_order_successfully_submits(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            clock=self.clock,
            uuid_factory=self.uuid_factory,
            logger=self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        entry = strategy.order_factory.stop_market(
            USDJPY_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            price=Price("90.100"),
        )

        order = strategy.order_factory.bracket(
            entry_order=entry,
            stop_loss=Price("90.000"),
            take_profit=Price("90.500"),
        )

        # Act
        strategy.submit_bracket_order(order)

        # Assert
        self.assertIn(entry, strategy.execution.orders())
        self.assertEqual(OrderState.WORKING, entry.state)
        self.assertIn(entry, strategy.execution.orders_working())
        self.assertTrue(strategy.execution.is_order_working(entry.cl_ord_id))
        self.assertFalse(strategy.execution.is_order_completed(entry.cl_ord_id))

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

        order = strategy.order_factory.stop_market(
            USDJPY_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price("90.005"),
        )

        strategy.submit_order(order)

        # Act
        strategy.cancel_order(order)

        # Assert
        self.assertIn(order, strategy.execution.orders())
        self.assertEqual(OrderState.CANCELLED, strategy.execution.orders()[0].state)
        self.assertEqual(order.cl_ord_id, strategy.execution.orders_completed()[0].cl_ord_id)
        self.assertNotIn(order.cl_ord_id, strategy.execution.orders_working())
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
        self.assertEqual(OrderState.WORKING, strategy.execution.orders()[0].state)
        self.assertEqual(Quantity(110000), strategy.execution.orders()[0].quantity)
        self.assertEqual(Price("90.002"), strategy.execution.orders()[0].price)
        self.assertTrue(strategy.execution.order_exists(order.cl_ord_id))
        self.assertTrue(strategy.execution.is_order_working(order.cl_ord_id))
        self.assertFalse(strategy.execution.is_order_completed(order.cl_ord_id))
        self.assertTrue(strategy.portfolio.is_flat(order.symbol))

    def test_cancel_all_orders(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            clock=self.clock,
            uuid_factory=self.uuid_factory,
            logger=self.logger)
        self.exec_engine.register_strategy(strategy)

        order1 = strategy.order_factory.stop_market(
            USDJPY_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price("90.003"),
        )

        order2 = strategy.order_factory.stop_market(
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
        self.assertIn(order1, strategy.execution.orders())
        self.assertIn(order2, strategy.execution.orders())
        self.assertEqual(OrderState.CANCELLED, strategy.execution.orders()[0].state)
        self.assertEqual(OrderState.CANCELLED, strategy.execution.orders()[1].state)
        self.assertIn(order1, strategy.execution.orders_completed())
        self.assertIn(order2, strategy.execution.orders_completed())

    def test_flatten_position(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            clock=self.clock,
            uuid_factory=self.uuid_factory,
            logger=self.logger,
        )

        # Wire strategy into system
        self.data_engine.register_strategy(strategy)
        self.exec_engine.register_strategy(strategy)

        order = strategy.order_factory.market(
            USDJPY_FXCM,
            OrderSide.BUY,
            Quantity(100000),
        )

        strategy.submit_order(order)

        position = strategy.execution.positions_open()[0]

        # Act
        strategy.flatten_position(position)

        # Assert
        self.assertEqual(OrderState.FILLED, order.state)
        self.assertTrue(strategy.portfolio.is_completely_flat())

    def test_flatten_all_positions(self):
        # Arrange
        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            clock=self.clock,
            uuid_factory=self.uuid_factory,
            logger=self.logger,
        )

        # Wire strategy into system
        self.data_engine.register_strategy(strategy)
        self.exec_engine.register_strategy(strategy)

        # Start strategy and submit orders to open positions
        strategy.start()

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

        # Act
        strategy.flatten_all_positions(USDJPY_FXCM)

        # Assert
        self.assertEqual(OrderState.FILLED, order1.state)
        self.assertEqual(OrderState.FILLED, order2.state)
        self.assertTrue(strategy.portfolio.is_completely_flat())
