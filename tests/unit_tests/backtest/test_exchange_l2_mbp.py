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

from decimal import Decimal

import pytest

from nautilus_trader.backtest.exchange import SimulatedExchange
from nautilus_trader.backtest.execution_client import BacktestExecClient
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.backtest.models import LatencyModel
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.enums import LogLevel
from nautilus_trader.common.logging import Logger
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orderbook import OrderBook
from nautilus_trader.msgbus.bus import MessageBus
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.risk.engine import RiskEngine
from nautilus_trader.test_kit.mocks.strategies import MockStrategy
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


SIM = Venue("SIM")
USDJPY_SIM = TestInstrumentProvider.default_fx_ccy("USD/JPY")


class TestL2OrderBookExchange:
    def setup(self):
        # Fixture Setup
        self.clock = TestClock()
        self.logger = Logger(
            clock=self.clock,
            level_stdout=LogLevel.DEBUG,
            bypass=True,
        )

        self.trader_id = TestIdStubs.trader_id()

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
            logger=self.logger,
        )

        self.cache = TestComponentStubs.cache()

        self.portfolio = Portfolio(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

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

        self.risk_engine = RiskEngine(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.exchange = SimulatedExchange(
            venue=SIM,
            oms_type=OmsType.HEDGING,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            starting_balances=[Money(1_000_000, USD)],
            default_leverage=Decimal(50),
            leverages={},
            instruments=[USDJPY_SIM],
            modules=[],
            fill_model=FillModel(),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
            book_type=BookType.L2_MBP,  # <-- L2 MBP book
            latency_model=LatencyModel(0),
        )

        self.exec_client = BacktestExecClient(
            exchange=self.exchange,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Prepare components
        self.cache.add_instrument(USDJPY_SIM)
        self.cache.add_order_book(
            OrderBook(
                instrument_id=USDJPY_SIM.id,
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
            logger=self.logger,
        )

        self.exchange.reset()
        self.data_engine.start()
        self.exec_engine.start()
        self.strategy.start()

    def test_submit_limit_order_aggressive_multiple_levels(self):
        # Arrange: Prepare market
        self.cache.add_instrument(USDJPY_SIM)

        quote = QuoteTick(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("110.000"),
            ask=Price.from_str("110.010"),
            bid_size=Quantity.from_int(1_500_000),
            ask_size=Quantity.from_int(1_500_000),
            ts_event=0,
            ts_init=0,
        )
        self.data_engine.process(quote)
        snapshot = TestDataStubs.order_book_snapshot(
            instrument_id=USDJPY_SIM.id,
            bid_size=10000,
            ask_size=10000,
        )
        self.data_engine.process(snapshot)
        self.exchange.process_order_book_deltas(snapshot)

        # Create order
        order = self.strategy.order_factory.limit(
            instrument_id=USDJPY_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(20_000),
            price=Price.from_int(20),
            post_only=False,
        )

        # Act
        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Assert
        assert order.status == OrderStatus.FILLED
        assert order.filled_qty == Decimal("20000.0")  # No slippage
        assert order.avg_px == 15.333333333333334
        assert self.exchange.get_account().balance_total(USD) == Money(999999.94, USD)

    def test_aggressive_partial_fill(self):
        # Arrange: Prepare market
        self.cache.add_instrument(USDJPY_SIM)

        quote = QuoteTick(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("110.000"),
            ask=Price.from_str("110.010"),
            bid_size=Quantity.from_int(1_500_000),
            ask_size=Quantity.from_int(1_500_000),
            ts_event=0,
            ts_init=0,
        )
        self.data_engine.process(quote)
        snapshot = TestDataStubs.order_book_snapshot(
            instrument_id=USDJPY_SIM.id,
            bid_size=10_000,
            ask_size=10_000,
        )
        print(str(snapshot))
        self.data_engine.process(snapshot)
        self.exchange.process_order_book_deltas(snapshot)

        # Act
        order = self.strategy.order_factory.limit(
            instrument_id=USDJPY_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(70_000),
            price=Price.from_int(20),
            post_only=False,
        )
        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Assert
        assert order.status == OrderStatus.PARTIALLY_FILLED
        assert order.filled_qty == Quantity.from_str("60000.0")  # No slippage
        assert order.avg_px == 15.933333333333334
        assert self.exchange.get_account().balance_total(USD) == Money(999999.83, USD)

    def test_post_only_insert(self):
        # Arrange: Prepare market
        self.cache.add_instrument(USDJPY_SIM)
        # Market is 10 @ 15
        snapshot = TestDataStubs.order_book_snapshot(
            instrument_id=USDJPY_SIM.id,
            bid_size=1000,
            ask_size=1000,
        )
        self.data_engine.process(snapshot)
        self.exchange.process_order_book_deltas(snapshot)

        # Act
        order = self.strategy.order_factory.limit(
            instrument_id=USDJPY_SIM.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(2_000),
            price=Price.from_str("14"),
            post_only=True,
        )
        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Assert
        assert order.status == OrderStatus.ACCEPTED

    # TODO - Need to discuss how we are going to support passive quotes trading now
    @pytest.mark.skip()
    def test_passive_partial_fill(self):
        # Arrange: Prepare market
        self.cache.add_instrument(USDJPY_SIM)
        # Market is 10 @ 15
        snapshot = TestDataStubs.order_book_snapshot(
            instrument_id=USDJPY_SIM.id,
            bid_size=1000,
            ask_size=1000,
        )
        self.data_engine.process(snapshot)
        self.exchange.process_order_book_deltas(snapshot)

        order = self.strategy.order_factory.limit(
            instrument_id=USDJPY_SIM.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(1_000),
            price=Price.from_str("14"),
            post_only=False,
        )
        self.strategy.submit_order(order)

        # Act
        tick = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid=15.0,
            ask=16.0,
            bid_size=1_000,
            ask_size=1_000,
        )
        # New tick will be in cross with our order
        self.exchange.process_quote_tick(tick)

        # Assert
        assert order.status == OrderStatus.PARTIALLY_FILLED
        assert order.filled_qty == Quantity.from_str("1000.0")
        assert order.avg_px == Decimal("15.0")

    # TODO - Need to discuss how we are going to support passive quotes trading now
    @pytest.mark.skip()
    def test_passive_fill_on_trade_tick(self):
        # Arrange: Prepare market
        # Market is 10 @ 15
        snapshot = TestDataStubs.order_book_snapshot(
            instrument_id=USDJPY_SIM.id,
            bid_size=1000,
            ask_size=1000,
        )
        self.data_engine.process(snapshot)
        self.exchange.process_order_book_deltas(snapshot)

        order = self.strategy.order_factory.limit(
            instrument_id=USDJPY_SIM.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(2_000),
            price=Price.from_str("14"),
            post_only=False,
        )
        self.strategy.submit_order(order)

        # Act
        tick1 = TradeTick(
            instrument_id=USDJPY_SIM.id,
            price=Price.from_str("14.0"),
            size=Quantity.from_int(1_000),
            aggressor_side=AggressorSide.SELLER,
            trade_id=TradeId("123456789"),
            ts_event=0,
            ts_init=0,
        )
        self.exchange.process_quote_tick(tick1)

        # Assert
        assert order.status == OrderStatus.PARTIALLY_FILLED
        assert order.filled_qty == Quantity.from_int(1000.0)  # No slippage
        assert order.avg_px == Decimal("14.0")
