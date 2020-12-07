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

from decimal import Decimal
import unittest

from nautilus_trader.analysis.performance import PerformanceAnalyzer
from nautilus_trader.backtest.config import BacktestConfig
from nautilus_trader.backtest.exchange import SimulatedExchange
from nautilus_trader.backtest.execution import BacktestExecClient
from nautilus_trader.backtest.loaders import InstrumentLoader
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.logging import TestLogger
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.execution.database import BypassExecutionDatabase
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.currencies import JPY
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OMSType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderState
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.events import OrderRejected
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.tick import QuoteTick
from nautilus_trader.trading.portfolio import Portfolio
from tests.test_kit.mocks import MockStrategy
from tests.test_kit.stubs import TestStubs
from tests.test_kit.stubs import UNIX_EPOCH


FXCM = Venue("FXCM")
AUDUSD_FXCM = InstrumentLoader.default_fx_ccy(TestStubs.symbol_audusd_fxcm())
USDJPY_FXCM = InstrumentLoader.default_fx_ccy(TestStubs.symbol_usdjpy_fxcm())
XBTUSD_BITMEX = InstrumentLoader.xbtusd_bitmex()


class ExchangeTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.clock = TestClock()
        self.uuid_factory = UUIDFactory()
        self.logger = TestLogger(self.clock)

        self.portfolio = Portfolio(
            clock=self.clock,
            logger=self.logger,
        )

        self.data_engine = DataEngine(
            portfolio=self.portfolio,
            clock=self.clock,
            logger=self.logger,
            config={'use_previous_close': False},  # To correctly reproduce historical data bars
        )

        self.data_engine.cache.add_instrument(USDJPY_FXCM)
        self.portfolio.register_cache(self.data_engine.cache)

        self.analyzer = PerformanceAnalyzer()
        self.trader_id = TraderId("TESTER", "000")
        self.account_id = AccountId("FXCM", "001", AccountType.SIMULATED)

        exec_db = BypassExecutionDatabase(
            trader_id=self.trader_id,
            logger=self.logger,
        )

        self.exec_engine = ExecutionEngine(
            database=exec_db,
            portfolio=self.portfolio,
            clock=self.clock,
            logger=self.logger,
        )

        self.exchange = SimulatedExchange(
            venue=FXCM,
            oms_type=OMSType.HEDGING,
            generate_position_ids=True,
            exec_cache=self.exec_engine.cache,
            instruments={USDJPY_FXCM.symbol: USDJPY_FXCM},
            config=BacktestConfig(),
            fill_model=FillModel(),
            clock=self.clock,
            logger=self.logger,
        )

        self.exec_client = BacktestExecClient(
            exchange=self.exchange,
            account_id=self.account_id,
            engine=self.exec_engine,
            clock=self.clock,
            logger=self.logger,
        )

        self.exec_engine.register_client(self.exec_client)
        self.exchange.register_client(self.exec_client)

        self.strategy = MockStrategy(bar_type=TestStubs.bartype_usdjpy_1min_bid())
        self.strategy.register_trader(
            self.trader_id,
            self.clock,
            self.logger,
        )

        self.data_engine.register_strategy(self.strategy)
        self.exec_engine.register_strategy(self.strategy)
        self.data_engine.start()
        self.exec_engine.start()
        self.strategy.start()

    def test_repr(self):
        # Arrange
        # Act
        # Assert
        self.assertEqual("SimulatedExchange(FXCM)", repr(self.exchange))

    def test_check_residuals(self):
        # Arrange
        # Act
        self.exchange.check_residuals()
        # Assert
        self.assertTrue(True)  # No exceptions raised

    def test_get_working_orders_when_no_orders_returns_empty_dict(self):
        # Arrange
        # Act
        orders = self.exchange.get_working_orders()

        self.assertEqual({}, orders)

    def test_submit_order_with_no_market_rejects_order(self):
        # Arrange
        order = self.strategy.order_factory.stop_market(
            USDJPY_FXCM.symbol,
            OrderSide.BUY,
            Quantity(100000),
            Price("80.000"),
        )

        # Act
        self.strategy.submit_order(order)

        # Assert
        self.assertEqual(2, self.strategy.object_storer.count)
        self.assertTrue(isinstance(self.strategy.object_storer.get_store()[1], OrderRejected))

    def test_submit_order_with_invalid_price_gets_rejected(self):
        # Arrange
        # Prepare market
        tick = TestStubs.quote_tick_3decimal(USDJPY_FXCM.symbol)
        self.exchange.process_tick(tick)
        self.portfolio.update_tick(tick)

        order = self.strategy.order_factory.stop_market(
            USDJPY_FXCM.symbol,
            OrderSide.BUY,
            Quantity(100000),
            Price("80.000"),
        )

        # Act
        self.strategy.submit_order(order)

        # Assert
        self.assertEqual(OrderState.REJECTED, order.state)

    def test_submit_market_order(self):
        # Arrange
        # Prepare market
        tick = TestStubs.quote_tick_3decimal(USDJPY_FXCM.symbol)
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        # Create order
        order = self.strategy.order_factory.market(
            USDJPY_FXCM.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

        # Act
        self.strategy.submit_order(order)

        # Assert
        self.assertEqual(OrderState.FILLED, order.state)
        self.assertEqual(Decimal("90.003"), order.avg_price)

    def test_submit_limit_order(self):
        # Arrange
        # Prepare market
        tick = TestStubs.quote_tick_3decimal(USDJPY_FXCM.symbol)
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        order = self.strategy.order_factory.limit(
            USDJPY_FXCM.symbol,
            OrderSide.BUY,
            Quantity(100000),
            Price("80.000"),
        )

        # Act
        self.strategy.submit_order(order)

        # Assert
        self.assertEqual(1, len(self.exchange.get_working_orders()))
        self.assertIn(order.cl_ord_id, self.exchange.get_working_orders())

    def test_submit_bracket_market_order(self):
        # Arrange
        # Prepare market
        tick = TestStubs.quote_tick_3decimal(USDJPY_FXCM.symbol)
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        entry_order = self.strategy.order_factory.market(
            USDJPY_FXCM.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

        bracket_order = self.strategy.order_factory.bracket(
            entry_order,
            Price("80.000"),
        )

        # Act
        self.strategy.submit_bracket_order(bracket_order)

        # Assert
        self.assertEqual(OrderState.FILLED, entry_order.state)

    def test_submit_bracket_stop_order(self):
        # Arrange
        # Prepare market
        tick = TestStubs.quote_tick_3decimal(USDJPY_FXCM.symbol)
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        entry_order = self.strategy.order_factory.stop_market(
            USDJPY_FXCM.symbol,
            OrderSide.BUY,
            Quantity(100000),
            Price("96.710"),
        )

        bracket_order = self.strategy.order_factory.bracket(
            entry_order,
            Price("86.000"),
            Price("97.000"),
        )

        # Act
        self.strategy.submit_bracket_order(bracket_order)

        # Assert
        self.assertEqual(1, len(self.exchange.get_working_orders()))
        self.assertIn(entry_order.cl_ord_id, self.exchange.get_working_orders())

    def test_cancel_stop_order(self):
        # Arrange
        # Prepare market
        tick = TestStubs.quote_tick_3decimal(USDJPY_FXCM.symbol)
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        order = self.strategy.order_factory.stop_market(
            USDJPY_FXCM.symbol,
            OrderSide.BUY,
            Quantity(100000),
            Price("96.711"),
        )

        self.strategy.submit_order(order)

        # Act
        self.strategy.cancel_order(order)

        # Assert
        self.assertEqual(0, len(self.exchange.get_working_orders()))

    def test_modify_stop_order(self):
        # Arrange
        # Prepare market
        tick = TestStubs.quote_tick_3decimal(USDJPY_FXCM.symbol)
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        order = self.strategy.order_factory.stop_market(
            USDJPY_FXCM.symbol,
            OrderSide.BUY,
            Quantity(100000),
            Price("96.711"),
        )

        self.strategy.submit_order(order)

        # Act
        self.strategy.modify_order(order, order.quantity, Price("96.714"))

        # Assert
        self.assertEqual(1, len(self.exchange.get_working_orders()))
        self.assertEqual(Price("96.714"), order.price)

    def test_modify_bracket_order_working_stop_loss(self):
        # Arrange
        # Prepare market
        tick = TestStubs.quote_tick_3decimal(USDJPY_FXCM.symbol)
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        entry_order = self.strategy.order_factory.market(
            USDJPY_FXCM.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

        bracket_order = self.strategy.order_factory.bracket(
            entry_order,
            stop_loss=Price("85.000"),
        )

        self.strategy.submit_bracket_order(bracket_order)

        # Act
        self.strategy.modify_order(bracket_order.stop_loss, bracket_order.entry.quantity, Price("85.100"))

        # Assert
        self.assertEqual(Price("85.100"), bracket_order.stop_loss.price)

    def test_submit_market_order_with_slippage_fill_model_slips_order(self):
        # Arrange
        # Prepare market
        tick = TestStubs.quote_tick_3decimal(USDJPY_FXCM.symbol)
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        fill_model = FillModel(
            prob_fill_at_limit=0.0,
            prob_fill_at_stop=1.0,
            prob_slippage=1.0,
            random_seed=None,
        )

        self.exchange.change_fill_model(fill_model)

        order = self.strategy.order_factory.market(
            USDJPY_FXCM.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

        # Act
        self.strategy.submit_order(order)

        # Assert
        self.assertEqual(Decimal("90.004"), order.avg_price)

    def test_order_fills_gets_commissioned(self):
        # Arrange
        # Prepare market
        tick = TestStubs.quote_tick_3decimal(USDJPY_FXCM.symbol)
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        order = self.strategy.order_factory.market(
            USDJPY_FXCM.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

        top_up_order = self.strategy.order_factory.market(
            USDJPY_FXCM.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

        reduce_order = self.strategy.order_factory.market(
            USDJPY_FXCM.symbol,
            OrderSide.BUY,
            Quantity(50000),
        )

        # Act
        self.strategy.submit_order(order)

        position_id = PositionId("B-USD/JPY-1")  # Generated by exchange

        self.strategy.submit_order(top_up_order, position_id)
        self.strategy.submit_order(reduce_order, position_id)

        account_event1 = self.strategy.object_storer.get_store()[2]
        account_event2 = self.strategy.object_storer.get_store()[6]
        account_event3 = self.strategy.object_storer.get_store()[10]

        account = self.exec_engine.cache.account_for_venue(Venue("FXCM"))

        # Assert
        self.assertEqual(Money(180.01, JPY), account_event1.commission)
        self.assertEqual(Money(180.01, JPY), account_event2.commission)
        self.assertEqual(Money(90.00, JPY), account_event3.commission)
        self.assertTrue(Money(999995.00, USD), account.balance())

    def test_process_tick_fills_buy_stop_order(self):
        # Arrange
        # Prepare market
        tick = TestStubs.quote_tick_3decimal(USDJPY_FXCM.symbol)
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        order = self.strategy.order_factory.stop_market(
            USDJPY_FXCM.symbol,
            OrderSide.BUY,
            Quantity(100000),
            Price("96.711"),
        )

        self.strategy.submit_order(order)

        # Act
        tick2 = QuoteTick(
            AUDUSD_FXCM.symbol,  # Different market
            Price("80.010"),
            Price("80.011"),
            Quantity(200000),
            Quantity(200000),
            UNIX_EPOCH,
        )

        tick3 = QuoteTick(
            USDJPY_FXCM.symbol,
            Price("96.710"),
            Price("96.712"),
            Quantity(100000),
            Quantity(100000),
            UNIX_EPOCH,
        )

        self.exchange.process_tick(tick2)
        self.exchange.process_tick(tick3)

        # Assert
        self.assertEqual(0, len(self.exchange.get_working_orders()))
        self.assertEqual(OrderState.FILLED, order.state)
        self.assertEqual(Price("96.711"), order.avg_price)

    def test_process_tick_fills_buy_limit_order(self):
        # Arrange
        # Prepare market
        tick = TestStubs.quote_tick_3decimal(USDJPY_FXCM.symbol)
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        order = self.strategy.order_factory.limit(
            USDJPY_FXCM.symbol,
            OrderSide.BUY,
            Quantity(100000),
            Price("90.001"),
        )

        self.strategy.submit_order(order)

        # Act
        tick2 = QuoteTick(
            AUDUSD_FXCM.symbol,  # Different market
            Price("80.010"),
            Price("80.011"),
            Quantity(200000),
            Quantity(200000),
            UNIX_EPOCH,
        )

        tick3 = QuoteTick(
            USDJPY_FXCM.symbol,
            Price("89.998"),
            Price("89.999"),
            Quantity(100000),
            Quantity(100000),
            UNIX_EPOCH,
        )

        self.exchange.process_tick(tick2)
        self.exchange.process_tick(tick3)

        # Assert
        self.assertEqual(0, len(self.exchange.get_working_orders()))
        self.assertEqual(OrderState.FILLED, order.state)
        self.assertEqual(Price("90.001"), order.avg_price)

    def test_process_tick_fills_sell_stop_order(self):
        # Arrange
        # Prepare market
        tick = TestStubs.quote_tick_3decimal(USDJPY_FXCM.symbol)
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        order = self.strategy.order_factory.stop_market(
            USDJPY_FXCM.symbol,
            OrderSide.SELL,
            Quantity(100000),
            Price("90.000"),
        )

        self.strategy.submit_order(order)

        # Act
        tick2 = QuoteTick(
            USDJPY_FXCM.symbol,
            Price("89.997"),
            Price("89.999"),
            Quantity(100000),
            Quantity(100000),
            UNIX_EPOCH,
        )

        self.exchange.process_tick(tick2)

        # Assert
        self.assertEqual(0, len(self.exchange.get_working_orders()))
        self.assertEqual(OrderState.FILLED, order.state)
        self.assertEqual(Price("90.000"), order.avg_price)

    def test_process_tick_fills_sell_limit_order(self):
        # Arrange
        # Prepare market
        tick = TestStubs.quote_tick_3decimal(USDJPY_FXCM.symbol)
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        order = self.strategy.order_factory.limit(
            USDJPY_FXCM.symbol,
            OrderSide.SELL,
            Quantity(100000),
            Price("90.100"),
        )

        self.strategy.submit_order(order)

        # Act
        tick2 = QuoteTick(
            USDJPY_FXCM.symbol,
            Price("90.101"),
            Price("90.102"),
            Quantity(100000),
            Quantity(100000),
            UNIX_EPOCH,
        )

        self.exchange.process_tick(tick2)

        # Assert
        self.assertEqual(0, len(self.exchange.get_working_orders()))
        self.assertEqual(OrderState.FILLED, order.state)
        self.assertEqual(Price("90.100"), order.avg_price)

    def test_process_tick_fills_buy_limit_entry_with_bracket(self):
        # Arrange
        # Prepare market
        tick = TestStubs.quote_tick_3decimal(USDJPY_FXCM.symbol)
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        entry = self.strategy.order_factory.limit(
            USDJPY_FXCM.symbol,
            OrderSide.BUY,
            Quantity(100000),
            Price("90.000"),
        )

        bracket = self.strategy.order_factory.bracket(
            entry_order=entry,
            stop_loss=Price("89.900"),
        )

        self.strategy.submit_bracket_order(bracket)

        # Act
        tick2 = QuoteTick(
            USDJPY_FXCM.symbol,
            Price("89.998"),
            Price("89.999"),
            Quantity(100000),
            Quantity(100000),
            UNIX_EPOCH,
        )

        self.exchange.process_tick(tick2)

        # Assert
        self.assertEqual(1, len(self.exchange.get_working_orders()))
        self.assertIn(bracket.stop_loss, self.exchange.get_working_orders().values())

    def test_process_tick_fills_sell_limit_entry_with_bracket(self):
        # Arrange
        # Prepare market
        tick = TestStubs.quote_tick_3decimal(USDJPY_FXCM.symbol)
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        entry = self.strategy.order_factory.limit(
            USDJPY_FXCM.symbol,
            OrderSide.SELL,
            Quantity(100000),
            Price("91.100"),
        )

        bracket = self.strategy.order_factory.bracket(
            entry_order=entry,
            stop_loss=Price("91.200"),
            take_profit=Price("90.000"),
        )

        self.strategy.submit_bracket_order(bracket)

        # Act
        tick2 = QuoteTick(
            USDJPY_FXCM.symbol,
            Price("91.101"),
            Price("91.102"),
            Quantity(100000),
            Quantity(100000),
            UNIX_EPOCH,
        )

        self.exchange.process_tick(tick2)

        # Assert
        self.assertEqual(2, len(self.exchange.get_working_orders()))  # SL and TP
        self.assertIn(bracket.stop_loss, self.exchange.get_working_orders().values())
        self.assertIn(bracket.take_profit, self.exchange.get_working_orders().values())

    def test_filling_oco_sell_cancels_other_order(self):
        # Arrange
        # Prepare market
        tick = TestStubs.quote_tick_3decimal(USDJPY_FXCM.symbol)
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        entry = self.strategy.order_factory.limit(
            USDJPY_FXCM.symbol,
            OrderSide.SELL,
            Quantity(100000),
            Price("91.100"),
        )

        bracket = self.strategy.order_factory.bracket(
            entry_order=entry,
            stop_loss=Price("91.200"),
            take_profit=Price("90.000"),
        )

        self.strategy.submit_bracket_order(bracket)

        # Act
        tick2 = QuoteTick(
            USDJPY_FXCM.symbol,
            Price("91.101"),
            Price("91.102"),
            Quantity(100000),
            Quantity(100000),
            UNIX_EPOCH,
        )

        tick3 = QuoteTick(
            USDJPY_FXCM.symbol,
            Price("91.201"),
            Price("91.203"),
            Quantity(100000),
            Quantity(100000),
            UNIX_EPOCH,
        )

        self.exchange.process_tick(tick2)
        self.exchange.process_tick(tick3)

        # Assert
        self.assertEqual(0, len(self.exchange.get_working_orders()))

    def test_realized_pnl_contains_commission(self):
        # Arrange
        # Prepare market
        tick = TestStubs.quote_tick_3decimal(USDJPY_FXCM.symbol)
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        order = self.strategy.order_factory.market(
            USDJPY_FXCM.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

        # Act
        self.strategy.submit_order(order)
        position = self.exec_engine.cache.positions_open()[0]

        # Assert
        self.assertEqual(Money(180.01, JPY), position.realized_pnl)
        self.assertEqual(Money(180.01, JPY), position.commissions)

    def test_unrealized_pnl(self):
        # Arrange
        # Prepare market
        tick = TestStubs.quote_tick_3decimal(USDJPY_FXCM.symbol)
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        order_open = self.strategy.order_factory.market(
            USDJPY_FXCM.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

        # Act 1
        self.strategy.submit_order(order_open)

        reduce_quote = QuoteTick(
            USDJPY_FXCM.symbol,
            Price("100.003"),
            Price("100.003"),
            Quantity(100000),
            Quantity(100000),
            UNIX_EPOCH,
        )

        self.exchange.process_tick(reduce_quote)
        self.portfolio.update_tick(reduce_quote)

        order_reduce = self.strategy.order_factory.market(
            USDJPY_FXCM.symbol,
            OrderSide.SELL,
            Quantity(50000),
        )

        position_id = PositionId("B-USD/JPY-1")  # Generated by exchange

        # Act 2
        self.strategy.submit_order(order_reduce, position_id)

        # Assert
        position = self.exec_engine.cache.positions_open()[0]
        self.assertEqual(Money(500000.00, JPY), position.unrealized_pnl(Price("100.003")))

    def test_position_flipped_when_reduce_order_exceeds_original_quantity(self):
        # Arrange
        # Prepare market
        tick = TestStubs.quote_tick_3decimal(USDJPY_FXCM.symbol)
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        open_quote = QuoteTick(
            USDJPY_FXCM.symbol,
            Price("90.002"),
            Price("90.003"),
            Quantity(1),
            Quantity(1),
            UNIX_EPOCH,
        )

        order_open = self.strategy.order_factory.market(
            USDJPY_FXCM.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

        # Act 1
        self.strategy.submit_order(order_open)

        reduce_quote = QuoteTick(
            USDJPY_FXCM.symbol,
            Price("100.003"),
            Price("100.003"),
            Quantity(1),
            Quantity(1),
            UNIX_EPOCH,
        )

        self.exchange.process_tick(reduce_quote)
        self.portfolio.update_tick(reduce_quote)

        order_reduce = self.strategy.order_factory.market(
            USDJPY_FXCM.symbol,
            OrderSide.SELL,
            Quantity(150000),
        )

        # Act 2
        self.strategy.submit_order(order_reduce, PositionId("B-USD/JPY-1"))

        # Assert
        position_open = self.strategy.execution.positions_open()[0]
        position_closed = self.strategy.execution.positions_closed()[0]
        self.assertEqual(PositionSide.SHORT, position_open.side)
        self.assertEqual(Quantity(50000), position_open.quantity)
        self.assertEqual(Money(1000380.02, JPY), position_closed.realized_pnl)


class BitmexExchangeTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.strategies = [MockStrategy(TestStubs.bartype_btcusdt_binance_1min_bid())]

        self.clock = TestClock()
        self.uuid_factory = UUIDFactory()
        self.logger = TestLogger(self.clock)

        self.portfolio = Portfolio(
            clock=self.clock,
            logger=self.logger,
        )

        self.data_engine = DataEngine(
            portfolio=self.portfolio,
            clock=self.clock,
            logger=self.logger,
            config={'use_previous_close': False},  # To correctly reproduce historical data bars
        )
        self.data_engine.cache.add_instrument(XBTUSD_BITMEX)
        self.portfolio.register_cache(self.data_engine.cache)

        self.analyzer = PerformanceAnalyzer()

        self.trader_id = TraderId("TESTER", "000")
        self.account_id = AccountId("BITMEX", "001", AccountType.SIMULATED)

        exec_db = BypassExecutionDatabase(
            trader_id=self.trader_id,
            logger=self.logger,
        )

        self.exec_engine = ExecutionEngine(
            database=exec_db,
            portfolio=self.portfolio,
            clock=self.clock,
            logger=self.logger,
        )

        self.config = BacktestConfig()
        self.exchange = SimulatedExchange(
            venue=Venue("BITMEX"),
            oms_type=OMSType.HEDGING,
            generate_position_ids=True,
            exec_cache=self.exec_engine.cache,
            instruments={XBTUSD_BITMEX.symbol: XBTUSD_BITMEX},
            config=self.config,
            fill_model=FillModel(),
            clock=self.clock,
            logger=self.logger,
        )

        self.exec_client = BacktestExecClient(
            exchange=self.exchange,
            account_id=self.account_id,
            engine=self.exec_engine,
            clock=self.clock,
            logger=self.logger,
        )

        self.exec_engine.register_client(self.exec_client)
        self.exchange.register_client(self.exec_client)

        self.strategy = MockStrategy(bar_type=TestStubs.bartype_btcusdt_binance_1min_bid())
        self.strategy.register_trader(
            self.trader_id,
            self.clock,
            self.logger,
        )

        self.data_engine.register_strategy(self.strategy)
        self.exec_engine.register_strategy(self.strategy)
        self.data_engine.start()
        self.exec_engine.start()
        self.strategy.start()

    def test_commission_maker_taker_order(self):
        # Arrange
        # Prepare market
        quote1 = QuoteTick(
            XBTUSD_BITMEX.symbol,
            Price("11493.70"),
            Price("11493.75"),
            Quantity(1500000),
            Quantity(1500000),
            UNIX_EPOCH,
        )

        self.data_engine.process(quote1)
        self.exchange.process_tick(quote1)

        order_market = self.strategy.order_factory.market(
            XBTUSD_BITMEX.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

        order_limit = self.strategy.order_factory.limit(
            XBTUSD_BITMEX.symbol,
            OrderSide.BUY,
            Quantity(100000),
            Price("11493.65"),
        )

        # Act
        self.strategy.submit_order(order_market)
        self.strategy.submit_order(order_limit)

        quote2 = QuoteTick(
            XBTUSD_BITMEX.symbol,
            Price("11493.60"),
            Price("11493.64"),
            Quantity(1500000),
            Quantity(1500000),
            UNIX_EPOCH,
        )

        self.exchange.process_tick(quote2)  # Fill the limit order
        self.portfolio.update_tick(quote2)

        # Assert
        self.assertEqual(LiquiditySide.TAKER, self.strategy.object_storer.get_store()[2].liquidity_side)
        self.assertEqual(LiquiditySide.MAKER, self.strategy.object_storer.get_store()[7].liquidity_side)
        self.assertEqual(Money(0.00652529, BTC), self.strategy.object_storer.get_store()[2].commission)
        self.assertEqual(Money(-0.00217511, BTC), self.strategy.object_storer.get_store()[7].commission)
