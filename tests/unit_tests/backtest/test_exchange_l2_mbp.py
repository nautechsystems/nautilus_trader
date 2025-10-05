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

from decimal import Decimal

from nautilus_trader.backtest.engine import SimulatedExchange
from nautilus_trader.backtest.execution_client import BacktestExecClient
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.backtest.models import LatencyModel
from nautilus_trader.backtest.models import MakerTakerFeeModel
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.component import TestClock
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.model.book import OrderBook
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Quantity
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.risk.engine import RiskEngine
from nautilus_trader.test_kit.mocks.strategies import MockStrategy
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


SIM = Venue("SIM")
_USDJPY_SIM = TestInstrumentProvider.default_fx_ccy("USD/JPY")


class TestL2OrderBookExchange:
    def setup(self):
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
            venue=SIM,
            oms_type=OmsType.HEDGING,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            starting_balances=[Money(1_000_000, USD)],
            default_leverage=Decimal(50),
            leverages={},
            modules=[],
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            book_type=BookType.L2_MBP,  # <-- L2 MBP book
            latency_model=LatencyModel(0),
        )
        self.exchange.add_instrument(_USDJPY_SIM)

        self.exec_client = BacktestExecClient(
            exchange=self.exchange,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Prepare components
        self.cache.add_instrument(_USDJPY_SIM)
        self.cache.add_order_book(
            OrderBook(
                instrument_id=_USDJPY_SIM.id,
                book_type=BookType.L2_MBP,  # <-- L2 MBP book
            ),
        )

        self.exec_engine.register_client(self.exec_client)
        self.exchange.register_client(self.exec_client)

        self.strategy = MockStrategy(bar_type=TestDataStubs.bartype_usdjpy_1min_bid())
        self.strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.exchange.reset()
        self.data_engine.start()
        self.exec_engine.start()
        self.strategy.start()

    def test_submit_limit_order_aggressive_multiple_levels(self):
        # Arrange: Prepare market
        self.cache.add_instrument(_USDJPY_SIM)

        quote = QuoteTick(
            instrument_id=_USDJPY_SIM.id,
            bid_price=_USDJPY_SIM.make_price(110.000),
            ask_price=_USDJPY_SIM.make_price(110.010),
            bid_size=Quantity.from_int(1_500_000),
            ask_size=Quantity.from_int(1_500_000),
            ts_event=0,
            ts_init=0,
        )
        self.data_engine.process(quote)
        snapshot = TestDataStubs.order_book_snapshot(
            instrument=_USDJPY_SIM,
            bid_size=10000,
            ask_size=10000,
        )
        self.data_engine.process(snapshot)
        self.exchange.process_order_book_deltas(snapshot)

        # Create order
        order = self.strategy.order_factory.limit(
            instrument_id=_USDJPY_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(20_000),
            price=_USDJPY_SIM.make_price(102.000),
            post_only=False,
        )

        # Act
        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Assert
        assert order.status == OrderStatus.FILLED
        assert order.filled_qty == Decimal("20000.0")
        # Corrected weighted average calculation
        assert order.avg_px == 101.5
        assert self.exchange.get_account().balance_total(USD) == Money(999999.64, USD)

    def test_aggressive_partial_fill(self):
        # Arrange: Prepare market
        self.cache.add_instrument(_USDJPY_SIM)

        snapshot = TestDataStubs.order_book_snapshot(
            instrument=_USDJPY_SIM,
            bid_size=10_000,
            ask_size=10_000,
        )
        self.data_engine.process(snapshot)
        self.exchange.process_order_book_deltas(snapshot)

        # Act
        order = self.strategy.order_factory.limit(
            instrument_id=_USDJPY_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(70_000),
            price=_USDJPY_SIM.make_price(112.000),
            post_only=False,
        )
        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Assert
        assert order.status == OrderStatus.PARTIALLY_FILLED
        assert order.filled_qty == Quantity.from_str("60000.0")
        # Corrected weighted average calculation
        assert order.avg_px == 102.33333333333333

    def test_post_only_insert(self):
        # Arrange: Prepare market
        self.cache.add_instrument(_USDJPY_SIM)
        snapshot = TestDataStubs.order_book_snapshot(
            instrument=_USDJPY_SIM,
            bid_size=100_000,
            ask_size=100_000,
        )
        self.data_engine.process(snapshot)
        self.exchange.process_order_book_deltas(snapshot)

        # Act
        order = self.strategy.order_factory.limit(
            instrument_id=_USDJPY_SIM.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(2_000),
            price=_USDJPY_SIM.make_price(102.000),
            post_only=True,
        )
        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Assert
        assert order.status == OrderStatus.ACCEPTED

    def test_post_only_reject_would_take(self):
        # Arrange: Prepare market
        self.cache.add_instrument(_USDJPY_SIM)
        # Market is at 100.000 @ 101.000
        snapshot = TestDataStubs.order_book_snapshot(
            instrument=_USDJPY_SIM,
            bid_size=100_000,
            ask_size=100_000,
        )
        self.data_engine.process(snapshot)
        self.exchange.process_order_book_deltas(snapshot)

        # Act: Submit a post-only BUY order at 101.000 (would cross the spread and take)
        order = self.strategy.order_factory.limit(
            instrument_id=_USDJPY_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(2_000),
            price=_USDJPY_SIM.make_price(101.000),  # At ask price - would be a taker
            post_only=True,
        )
        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Assert: Order should be rejected
        assert order.status == OrderStatus.REJECTED
        assert len(order.events) == 3  # Initialized, Submitted, and Rejected

        # Check the rejection event has due_post_only flag set
        rejected_event = order.events[-1]
        from nautilus_trader.model.events import OrderRejected

        assert isinstance(rejected_event, OrderRejected)
        assert rejected_event.due_post_only is True
        assert "POST_ONLY" in rejected_event.reason

    def test_passive_partial_fill_sell(self):
        # Arrange: Prepare market
        self.cache.add_instrument(_USDJPY_SIM)
        # Market is 10 @ 15
        snapshot = TestDataStubs.order_book_snapshot(
            instrument=_USDJPY_SIM,
            bid_size=100_000,
            ask_size=100_000,
        )
        self.data_engine.process(snapshot)
        self.exchange.process_order_book_deltas(snapshot)

        order = self.strategy.order_factory.limit(
            instrument_id=_USDJPY_SIM.id,
            order_side=OrderSide.SELL,
            quantity=_USDJPY_SIM.make_qty(100_000),
            price=_USDJPY_SIM.make_price(101.0),
            post_only=False,
        )
        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Act
        delta = TestDataStubs.order_book_delta(
            instrument_id=_USDJPY_SIM.id,
            action=BookAction.ADD,
            order=TestDataStubs.order(
                instrument=_USDJPY_SIM,
                side=OrderSide.BUY,
                price=102.0,
                size=50_000,
            ),
        )
        self.exchange.process_order_book_delta(delta)

        # Assert
        assert order.status == OrderStatus.PARTIALLY_FILLED
        assert order.filled_qty == _USDJPY_SIM.make_qty(50_000)
        assert order.avg_px == Decimal("101.000")  # <-- Fills at limit price

    def test_passive_partial_fill_buy(self):
        # Arrange: Prepare market
        self.cache.add_instrument(_USDJPY_SIM)
        snapshot = TestDataStubs.order_book_snapshot(
            instrument=_USDJPY_SIM,
            bid_size=100_000,
            ask_size=100_000,
        )
        self.data_engine.process(snapshot)
        self.exchange.process_order_book_deltas(snapshot)

        order = self.strategy.order_factory.limit(
            instrument_id=_USDJPY_SIM.id,
            order_side=OrderSide.BUY,
            quantity=_USDJPY_SIM.make_qty(100_000),
            price=_USDJPY_SIM.make_price(100.0),
            post_only=False,
        )
        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Act
        delta = TestDataStubs.order_book_delta(
            instrument_id=_USDJPY_SIM.id,
            action=BookAction.ADD,
            order=TestDataStubs.order(
                instrument=_USDJPY_SIM,
                side=OrderSide.SELL,
                price=98.0,
                size=50_000,
            ),
        )
        self.exchange.process_order_book_delta(delta)

        # Assert
        assert order.status == OrderStatus.PARTIALLY_FILLED
        assert order.filled_qty == _USDJPY_SIM.make_qty(50_000)
        assert order.avg_px == Decimal("100.000")  # <-- Fills at limit price

    def test_passive_multiple_fills_sell(self):
        # Arrange: Prepare market
        self.cache.add_instrument(_USDJPY_SIM)
        snapshot = TestDataStubs.order_book_snapshot(
            instrument=_USDJPY_SIM,
            bid_size=100_000,
            ask_size=100_000,
        )
        self.data_engine.process(snapshot)
        self.exchange.process_order_book_deltas(snapshot)

        order = self.strategy.order_factory.limit(
            instrument_id=_USDJPY_SIM.id,
            order_side=OrderSide.SELL,
            quantity=_USDJPY_SIM.make_qty(100_000),
            price=_USDJPY_SIM.make_price(101.0),
            post_only=False,
        )
        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Act
        deltas = TestDataStubs.order_book_snapshot(
            instrument=_USDJPY_SIM,
            ask_price=104.0,
            bid_price=103.0,
            bid_size=50_000,
            ask_size=50_000,
        )
        self.exchange.process_order_book_deltas(deltas)

        # Assert
        assert order.status == OrderStatus.FILLED
        assert order.events[3].last_px == _USDJPY_SIM.make_price(101.0)
        assert order.events[3].last_qty == _USDJPY_SIM.make_qty(50_000)
        assert order.events[4].last_px == _USDJPY_SIM.make_price(101.0)
        assert order.events[4].last_qty == _USDJPY_SIM.make_qty(50_000)
        assert order.avg_px == Decimal("101.000")  # <-- Fills at limit price

    def test_passive_multiple_fills_buy(self):
        # Arrange: Prepare market
        self.cache.add_instrument(_USDJPY_SIM)
        snapshot = TestDataStubs.order_book_snapshot(
            instrument=_USDJPY_SIM,
            bid_size=100_000,
            ask_size=100_000,
        )
        self.data_engine.process(snapshot)
        self.exchange.process_order_book_deltas(snapshot)

        order = self.strategy.order_factory.limit(
            instrument_id=_USDJPY_SIM.id,
            order_side=OrderSide.BUY,
            quantity=_USDJPY_SIM.make_qty(100_000),
            price=_USDJPY_SIM.make_price(100.0),
            post_only=False,
        )
        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Act
        deltas = TestDataStubs.order_book_snapshot(
            instrument=_USDJPY_SIM,
            ask_price=98.0,
            bid_price=96.0,
            bid_size=50_000,
            ask_size=50_000,
        )
        self.exchange.process_order_book_deltas(deltas)

        # Assert
        assert order.status == OrderStatus.FILLED
        assert order.events[3].last_px == _USDJPY_SIM.make_price(100.0)
        assert order.events[3].last_qty == _USDJPY_SIM.make_qty(50_000)
        assert order.events[4].last_px == _USDJPY_SIM.make_price(100.0)
        assert order.events[4].last_qty == _USDJPY_SIM.make_qty(50_000)
        assert order.avg_px == Decimal("100.000")  # <-- Fills at limit price
