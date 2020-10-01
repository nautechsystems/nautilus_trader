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

import unittest

import pandas as pd

from nautilus_trader.analysis.performance import PerformanceAnalyzer
from nautilus_trader.backtest.config import BacktestConfig
from nautilus_trader.backtest.execution import BacktestExecClient
from nautilus_trader.backtest.logging import TestLogger
from nautilus_trader.backtest.market import SimulatedMarket
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.market import MakerTakerCommissionModel
from nautilus_trader.common.portfolio import Portfolio
from nautilus_trader.common.uuid import TestUUIDFactory
from nautilus_trader.core.functions import basis_points_as_percentage
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.execution.database import InMemoryExecutionDatabase
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.events import OrderModified
from nautilus_trader.model.events import OrderRejected
from nautilus_trader.model.events import OrderWorking
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.tick import QuoteTick
from tests.test_kit.data import TestDataProvider
from tests.test_kit.strategies import TestStrategy1
from tests.test_kit.stubs import TestStubs
from tests.test_kit.stubs import UNIX_EPOCH

USDJPY_FXCM = TestStubs.symbol_usdjpy_fxcm()


class SimulatedMarketTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.usdjpy = TestStubs.instrument_usdjpy()
        self.bid_data_1min = TestDataProvider.usdjpy_1min_bid()[:2000]
        self.ask_data_1min = TestDataProvider.usdjpy_1min_ask()[:2000]

        self.data_ticks = {self.usdjpy.symbol: pd.DataFrame()}
        self.data_bars_bid = {self.usdjpy.symbol: self.bid_data_1min}
        self.data_bars_ask = {self.usdjpy.symbol: self.ask_data_1min}

        self.strategies = [TestStrategy1(TestStubs.bartype_usdjpy_1min_bid())]

        self.clock = TestClock()
        self.uuid_factory = TestUUIDFactory()
        self.logger = TestLogger(self.clock)

        self.data_engine = DataEngine(
            tick_capacity=1000,
            bar_capacity=1000,
            clock=self.clock,
            uuid_factory=self.uuid_factory,
            logger=self.logger)
        self.data_engine.set_use_previous_close(False)

        self.portfolio = Portfolio(
            clock=self.clock,
            uuid_factory=self.uuid_factory,
            logger=self.logger)

        self.analyzer = PerformanceAnalyzer()

        self.trader_id = TraderId("TESTER", "000")
        account_id = TestStubs.account_id()

        self.exec_db = InMemoryExecutionDatabase(
            trader_id=self.trader_id,
            logger=self.logger)
        self.exec_engine = ExecutionEngine(
            trader_id=self.trader_id,
            account_id=account_id,
            database=self.exec_db,
            portfolio=self.portfolio,
            clock=self.clock,
            uuid_factory=self.uuid_factory,
            logger=self.logger)

        self.config = BacktestConfig()
        self.market = SimulatedMarket(
            exec_engine=self.exec_engine,
            instruments={self.usdjpy.symbol: self.usdjpy},
            config=self.config,
            fill_model=FillModel(),
            commission_model=MakerTakerCommissionModel(),
            clock=self.clock,
            uuid_factory=TestUUIDFactory(),
            logger=self.logger)

        self.exec_client = BacktestExecClient(
            market=self.market,
            logger=self.logger)

        self.exec_engine.register_client(self.exec_client)

    def test_account_collateral_inquiry(self):
        # Arrange
        strategy = TestStrategy1(bar_type=TestStubs.bartype_usdjpy_1min_bid())
        strategy.register_trader(
            self.trader_id,
            self.clock,
            self.uuid_factory,
            self.logger)
        self.exec_engine.register_strategy(strategy)

        # Act
        strategy.account_inquiry()

        # Assert
        self.assertEqual(1, len(strategy.account().get_events()))

    def test_submit_market_order(self):
        # Arrange
        strategy = TestStrategy1(bar_type=TestStubs.bartype_usdjpy_1min_bid())
        strategy.register_trader(
            self.trader_id,
            self.clock,
            self.uuid_factory,
            self.logger)
        self.data_engine.register_strategy(strategy)
        self.exec_engine.register_strategy(strategy)
        strategy.start()

        self.market.process_tick(TestStubs.quote_tick_3decimal(self.usdjpy.symbol))  # Prepare market
        order = strategy.order_factory.market(
            USDJPY_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        # Act
        strategy.submit_order(order)

        # Assert
        self.assertEqual(5, strategy.object_storer.count)
        self.assertTrue(isinstance(strategy.object_storer.get_store()[3], OrderFilled))
        self.assertEqual(Price(90.003, 3), self.exec_engine.database.order(order.cl_ord_id).avg_price)

    def test_submit_limit_order(self):
        # Arrange
        strategy = TestStrategy1(bar_type=TestStubs.bartype_usdjpy_1min_bid())
        strategy.register_trader(
            self.trader_id,
            self.clock,
            self.uuid_factory,
            self.logger)
        self.data_engine.register_strategy(strategy)
        self.exec_engine.register_strategy(strategy)
        strategy.start()

        self.market.process_tick(TestStubs.quote_tick_3decimal(self.usdjpy.symbol))  # Prepare market
        order = strategy.order_factory.limit(
            USDJPY_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price(80.000, 3))

        # Act
        strategy.submit_order(order)

        # Assert
        self.assertEqual(4, strategy.object_storer.count)
        self.assertTrue(isinstance(strategy.object_storer.get_store()[3], OrderWorking))
        self.assertEqual(Price(80.000, 3), order.price)

    def test_submit_bracket_market_order(self):
        # Arrange
        strategy = TestStrategy1(bar_type=TestStubs.bartype_usdjpy_1min_bid())
        strategy.register_trader(
            self.trader_id,
            self.clock,
            self.uuid_factory,
            self.logger)
        self.data_engine.register_strategy(strategy)
        self.exec_engine.register_strategy(strategy)
        strategy.start()

        self.market.process_tick(TestStubs.quote_tick_3decimal(self.usdjpy.symbol))  # Prepare market

        entry_order = strategy.order_factory.market(
            USDJPY_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        bracket_order = strategy.order_factory.bracket(
            entry_order,
            Price(80.000, 3))

        # Act
        strategy.submit_bracket_order(bracket_order)

        # Assert
        self.assertEqual(8, strategy.object_storer.count)
        self.assertTrue(isinstance(strategy.object_storer.get_store()[4], OrderFilled))
        self.assertEqual(Price(80.000, 3), bracket_order.stop_loss.price)

    def test_submit_bracket_stop_order(self):
        # Arrange
        strategy = TestStrategy1(bar_type=TestStubs.bartype_usdjpy_1min_bid())
        strategy.register_trader(
            self.trader_id,
            self.clock,
            self.uuid_factory,
            self.logger)
        self.data_engine.register_strategy(strategy)
        self.exec_engine.register_strategy(strategy)
        strategy.start()

        self.market.process_tick(TestStubs.quote_tick_3decimal(self.usdjpy.symbol))  # Prepare market

        entry_order = strategy.order_factory.stop(
            USDJPY_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price(96.710, 3))

        bracket_order = strategy.order_factory.bracket(
            entry_order,
            stop_loss=Price(86.000, 3),
            take_profit=Price(97.000, 3))

        # Act
        strategy.submit_bracket_order(bracket_order)

        # Assert
        self.assertEqual(6, strategy.object_storer.count)
        print(strategy.object_storer.get_store())
        self.assertTrue(isinstance(strategy.object_storer.get_store()[5], OrderWorking))

    def test_modify_stop_order(self):
        # Arrange
        strategy = TestStrategy1(bar_type=TestStubs.bartype_usdjpy_1min_bid())
        strategy.register_trader(
            self.trader_id,
            self.clock,
            self.uuid_factory,
            self.logger)
        self.data_engine.register_strategy(strategy)
        self.exec_engine.register_strategy(strategy)
        strategy.start()

        self.market.process_tick(TestStubs.quote_tick_3decimal(self.usdjpy.symbol))  # Prepare market
        order = strategy.order_factory.stop(
            USDJPY_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price(96.711, 3))

        strategy.submit_order(order)

        # Act
        strategy.modify_order(order, order.quantity, Price(96.714, 3))

        # Assert
        self.assertEqual(Price(96.714, 3), strategy.execution.order(order.cl_ord_id).price)
        self.assertEqual(5, strategy.object_storer.count)
        self.assertTrue(isinstance(strategy.object_storer.get_store()[4], OrderModified))

    def test_modify_bracket_order_working_stop_loss(self):
        # Arrange
        strategy = TestStrategy1(bar_type=TestStubs.bartype_usdjpy_1min_bid())
        strategy.register_trader(
            self.trader_id,
            self.clock,
            self.uuid_factory,
            self.logger)
        self.data_engine.register_strategy(strategy)
        self.exec_engine.register_strategy(strategy)
        strategy.start()

        self.market.process_tick(TestStubs.quote_tick_3decimal(self.usdjpy.symbol))  # Prepare market

        entry_order = strategy.order_factory.market(
            USDJPY_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        bracket_order = strategy.order_factory.bracket(
            entry_order,
            stop_loss=Price(85.000, 3))

        strategy.submit_bracket_order(bracket_order)

        # Act
        strategy.modify_order(bracket_order.stop_loss, bracket_order.entry.quantity, Price(85.100, 3))

        # Assert
        self.assertEqual(Price(85.100, 3), strategy.execution.order(bracket_order.stop_loss.cl_ord_id).price)
        self.assertEqual(9, strategy.object_storer.count)
        self.assertTrue(isinstance(strategy.object_storer.get_store()[8], OrderModified))

    # TODO: Fix failing test - market not updating inside SimulatedMarket
    # def test_submit_market_order_with_slippage_fill_model_slips_order(self):
    #     # Arrange
    #     fill_model = FillModel(
    #         prob_fill_at_limit=0.0,
    #         prob_fill_at_stop=1.0,
    #         prob_slippage=1.0,
    #         random_seed=None)
    #
    #     market = SimulatedMarket(
    #         exec_engine=self.exec_engine,
    #         instruments={self.usdjpy.symbol: self.usdjpy},
    #         config=self.config,
    #         fill_model=fill_model,
    #         commission_model=MakerTakerCommissionModel(),
    #         clock=self.clock,
    #         uuid_factory=TestUUIDFactory(),
    #         logger=self.logger)
    #
    #     exec_client = BacktestExecClient(
    #         market=self.market,
    #         logger=self.logger)
    #
    #     self.exec_engine.register_client(exec_client)
    #     strategy = TestStrategy1(bar_type=TestStubs.bartype_usdjpy_1min_bid())
    #     strategy.register_trader(
    #         self.trader_id,
    #         self.clock,
    #         self.uuid_factory,
    #         self.logger)
    #     self.data_engine.register_strategy(strategy)
    #     self.exec_engine.register_strategy(strategy)
    #     strategy.start()
    #
    #     market.process_tick(TestStubs.quote_tick_3decimal(self.usdjpy.symbol))  # Prepare market
    #     market.process_tick(TestStubs.quote_tick_3decimal(self.usdjpy.symbol))  # Prepare market
    #     order = strategy.order_factory.market(
    #         USDJPY_FXCM,
    #         OrderSide.BUY,
    #         Quantity(100000))
    #
    #     # Act
    #     strategy.submit_order(order)
    #
    #     # Assert
    #     self.assertEqual(5, strategy.object_storer.count)
    #     self.assertTrue(isinstance(strategy.object_storer.get_store()[3], OrderFilled))
    #     self.assertEqual(Price(90.004, 3), strategy.order(order.id).avg_price)

    def test_submit_order_with_no_market_rejects_order(self):
        # Arrange
        strategy = TestStrategy1(bar_type=TestStubs.bartype_usdjpy_1min_bid())
        strategy.register_trader(
            self.trader_id,
            self.clock,
            self.uuid_factory,
            self.logger)
        self.data_engine.register_strategy(strategy)
        self.exec_engine.register_strategy(strategy)
        strategy.start()

        order = strategy.order_factory.stop(
            USDJPY_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price(80.000, 3))

        # Act
        strategy.submit_order(order)

        # Assert
        self.assertEqual(3, strategy.object_storer.count)
        self.assertTrue(isinstance(strategy.object_storer.get_store()[2], OrderRejected))

    def test_submit_order_with_invalid_price_gets_rejected(self):
        # Arrange
        strategy = TestStrategy1(bar_type=TestStubs.bartype_usdjpy_1min_bid())
        strategy.register_trader(
            self.trader_id,
            self.clock,
            self.uuid_factory,
            self.logger)
        self.data_engine.register_strategy(strategy)
        self.exec_engine.register_strategy(strategy)
        strategy.start()

        self.market.process_tick(TestStubs.quote_tick_3decimal(self.usdjpy.symbol))  # Prepare market
        order = strategy.order_factory.stop(
            USDJPY_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price(80.000, 3))

        # Act
        strategy.submit_order(order)

        # Assert
        self.assertEqual(3, strategy.object_storer.count)
        self.assertTrue(isinstance(strategy.object_storer.get_store()[2], OrderRejected))

    def test_order_fills_gets_commissioned(self):
        # Arrange
        strategy = TestStrategy1(bar_type=TestStubs.bartype_usdjpy_1min_bid())
        strategy.register_trader(
            self.trader_id,
            self.clock,
            self.uuid_factory,
            self.logger)
        self.data_engine.register_strategy(strategy)
        self.exec_engine.register_strategy(strategy)
        strategy.start()

        self.market.process_tick(TestStubs.quote_tick_3decimal(self.usdjpy.symbol))  # Prepare market
        order = strategy.order_factory.market(
            USDJPY_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        top_up_order = strategy.order_factory.market(
            USDJPY_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        reduce_order = strategy.order_factory.market(
            USDJPY_FXCM,
            OrderSide.BUY,
            Quantity(50000))

        # Act
        strategy.submit_order(order)

        position_id = PositionId("B-USD/JPY-1")  # Generated by exchange

        strategy.submit_order(top_up_order, position_id)
        strategy.submit_order(reduce_order, position_id)

        commission_percent = basis_points_as_percentage(7.5)
        self.assertEqual(strategy.object_storer.get_store()[3].commission.as_double(),
                         order.filled_qty.as_double() * commission_percent)
        self.assertEqual(strategy.object_storer.get_store()[7].commission.as_double(),
                         top_up_order.filled_qty.as_double() * commission_percent)
        self.assertEqual(strategy.object_storer.get_store()[11].commission.as_double(),
                         reduce_order.filled_qty.as_double() * commission_percent)

        position = self.exec_engine.database.positions_open()[0]
        expected_commission = position.quantity.as_double() * commission_percent
        self.assertEqual(strategy.account().cash_start_day.as_double() - expected_commission,
                         strategy.account().cash_balance.as_double())

    def test_realized_pnl_contains_commission(self):
        # Arrange
        strategy = TestStrategy1(bar_type=TestStubs.bartype_usdjpy_1min_bid())
        strategy.register_trader(
            self.trader_id,
            self.clock,
            self.uuid_factory,
            self.logger)
        self.data_engine.register_strategy(strategy)
        self.exec_engine.register_strategy(strategy)
        strategy.start()

        self.market.process_tick(TestStubs.quote_tick_3decimal(self.usdjpy.symbol))  # Prepare market
        order = strategy.order_factory.market(
            USDJPY_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        # Act
        strategy.submit_order(order)

        filled_price = strategy.object_storer.get_store()[3].avg_price.as_double()
        commission = strategy.object_storer.get_store()[3].commission.as_double()
        commission = Money(-commission * filled_price, 392)
        position = self.exec_engine.database.positions_open()[0]
        self.assertEqual(position.realized_pnl, commission)

    def test_commission_maker_taker_order(self):
        # Arrange
        strategy = TestStrategy1(bar_type=TestStubs.bartype_usdjpy_1min_bid())
        strategy.register_trader(
            self.trader_id,
            self.clock,
            self.uuid_factory,
            self.logger)
        self.data_engine.register_strategy(strategy)
        self.exec_engine.register_strategy(strategy)
        strategy.start()

        self.market.process_tick(TestStubs.quote_tick_3decimal(self.usdjpy.symbol))  # Prepare market

        order_market = strategy.order_factory.market(
            USDJPY_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        order_limit = strategy.order_factory.limit(
            USDJPY_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price(80.001, 3))

        # Act

        strategy.submit_order(order_market)
        strategy.submit_order(order_limit)

        self.market.process_tick(QuoteTick(
            self.usdjpy.symbol,
            Price(80.000, 3),
            Price(80.000, 3),
            Quantity(100000),
            Quantity(100000),
            UNIX_EPOCH)
        )  # Fill the limit order

        # Assert
        self.assertEqual(LiquiditySide.TAKER, strategy.object_storer.get_store()[3].liquidity_side)
        self.assertEqual(LiquiditySide.MAKER, strategy.object_storer.get_store()[8].liquidity_side)
        self.assertEqual(75, strategy.object_storer.get_store()[3].commission.as_double())
        self.assertEqual(-25, strategy.object_storer.get_store()[8].commission.as_double())

    def test_unrealized_pnl(self):
        # Arrange
        strategy = TestStrategy1(bar_type=TestStubs.bartype_usdjpy_1min_bid())
        strategy.register_trader(
            self.trader_id,
            self.clock,
            self.uuid_factory,
            self.logger)
        self.data_engine.register_strategy(strategy)
        self.exec_engine.register_strategy(strategy)
        strategy.start()

        open_quote = TestStubs.quote_tick_3decimal(self.usdjpy.symbol)
        self.market.process_tick(open_quote)  # Prepare market
        order_open = strategy.order_factory.market(
            USDJPY_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        # Act 1
        strategy.submit_order(order_open)

        reduce_quote = QuoteTick(
            self.usdjpy.symbol,
            Price(100.003, 3),
            Price(100.003, 3),
            Quantity(1),
            Quantity(1),
            UNIX_EPOCH)
        self.market.process_tick(reduce_quote)

        order_reduce = strategy.order_factory.market(
            USDJPY_FXCM,
            OrderSide.SELL,
            Quantity(50000))

        position_id = PositionId("B-USD/JPY-1")  # Generated by exchange

        # Act 2
        strategy.submit_order(order_reduce, position_id)

        # Assert
        position = self.exec_engine.database.positions_open()[0]
        unrealized_pnl = position.unrealized_pnl(reduce_quote).as_double()
        expected_unrealized_pnl = \
            order_reduce.quantity.as_double() * (reduce_quote.bid.sub(open_quote.ask).as_double())
        self.assertEqual(unrealized_pnl, expected_unrealized_pnl)

    # TODO: Position flip behaviour needs to be implemented
    # def test_position_dir_change(self):
    #     # Arrange
    #     strategy = TestStrategy1(bar_type=TestStubs.bartype_usdjpy_1min_bid())
    #     strategy.register_trader(
    #         self.trader_id,
    #         self.clock,
    #         self.uuid_factory,
    #         self.logger)
    #     self.data_engine.register_strategy(strategy)
    #     self.exec_engine.register_strategy(strategy)
    #     strategy.start()
    #
    #     open_quote = QuoteTick(
    #         self.usdjpy.symbol,
    #         Price(90.002, 3),
    #         Price(90.003, 3),
    #         Quantity(1),
    #         Quantity(1),
    #         UNIX_EPOCH,
    #     )
    #
    #     self.market.process_tick(open_quote)  # Prepare market
    #     order_open = strategy.order_factory.market(
    #         USDJPY_FXCM,
    #         OrderSide.BUY,
    #         Quantity(100000),
    #     )
    #
    #     # Act 1
    #     strategy.submit_order(order_open)
    #
    #     reduce_quote = QuoteTick(
    #         self.usdjpy.symbol,
    #         Price(100.003, 3),
    #         Price(100.003, 3),
    #         Quantity(1),
    #         Quantity(1),
    #         UNIX_EPOCH,
    #     )
    #
    #     self.market.process_tick(reduce_quote)
    #
    #     order_reduce = strategy.order_factory.market(
    #         USDJPY_FXCM,
    #         OrderSide.SELL,
    #         Quantity(150000),
    #     )
    #
    #     # Act 2
    #     strategy.submit_order(order_reduce)
    #
    #     # Assert
    #     position = [p for p in strategy.positions_open().values()][0]
    #     self.assertEqual(position.side, PositionSide.SHORT)
    #     self.assertEqual(position.quantity, order_reduce.quantity.sub(order_open.quantity))
    #     self.assertEqual(position.unrealized_points(reduce_quote), -10.0)
    #     self.assertEqual(position.unrealized_pnl(reduce_quote).as_double(), -500000.0)
