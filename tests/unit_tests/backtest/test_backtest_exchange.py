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

from datetime import timedelta
from decimal import Decimal

import pytest

from nautilus_trader.backtest.exchange import SimulatedExchange
from nautilus_trader.backtest.execution_client import BacktestExecClient
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.backtest.models import LatencyModel
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.enums import LogLevel
from nautilus_trader.common.logging import Logger
from nautilus_trader.config import ExecEngineConfig
from nautilus_trader.config import RiskEngineConfig
from nautilus_trader.core.datetime import secs_to_nanos
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.model.currencies import JPY
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.events.order import OrderAccepted
from nautilus_trader.model.events.order import OrderCanceled
from nautilus_trader.model.events.order import OrderFilled
from nautilus_trader.model.events.order import OrderInitialized
from nautilus_trader.model.events.order import OrderPendingCancel
from nautilus_trader.model.events.order import OrderPendingUpdate
from nautilus_trader.model.events.order import OrderRejected
from nautilus_trader.model.events.order import OrderSubmitted
from nautilus_trader.model.events.order import OrderUpdated
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.msgbus.bus import MessageBus
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.risk.engine import RiskEngine
from nautilus_trader.test_kit.mocks.strategies import MockStrategy
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs import UNIX_EPOCH
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
USDJPY_SIM = TestInstrumentProvider.default_fx_ccy("USD/JPY")


class TestSimulatedExchange:
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
            clock=self.clock,
            cache=self.cache,
            logger=self.logger,
        )

        self.exec_engine = ExecutionEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
            config=ExecEngineConfig(debug=True),
        )

        self.risk_engine = RiskEngine(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
            config=RiskEngineConfig(debug=True),
        )

        self.exchange = SimulatedExchange(
            venue=Venue("SIM"),
            oms_type=OmsType.HEDGING,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            starting_balances=[Money(1_000_000, USD)],
            default_leverage=Decimal(50),
            leverages={AUDUSD_SIM.id: Decimal(10)},
            instruments=[USDJPY_SIM],
            modules=[],
            fill_model=FillModel(),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
            latency_model=LatencyModel(0),
        )

        self.exec_client = BacktestExecClient(
            exchange=self.exchange,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Wire up components
        self.exec_engine.register_client(self.exec_client)
        self.exchange.register_client(self.exec_client)

        self.cache.add_instrument(USDJPY_SIM)

        # Create mock strategy
        self.strategy = MockStrategy(bar_type=TestDataStubs.bartype_usdjpy_1min_bid())
        self.strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Start components
        self.exchange.reset()
        self.data_engine.start()
        self.exec_engine.start()
        self.strategy.start()

    def test_repr(self):
        # Arrange, Act, Assert
        assert (
            repr(self.exchange)
            == "SimulatedExchange(id=SIM, oms_type=HEDGING, account_type=MARGIN)"
        )

    def test_set_fill_model(self):
        # Arrange
        fill_model = FillModel()

        # Act
        self.exchange.set_fill_model(fill_model)

        # Assert
        assert self.exchange.fill_model == fill_model

    def test_get_matching_engines_when_engine_returns_expected_dict(self):
        # Arrange, Act
        matching_engines = self.exchange.get_matching_engines()

        # Assert
        assert isinstance(matching_engines, dict)
        assert len(matching_engines) == 1
        assert list(matching_engines.keys()) == [USDJPY_SIM.id]

    def test_get_matching_engine_when_no_engine_for_instrument_returns_none(self):
        # Arrange, Act
        matching_engine = self.exchange.get_matching_engine(USDJPY_SIM.id)

        # Assert
        assert matching_engine.instrument == USDJPY_SIM

    def test_get_books_with_one_instrument_returns_one_book(self):
        # Arrange, Act
        books = self.exchange.get_books()

        # Assert
        assert len(books) == 1

    def test_get_open_orders_when_no_orders_returns_empty_list(self):
        # Arrange, Act
        orders = self.exchange.get_open_orders()

        # Assert
        assert orders == []

    def test_get_open_bid_orders_when_no_orders_returns_empty_list(self):
        # Arrange, Act
        orders = self.exchange.get_open_bid_orders()

        # Assert
        assert orders == []

    def test_get_open_ask_orders_when_no_orders_returns_empty_list(self):
        # Arrange, Act
        orders = self.exchange.get_open_ask_orders()

        # Assert
        assert orders == []

    def test_get_open_bid_orders_with_instrument_when_no_orders_returns_empty_list(self):
        # Arrange, Act
        orders = self.exchange.get_open_bid_orders(AUDUSD_SIM.id)

        # Assert
        assert orders == []

    def test_get_open_ask_orders_with_instrument_when_no_orders_returns_empty_list(self):
        # Arrange, Act
        orders = self.exchange.get_open_ask_orders(AUDUSD_SIM.id)

        # Assert
        assert orders == []

    def test_process_quote_tick_updates_market(self):
        # Arrange
        tick = TestDataStubs.quote_tick_3decimal(instrument_id=USDJPY_SIM.id)

        # Act
        self.exchange.process_quote_tick(tick)

        # Assert
        assert self.exchange.get_book(USDJPY_SIM.id).type == BookType.L1_TBBO
        assert self.exchange.best_ask_price(USDJPY_SIM.id) == Price.from_str("90.005")
        assert self.exchange.best_bid_price(USDJPY_SIM.id) == Price.from_str("90.002")

    def test_process_trade_tick_updates_market(self):
        # Arrange
        tick1 = TestDataStubs.trade_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            aggressor_side=AggressorSide.BUYER,
        )

        tick2 = TestDataStubs.trade_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            aggressor_side=AggressorSide.SELLER,
        )

        # Act
        self.exchange.process_trade_tick(tick1)
        self.exchange.process_trade_tick(tick2)

        # Assert
        assert self.exchange.best_bid_price(USDJPY_SIM.id) == Price.from_str("1.001")
        assert self.exchange.best_ask_price(USDJPY_SIM.id) == Price.from_str("1.001")

    def test_submit_buy_limit_order_with_no_market_accepts_order(self):
        # Arrange
        order = self.strategy.order_factory.limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("110.000"),
        )

        # Act
        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Assert
        assert order.status == OrderStatus.ACCEPTED
        assert len(self.strategy.store) == 3
        assert isinstance(self.strategy.store[2], OrderAccepted)

    def test_submit_buy_limit_order_with_immediate_modify(self):
        # Arrange
        order = self.strategy.order_factory.limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("110.000"),
        )

        # Act
        self.strategy.submit_order(order)
        self.strategy.modify_order(order, price=Price.from_str("110.010"))
        self.exchange.process(0)

        # Assert
        assert order.status == OrderStatus.ACCEPTED
        assert len(self.strategy.store) == 5
        assert isinstance(self.strategy.store[0], OrderInitialized)
        assert isinstance(self.strategy.store[1], OrderSubmitted)
        assert isinstance(self.strategy.store[2], OrderPendingUpdate)  # <-- Now in-flight
        assert isinstance(self.strategy.store[3], OrderAccepted)
        assert isinstance(self.strategy.store[4], OrderUpdated)

    def test_submit_buy_limit_order_with_immediate_cancel(self):
        # Arrange
        order = self.strategy.order_factory.limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("110.000"),
        )

        # Act
        self.strategy.submit_order(order)
        self.strategy.cancel_order(order)
        self.exchange.process(0)

        # Assert
        assert order.status == OrderStatus.CANCELED
        assert len(self.strategy.store) == 5
        assert isinstance(self.strategy.store[0], OrderInitialized)
        assert isinstance(self.strategy.store[1], OrderSubmitted)
        assert isinstance(self.strategy.store[2], OrderPendingCancel)  # <-- Now in-flight
        assert isinstance(self.strategy.store[3], OrderAccepted)
        assert isinstance(self.strategy.store[4], OrderCanceled)

    def test_submit_sell_limit_order_with_no_market_accepts_order(self):
        # Arrange
        order = self.strategy.order_factory.limit(
            USDJPY_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
            Price.from_str("110.000"),
        )

        # Act
        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Assert
        assert order.status == OrderStatus.ACCEPTED
        assert len(self.strategy.store) == 3
        assert isinstance(self.strategy.store[2], OrderAccepted)

    def test_submit_buy_market_order_with_no_market_rejects_order(self):
        # Arrange
        order = self.strategy.order_factory.market(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        # Act
        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Assert
        assert order.status == OrderStatus.REJECTED
        assert len(self.strategy.store) == 3
        assert isinstance(self.strategy.store[2], OrderRejected)

    def test_submit_sell_market_order_with_no_market_rejects_order(self):
        # Arrange
        order = self.strategy.order_factory.market(
            USDJPY_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
        )

        # Act
        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Assert
        assert order.status == OrderStatus.REJECTED
        assert len(self.strategy.store) == 3
        assert isinstance(self.strategy.store[2], OrderRejected)

    def test_submit_order_with_invalid_price_gets_rejected(self):
        # Arrange: Prepare market
        tick = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.exchange.process_quote_tick(tick)
        self.portfolio.update_quote_tick(tick)

        order = self.strategy.order_factory.stop_market(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("90.005"),  # Price at ask
        )

        # Act
        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Assert
        assert order.status == OrderStatus.REJECTED

    def test_submit_order_when_quantity_below_min_then_gets_denied(self):
        # Arrange: Prepare market
        order = self.strategy.order_factory.market(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(1),  # <-- Below minimum quantity for instrument
        )

        # Act
        self.strategy.submit_order(order)

        # Assert
        assert order.status == OrderStatus.DENIED

    def test_submit_order_when_quantity_above_max_then_gets_denied(self):
        # Arrange: Prepare market
        order = self.strategy.order_factory.market(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity(1e8, 0),  # <-- Above maximum quantity for instrument
        )

        # Act
        self.strategy.submit_order(order)

        # Assert
        assert order.status == OrderStatus.DENIED

    def test_submit_market_order(self):
        # Arrange: Prepare market
        tick = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        # Create order
        order = self.strategy.order_factory.market(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        # Act
        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Assert
        assert order.status == OrderStatus.FILLED
        assert order.avg_px == 90.005  # No slippage

    def test_submit_market_order_then_immediately_cancel_submits_and_fills(self):
        # Arrange: Prepare market
        tick = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        # Create order
        order = self.strategy.order_factory.market(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        # Act
        self.strategy.submit_order(order)
        self.strategy.cancel_order(order)
        self.exchange.process(0)

        # Assert
        assert order.status == OrderStatus.FILLED

    def test_submit_market_order_with_fok_time_in_force_cancels_immediately(self):
        # Arrange: Prepare market
        tick = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
            bid_size=Quantity.from_int(500_000),
            ask_size=Quantity.from_int(500_000),
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        # Create order
        order = self.strategy.order_factory.market(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(1_000_000),
            time_in_force=TimeInForce.FOK,
        )

        # Act
        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Assert
        assert order.status == OrderStatus.CANCELED
        assert order.quantity == Quantity.from_int(1_000_000)
        assert order.filled_qty == Quantity.from_int(0)

    def test_submit_market_order_with_ioc_time_in_force_cancels_remaining_qty(self):
        # Arrange: Prepare market
        tick = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
            bid_size=Quantity.from_int(500_000),
            ask_size=Quantity.from_int(500_000),
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        # Create order
        order = self.strategy.order_factory.market(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(1_000_000),
            time_in_force=TimeInForce.IOC,
        )

        # Act
        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Assert
        assert order.status == OrderStatus.CANCELED
        assert order.quantity == Quantity.from_int(1_000_000)
        assert order.filled_qty == Quantity.from_int(500_000)

    def test_submit_limit_order_then_immediately_cancel_submits_then_cancels(self):
        # Arrange: Prepare market
        tick = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        order = self.strategy.order_factory.limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("90.000"),
        )

        # Act
        self.strategy.submit_order(order)
        self.strategy.cancel_order(order)
        self.exchange.process(0)

        # Assert
        assert order.status == OrderStatus.CANCELED
        assert len(self.exchange.get_open_orders()) == 0

    def test_submit_post_only_limit_order_when_marketable_then_rejects(self):
        # Arrange: Prepare market
        tick = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        order = self.strategy.order_factory.limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("90.005"),
            post_only=True,
        )

        # Act
        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Assert
        assert order.status == OrderStatus.REJECTED
        assert len(self.exchange.get_open_orders()) == 0

    def test_submit_limit_order(self):
        # Arrange: Prepare market
        tick = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        order = self.strategy.order_factory.limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("90.001"),
        )

        # Act
        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Assert
        assert order.status == OrderStatus.ACCEPTED
        assert len(self.exchange.get_open_orders()) == 1
        assert order in self.exchange.get_open_orders()

    def test_submit_limit_order_with_ioc_time_in_force_immediately_cancels(self):
        # Arrange: Prepare market
        tick = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
            bid_size=Quantity.from_int(500_000),
            ask_size=Quantity.from_int(500_000),
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        # Create order
        order = self.strategy.order_factory.limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(1_000_000),
            Price.from_int(1),
            time_in_force=TimeInForce.IOC,
        )

        # Act
        self.strategy.submit_order(order)
        self.exchange.process(0)
        self.exchange.process(0)
        self.exchange.process(0)

        # Assert
        assert order.status == OrderStatus.CANCELED
        assert order.quantity == Quantity.from_int(1_000_000)
        assert order.filled_qty == Quantity.from_int(0)

    def test_submit_limit_order_with_fok_time_in_force_immediately_cancels(self):
        # Arrange: Prepare market
        tick = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
            bid_size=Quantity.from_int(500_000),
            ask_size=Quantity.from_int(500_000),
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        # Create order
        order = self.strategy.order_factory.limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(1_000_000),
            Price.from_int(1),
            time_in_force=TimeInForce.FOK,
        )

        # Act
        self.strategy.submit_order(order)
        self.exchange.process(0)
        self.exchange.process(0)
        self.exchange.process(0)

        # Assert
        assert order.status == OrderStatus.CANCELED
        assert order.quantity == Quantity.from_int(1_000_000)
        assert order.filled_qty == Quantity.from_int(0)

    def test_submit_market_to_limit_order_less_than_available_top_of_book(self):
        # Arrange: Prepare market
        tick = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        order = self.strategy.order_factory.market_to_limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        # Act
        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Assert
        assert order.status == OrderStatus.FILLED
        assert order.price == Price.from_str("90.005")
        assert len(self.exchange.get_open_orders()) == 0

    def test_submit_market_to_limit_order_greater_than_available_top_of_book(self):
        # Arrange: Prepare market
        tick = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        order = self.strategy.order_factory.market_to_limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(2_000_000),
        )

        # Act
        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Assert
        assert order.status == OrderStatus.PARTIALLY_FILLED
        assert order.price == Price.from_str("90.005")
        assert order.filled_qty == Quantity.from_int(1_000_000)
        assert order.leaves_qty == Quantity.from_int(1_000_000)
        assert len(self.exchange.get_open_orders()) == 1

    def test_modify_market_to_limit_order_after_filling_initial_quantity(self):
        # Arrange: Prepare market
        tick = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        order = self.strategy.order_factory.market_to_limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(2_000_000),
        )

        # Act
        self.strategy.submit_order(order)
        self.exchange.process(0)

        self.strategy.modify_order(
            order,
            quantity=Quantity.from_int(1_500_000),
            price=Price.from_str("90.000"),
        )
        self.exchange.process(0)

        # Assert
        assert order.status == OrderStatus.PARTIALLY_FILLED
        assert order.price == Price.from_str("90.000")
        assert order.filled_qty == Quantity.from_int(1_000_000)
        assert order.leaves_qty == Quantity.from_int(500_000)
        assert len(self.exchange.get_open_orders()) == 1

    def test_submit_market_to_limit_order_becomes_limit_then_fills_remaining(self):
        # Arrange: Prepare market
        tick = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        order = self.strategy.order_factory.market_to_limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(2_000_000),
        )

        # Act
        self.strategy.submit_order(order)
        self.exchange.process(0)

        tick = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),  # <-- hit bid again
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        # Assert
        assert order.status == OrderStatus.FILLED
        assert order.price == Price.from_str("90.005")
        assert order.filled_qty == Quantity.from_int(2_000_000)
        assert order.leaves_qty == Quantity.from_int(0)
        assert len(self.exchange.get_open_orders()) == 0

    def test_submit_market_if_touched_order(self):
        # Arrange: Prepare market
        tick = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        order = self.strategy.order_factory.market_if_touched(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("90.000"),
        )

        # Act
        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Assert
        assert order.status == OrderStatus.ACCEPTED
        assert len(self.exchange.get_open_orders()) == 1
        assert order in self.exchange.get_open_orders()

    def test_submit_limit_if_touched_order(self):
        # Arrange: Prepare market
        tick = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        order = self.strategy.order_factory.limit_if_touched(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("90.010"),
            Price.from_str("90.000"),
        )

        # Act
        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Assert
        assert order.status == OrderStatus.ACCEPTED
        assert len(self.exchange.get_open_orders()) == 1
        assert order in self.exchange.get_open_orders()

    def test_submit_limit_order_when_marketable_then_fills(self):
        # Arrange: Prepare market
        tick = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        order = self.strategy.order_factory.limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("90.010"),
            post_only=False,  # <-- Can be liquidity TAKER
        )

        # Act
        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Assert
        assert order.status == OrderStatus.FILLED
        assert order.avg_px == 90.005  # <-- fills at ask
        assert order.liquidity_side == LiquiditySide.TAKER
        assert len(self.exchange.get_open_orders()) == 0

    def test_submit_limit_order_fills_at_correct_price(self):
        # Arrange: Prepare market
        tick = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        order = self.strategy.order_factory.limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("90.000"),  # <-- Limit price above the ask
            post_only=False,  # <-- Can be liquidity TAKER
        )

        # Act
        self.strategy.submit_order(order)
        self.exchange.process(0)

        tick = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("89.900"),
            ask=Price.from_str("89.950"),
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)
        self.exchange.process(0)

        # Assert
        assert order.status == OrderStatus.FILLED
        assert order.avg_px == 90.000

    def test_submit_limit_order_fills_at_most_book_volume(self):
        # Arrange: Prepare market
        tick = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        order = self.strategy.order_factory.limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(2_000_000),  # <-- Order volume greater than available ask volume
            Price.from_str("90.010"),
            post_only=False,  # <-- Can be liquidity TAKER
        )

        # Act
        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Assert
        assert order.status == OrderStatus.PARTIALLY_FILLED
        assert order.filled_qty == 1_000_000

    def test_submit_market_if_touched_order_then_fills(self):
        # Arrange: Prepare market
        tick = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        order = self.strategy.order_factory.market_if_touched(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(10_000),  # <-- Order volume greater than available ask volume
            Price.from_str("90.000"),
        )

        # Act
        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Quantity is refreshed -> Ensure we don't trade the entire amount
        tick = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            ask=Price.from_str("90.000"),
            ask_size=Quantity.from_int(10_000),
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        # Assert
        assert order.status == OrderStatus.FILLED
        assert order.filled_qty == 10_000

    @pytest.mark.parametrize(
        ("side", "price", "trigger_price"),
        [
            [OrderSide.BUY, Price.from_str("90.000"), Price.from_str("90.000")],
            [OrderSide.SELL, Price.from_str("90.010"), Price.from_str("90.010")],
        ],
    )
    def test_submit_limit_if_touched_order_then_fills(self, side, price, trigger_price):
        # Arrange: Prepare market
        tick = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.000"),
            ask=Price.from_str("90.010"),
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        order = self.strategy.order_factory.limit_if_touched(
            USDJPY_SIM.id,
            side,
            Quantity.from_int(10_000),  # <-- Order volume greater than available ask volume
            price=price,
            trigger_price=trigger_price,
        )

        # Act
        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Quantity is refreshed -> Ensure we don't trade the entire amount
        tick = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.010"),  # <-- in cross for purpose of test
            ask=Price.from_str("90.000"),
            bid_size=Quantity.from_int(10_000),
            ask_size=Quantity.from_int(10_000),
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        # Assert
        assert order.status == OrderStatus.FILLED
        assert order.filled_qty == 10_000

    @pytest.mark.parametrize(
        ("side", "price"),
        [
            [OrderSide.BUY, Price.from_str("90.010")],
            [OrderSide.SELL, Price.from_str("90.000")],
        ],
    )
    def test_submit_limit_order_fills_at_most_order_volume(self, side, price):
        # Arrange: Prepare market
        tick = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.005"),
            ask=Price.from_str("90.005"),
            bid_size=Quantity.from_int(10_000),
            ask_size=Quantity.from_int(10_000),
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        order = self.strategy.order_factory.limit(
            USDJPY_SIM.id,
            side,
            Quantity.from_int(15_000),  # <-- Order volume greater than available ask volume
            price,
            post_only=False,  # <-- Can be liquidity TAKER
        )

        # Partially fill order
        self.strategy.submit_order(order)
        self.exchange.process(0)
        assert order.status == OrderStatus.PARTIALLY_FILLED
        assert order.filled_qty == 10_000

        # Quantity is refreshed -> Ensure we don't trade the entire amount
        tick = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.005"),
            ask=Price.from_str("90.005"),
            bid_size=Quantity.from_int(10_000),
            ask_size=Quantity.from_int(10_000),
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        # Assert
        assert order.status == OrderStatus.FILLED
        assert order.filled_qty == 15_000

    @pytest.mark.parametrize(
        ("side", "trigger_price"),
        [
            [OrderSide.BUY, Price.from_str("90.005")],
            [OrderSide.SELL, Price.from_str("90.002")],
        ],
    )
    def test_submit_stop_market_order_inside_market_rejects(self, side, trigger_price):
        # Arrange: Prepare market
        tick = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        order = self.strategy.order_factory.stop_market(
            USDJPY_SIM.id,
            side,
            Quantity.from_int(100_000),
            trigger_price,
        )

        # Act
        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Assert
        assert order.status == OrderStatus.REJECTED
        assert len(self.exchange.get_open_orders()) == 0

    @pytest.mark.parametrize(
        ("side", "price", "trigger_price"),
        [
            [OrderSide.BUY, Price.from_str("90.005"), Price.from_str("90.005")],
            [OrderSide.SELL, Price.from_str("90.002"), Price.from_str("90.002")],
        ],
    )
    def test_submit_stop_limit_order_inside_market_rejects(self, side, price, trigger_price):
        # Arrange: Prepare market
        tick = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        order = self.strategy.order_factory.stop_limit(
            USDJPY_SIM.id,
            side,
            Quantity.from_int(100_000),
            price=price,
            trigger_price=trigger_price,
        )

        # Act
        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Assert
        assert order.status == OrderStatus.REJECTED
        assert len(self.exchange.get_open_orders()) == 0

    @pytest.mark.parametrize(
        ("side", "trigger_price"),
        [
            [OrderSide.BUY, Price.from_str("90.010")],
            [OrderSide.SELL, Price.from_str("90.000")],
        ],
    )
    def test_submit_stop_market_order(self, side, trigger_price):
        # Arrange: Prepare market
        tick = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        order = self.strategy.order_factory.stop_market(
            USDJPY_SIM.id,
            side,
            Quantity.from_int(100_000),
            trigger_price=trigger_price,
        )

        # Act
        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Assert
        assert order.status == OrderStatus.ACCEPTED
        assert len(self.exchange.get_open_orders()) == 1
        assert order in self.exchange.get_open_orders()

    @pytest.mark.parametrize(
        ("side", "price", "trigger_price"),
        [
            [OrderSide.BUY, Price.from_str("90.010"), Price.from_str("90.002")],
            [OrderSide.SELL, Price.from_str("90.000"), Price.from_str("90.005")],
        ],
    )
    def test_submit_stop_limit_order_when_inside_market_rejects(self, side, price, trigger_price):
        # Arrange: Prepare market
        tick = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        order = self.strategy.order_factory.stop_limit(
            USDJPY_SIM.id,
            side,
            Quantity.from_int(100_000),
            price=price,
            trigger_price=trigger_price,
        )

        # Act
        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Assert
        assert order.status == OrderStatus.REJECTED
        assert len(self.exchange.get_open_orders()) == 0

    @pytest.mark.parametrize(
        ("side", "price", "trigger_price"),
        [
            [OrderSide.BUY, Price.from_str("90.000"), Price.from_str("90.010")],
            [OrderSide.SELL, Price.from_str("89.980"), Price.from_str("89.990")],
        ],
    )
    def test_submit_stop_limit_order(self, side, price, trigger_price):
        # Arrange: Prepare market
        tick = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        order = self.strategy.order_factory.stop_limit(
            USDJPY_SIM.id,
            side,
            Quantity.from_int(100_000),
            price=price,
            trigger_price=trigger_price,
        )

        # Act
        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Assert
        assert order.status == OrderStatus.ACCEPTED
        assert len(self.exchange.get_open_orders()) == 1
        assert order in self.exchange.get_open_orders()

    @pytest.mark.parametrize(
        "side",
        [
            OrderSide.BUY,
            OrderSide.SELL,
        ],
    )
    def test_submit_reduce_only_order_when_no_position_rejects(self, side):
        # Arrange: Prepare market
        tick = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        order = self.strategy.order_factory.market(
            USDJPY_SIM.id,
            side,
            Quantity.from_int(100_000),
            reduce_only=True,
        )

        # Act
        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Assert
        assert order.status == OrderStatus.REJECTED
        assert len(self.exchange.get_open_orders()) == 0

    @pytest.mark.parametrize(
        "side",
        [
            OrderSide.BUY,
            OrderSide.SELL,
        ],
    )
    def test_submit_reduce_only_order_when_would_increase_position_rejects(self, side):
        # Arrange: Prepare market
        tick = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        order1 = self.strategy.order_factory.market(
            USDJPY_SIM.id,
            side,
            Quantity.from_int(100_000),
            reduce_only=False,
        )

        order2 = self.strategy.order_factory.market(
            USDJPY_SIM.id,
            side,
            Quantity.from_int(100_000),
            reduce_only=True,  # <-- reduce only set
        )

        self.strategy.submit_order(order1)
        self.exchange.process(0)

        # Act
        self.strategy.submit_order(order2)
        self.exchange.process(0)

        # Assert
        assert order1.status == OrderStatus.FILLED
        assert order2.status == OrderStatus.REJECTED
        assert len(self.exchange.get_open_orders()) == 0

    def test_cancel_stop_order(self):
        # Arrange: Prepare market
        tick = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        order = self.strategy.order_factory.stop_market(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("90.010"),
        )

        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Act
        self.strategy.cancel_order(order)
        self.exchange.process(0)

        # Assert
        assert order.status == OrderStatus.CANCELED
        assert len(self.exchange.get_open_orders()) == 0

    def test_cancel_stop_order_when_order_does_not_exist_generates_cancel_reject(self):
        # Arrange
        command = CancelOrder(
            trader_id=self.trader_id,
            strategy_id=StrategyId("SCALPER-001"),
            instrument_id=USDJPY_SIM.id,
            client_order_id=ClientOrderId("O-123456"),
            venue_order_id=VenueOrderId("001"),
            command_id=UUID4(),
            ts_init=0,
        )

        # Act
        self.exchange.send(command)
        self.exchange.process(0)

        # Assert
        assert self.exec_engine.event_count == 1

    def test_cancel_all_orders_with_no_side_filter_cancels_all(self):
        # Arrange: Prepare market
        tick = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        order1 = self.strategy.order_factory.limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("90.000"),
        )

        order2 = self.strategy.order_factory.limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("90.000"),
        )

        self.strategy.submit_order(order1)
        self.strategy.submit_order(order2)
        self.exchange.process(0)

        # Act
        self.strategy.cancel_all_orders(instrument_id=USDJPY_SIM.id)
        self.exchange.process(0)

        # Assert
        assert order1.status == OrderStatus.CANCELED
        assert order2.status == OrderStatus.CANCELED
        assert len(self.exchange.get_open_orders()) == 0

    def test_cancel_all_orders_with_buy_side_filter_cancels_all_buy_orders(self):
        # Arrange: Prepare market
        tick = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        order1 = self.strategy.order_factory.limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("90.000"),
        )

        order2 = self.strategy.order_factory.limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("90.000"),
        )

        order3 = self.strategy.order_factory.limit(
            USDJPY_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
            Price.from_str("90.010"),
        )

        self.strategy.submit_order(order1)
        self.strategy.submit_order(order2)
        self.strategy.submit_order(order3)
        self.exchange.process(0)

        # Act
        self.strategy.cancel_all_orders(
            instrument_id=USDJPY_SIM.id,
            order_side=OrderSide.BUY,
        )
        self.exchange.process(0)

        # Assert
        assert order1.status == OrderStatus.CANCELED
        assert order2.status == OrderStatus.CANCELED
        assert order3.status == OrderStatus.ACCEPTED
        assert len(self.exchange.get_open_orders()) == 1

    def test_cancel_all_orders_with_sell_side_filter_cancels_all_sell_orders(self):
        # Arrange: Prepare market
        tick = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        order1 = self.strategy.order_factory.limit(
            USDJPY_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
            Price.from_str("90.010"),
        )

        order2 = self.strategy.order_factory.limit(
            USDJPY_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
            Price.from_str("90.010"),
        )

        order3 = self.strategy.order_factory.limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("90.000"),
        )

        self.strategy.submit_order(order1)
        self.strategy.submit_order(order2)
        self.strategy.submit_order(order3)
        self.exchange.process(0)

        # Act
        self.strategy.cancel_all_orders(
            instrument_id=USDJPY_SIM.id,
            order_side=OrderSide.SELL,
        )
        self.exchange.process(0)

        # Assert
        assert order1.status == OrderStatus.CANCELED
        assert order2.status == OrderStatus.CANCELED
        assert order3.status == OrderStatus.ACCEPTED
        assert len(self.exchange.get_open_orders()) == 1

    def test_modify_stop_order_when_order_does_not_exist(self):
        # Arrange
        command = ModifyOrder(
            trader_id=self.trader_id,
            strategy_id=StrategyId("SCALPER-001"),
            instrument_id=USDJPY_SIM.id,
            client_order_id=ClientOrderId("O-123456"),
            venue_order_id=VenueOrderId("001"),
            quantity=Quantity.from_int(100_000),
            price=Price.from_str("110.000"),
            trigger_price=None,
            command_id=UUID4(),
            ts_init=0,
        )

        # Act
        self.exchange.send(command)
        self.exchange.process(0)

        # Assert
        assert self.exec_engine.event_count == 1

    def test_modify_order_with_zero_quantity_rejects_modify(self):
        # Arrange: Prepare market
        tick = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        order = self.strategy.order_factory.limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("90.001"),
            post_only=True,  # default value
        )

        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Act: Amending BUY LIMIT order limit price to ask will become marketable
        self.strategy.modify_order(order, Quantity.zero(), Price.from_str("90.001"))
        self.exchange.process(0)

        # Assert
        assert order.status == OrderStatus.ACCEPTED
        assert len(self.exchange.get_open_orders()) == 1  # Order still open
        assert order.price == Price.from_str("90.001")  # Did not update

    def test_modify_post_only_limit_order_when_marketable_then_rejects_modify(self):
        # Arrange: Prepare market
        tick = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        order = self.strategy.order_factory.limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("90.001"),
            post_only=True,  # default value
        )

        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Act: Amending BUY LIMIT order limit price to ask will become marketable
        self.strategy.modify_order(order, order.quantity, Price.from_str("90.005"))
        self.exchange.process(0)

        # Assert
        assert order.status == OrderStatus.ACCEPTED
        assert len(self.exchange.get_open_orders()) == 1  # Order still open
        assert order.price == Price.from_str("90.001")  # Did not update

    def test_modify_limit_order_when_marketable_then_fills_order(self):
        # Arrange: Prepare market
        tick = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        order = self.strategy.order_factory.limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("90.001"),
            post_only=False,  # Ensures marketable on amendment
        )

        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Act: Amending BUY LIMIT order limit price to ask will become marketable
        self.strategy.modify_order(order, order.quantity, Price.from_str("90.005"))
        self.exchange.process(0)

        # Assert
        assert order.status == OrderStatus.FILLED
        assert len(self.exchange.get_open_orders()) == 0
        assert order.avg_px == 90.005

    def test_modify_stop_market_order_when_price_inside_market_then_rejects_modify(
        self,
    ):
        # Arrange: Prepare market
        tick = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        order = self.strategy.order_factory.stop_market(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("90.010"),
        )

        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Act
        self.strategy.modify_order(order, order.quantity, trigger_price=Price.from_str("90.005"))
        self.exchange.process(0)

        # Assert
        assert order.status == OrderStatus.ACCEPTED
        assert len(self.exchange.get_open_orders()) == 1
        assert order.trigger_price == Price.from_str("90.010")

    def test_modify_stop_market_order_when_price_valid_then_updates(self):
        # Arrange: Prepare market
        tick = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        order = self.strategy.order_factory.stop_market(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("90.010"),
        )

        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Act
        self.strategy.modify_order(order, order.quantity, trigger_price=Price.from_str("90.011"))
        self.exchange.process(0)

        # Assert
        assert order.status == OrderStatus.ACCEPTED
        assert len(self.exchange.get_open_orders()) == 1
        assert order.trigger_price == Price.from_str("90.011")

    def test_modify_untriggered_stop_limit_order_when_price_inside_market_then_rejects_modify(
        self,
    ):
        # Arrange: Prepare market
        tick = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        order = self.strategy.order_factory.stop_limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            price=Price.from_str("90.000"),
            trigger_price=Price.from_str("90.010"),
        )

        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Act
        self.strategy.modify_order(order, order.quantity, Price.from_str("90.005"))
        self.exchange.process(0)

        # Assert
        assert order.status == OrderStatus.ACCEPTED
        assert len(self.exchange.get_open_orders()) == 1
        assert order.trigger_price == Price.from_str("90.010")

    def test_modify_untriggered_stop_limit_order_when_price_valid_then_amends(self):
        # Arrange: Prepare market
        tick = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        order = self.strategy.order_factory.stop_limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            price=Price.from_str("90.000"),
            trigger_price=Price.from_str("90.010"),
        )

        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Act
        self.strategy.modify_order(order, order.quantity, Price.from_str("90.011"))
        self.exchange.process(0)

        # Assert
        assert order.status == OrderStatus.ACCEPTED
        assert len(self.exchange.get_open_orders()) == 1
        assert order.price == Price.from_str("90.011")
        assert order.price == Price.from_str("90.011")

    def test_modify_triggered_post_only_stop_limit_order_when_price_inside_market_then_rejects_modify(
        self,
    ):
        # Arrange: Prepare market
        tick1 = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick1)
        self.exchange.process_quote_tick(tick1)

        order = self.strategy.order_factory.stop_limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            price=Price.from_str("90.000"),
            trigger_price=Price.from_str("90.010"),
            post_only=True,
        )

        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Trigger order
        tick2 = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.009"),
            ask=Price.from_str("90.010"),
        )
        self.data_engine.process(tick2)
        self.exchange.process_quote_tick(tick2)

        # Act
        self.strategy.modify_order(order, order.quantity, Price.from_str("90.010"))
        self.exchange.process(0)

        # Assert
        assert order.status == OrderStatus.TRIGGERED
        assert order.is_triggered
        assert len(self.exchange.get_open_orders()) == 1
        assert order.price == Price.from_str("90.000")

    def test_modify_triggered_stop_limit_order_when_price_inside_market_then_fills(
        self,
    ):
        # Arrange: Prepare market
        tick1 = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick1)
        self.exchange.process_quote_tick(tick1)

        order = self.strategy.order_factory.stop_limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            price=Price.from_str("90.000"),
            trigger_price=Price.from_str("90.010"),
            post_only=False,
        )

        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Trigger order
        tick2 = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.009"),
            ask=Price.from_str("90.010"),
        )
        self.data_engine.process(tick2)
        self.exchange.process_quote_tick(tick2)

        # Act
        self.strategy.modify_order(order, order.quantity, Price.from_str("90.010"))
        self.exchange.process(0)

        # Assert
        assert order.status == OrderStatus.FILLED
        assert order.is_triggered
        assert len(self.exchange.get_open_orders()) == 0
        assert order.price == Price.from_str("90.010")

    def test_modify_triggered_stop_limit_order_when_price_valid_then_amends(self):
        # Arrange: Prepare market
        tick = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        order = self.strategy.order_factory.stop_limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            price=Price.from_str("90.000"),
            trigger_price=Price.from_str("90.010"),
        )

        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Trigger order
        tick2 = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.009"),
            ask=Price.from_str("90.010"),
        )
        self.data_engine.process(tick2)
        self.exchange.process_quote_tick(tick2)

        # Act
        self.strategy.modify_order(order, order.quantity, Price.from_str("90.005"))
        self.exchange.process(0)

        # Assert
        assert order.status == OrderStatus.TRIGGERED
        assert order.is_triggered
        assert len(self.exchange.get_open_orders()) == 1
        assert order.price == Price.from_str("90.005")

    def test_order_fills_gets_commissioned(self) -> None:
        # Arrange: Prepare market
        tick = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        order = self.strategy.order_factory.market(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        top_up_order = self.strategy.order_factory.market(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        reduce_order = self.strategy.order_factory.market(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(50_000),
        )

        # Act
        self.strategy.submit_order(order)
        self.exchange.process(0)

        position_id = PositionId("SIM-1-002")  # Generated by exchange

        self.strategy.submit_order(top_up_order)
        self.exchange.process(0)
        self.strategy.submit_order(reduce_order, position_id=position_id)
        self.exchange.process(0)
        fill_event1: OrderFilled = self.strategy.store[2]
        fill_event2: OrderFilled = self.strategy.store[6]
        fill_event3: OrderFilled = self.strategy.store[10]

        # Assert
        assert order.status == OrderStatus.FILLED
        assert fill_event1.commission == Money(180, JPY)
        assert fill_event2.commission == Money(180, JPY)
        assert fill_event3.commission == Money(90, JPY)
        assert Money(999995.00, USD), self.exchange.get_account().balance_total(USD)

    def test_expire_order(self):
        # Arrange: Prepare market
        tick1 = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick1)
        self.exchange.process_quote_tick(tick1)

        order = self.strategy.order_factory.stop_market(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("96.711"),
            time_in_force=TimeInForce.GTD,
            expire_time=UNIX_EPOCH + timedelta(minutes=1),
        )

        self.strategy.submit_order(order)
        self.exchange.process(0)

        tick2 = QuoteTick(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("96.709"),
            ask=Price.from_str("96.710"),
            bid_size=Quantity.from_int(100_000),
            ask_size=Quantity.from_int(100_000),
            ts_event=1 * 60 * 1_000_000_000,  # 1 minute in nanoseconds
            ts_init=1 * 60 * 1_000_000_000,  # 1 minute in nanoseconds
        )

        # Act
        self.exchange.process_quote_tick(tick2)

        # Assert
        assert order.status == OrderStatus.EXPIRED
        assert len(self.exchange.get_open_orders()) == 0

    def test_process_quote_tick_fills_buy_stop_order(self):
        # Arrange: Prepare market
        tick1 = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick1)
        self.exchange.process_quote_tick(tick1)

        order = self.strategy.order_factory.stop_market(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("96.711"),
        )

        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Act
        tick2 = QuoteTick(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("96.710"),
            ask=Price.from_str("96.711"),
            bid_size=Quantity.from_int(100_000),
            ask_size=Quantity.from_int(100_000),
            ts_event=0,
            ts_init=0,
        )

        self.exchange.process_quote_tick(tick2)

        # Assert
        assert len(self.exchange.get_open_orders()) == 0
        assert order.status == OrderStatus.FILLED
        assert order.avg_px == 96.711
        assert self.exchange.get_account().balance_total(USD) == Money(999997.86, USD)

    def test_process_quote_tick_triggers_buy_stop_limit_order(self):
        # Arrange: Prepare market
        tick1 = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick1)
        self.exchange.process_quote_tick(tick1)

        order = self.strategy.order_factory.stop_limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("96.500"),  # LimitPx
            Price.from_str("96.710"),  # StopPx
        )

        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Act
        tick2 = QuoteTick(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("96.710"),
            ask=Price.from_str("96.712"),
            bid_size=Quantity.from_int(100_000),
            ask_size=Quantity.from_int(100_000),
            ts_event=0,
            ts_init=0,
        )

        self.exchange.process_quote_tick(tick2)

        # Assert
        assert order.status == OrderStatus.TRIGGERED
        assert len(self.exchange.get_open_orders()) == 1

    def test_process_quote_tick_rejects_triggered_post_only_buy_stop_limit_order(self):
        # Arrange: Prepare market
        tick1 = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick1)
        self.exchange.process_quote_tick(tick1)

        order = self.strategy.order_factory.stop_limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            price=Price.from_str("90.006"),
            trigger_price=Price.from_str("90.006"),
            post_only=True,
        )

        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Act
        tick2 = QuoteTick(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.005"),
            ask=Price.from_str("90.006"),
            bid_size=Quantity.from_int(100_000),
            ask_size=Quantity.from_int(100_000),
            ts_event=1_000_000_000,
            ts_init=1_000_000_000,
        )

        self.exchange.process_quote_tick(tick2)

        # Assert
        assert order.status == OrderStatus.REJECTED
        assert len(self.exchange.get_open_orders()) == 0

    def test_process_quote_tick_fills_triggered_buy_stop_limit_order(self):
        # Arrange: Prepare market
        tick1 = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick1)
        self.exchange.process_quote_tick(tick1)

        order = self.strategy.order_factory.stop_limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            price=Price.from_str("90.001"),
            trigger_price=Price.from_str("90.006"),
        )

        self.strategy.submit_order(order)
        self.exchange.process(0)

        tick2 = QuoteTick(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.006"),
            ask=Price.from_str("90.007"),
            bid_size=Quantity.from_int(100_000),
            ask_size=Quantity.from_int(100_000),
            ts_event=0,
            ts_init=0,
        )

        # Act
        tick3 = QuoteTick(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.000"),
            ask=Price.from_str("90.001"),
            bid_size=Quantity.from_int(100_000),
            ask_size=Quantity.from_int(100_000),
            ts_event=0,
            ts_init=0,
        )

        self.exchange.process_quote_tick(tick2)
        self.exchange.process_quote_tick(tick3)

        # Assert
        assert order.status == OrderStatus.FILLED
        assert len(self.exchange.get_open_orders()) == 0

    def test_process_quote_tick_fills_buy_limit_order(self):
        # Arrange: Prepare market
        tick1 = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick1)
        self.exchange.process_quote_tick(tick1)

        order = self.strategy.order_factory.limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("90.001"),
        )

        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Act
        tick2 = QuoteTick(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.000"),
            ask=Price.from_str("90.001"),
            bid_size=Quantity.from_int(100_000),
            ask_size=Quantity.from_int(100_000),
            ts_event=0,
            ts_init=0,
        )

        self.exchange.process_quote_tick(tick2)

        # Assert
        assert order.status == OrderStatus.FILLED
        assert len(self.exchange.get_open_orders()) == 0
        assert order.avg_px == 90.001
        assert self.exchange.get_account().balance_total(USD) == Money(999998.00, USD)

    def test_process_quote_tick_fills_sell_stop_order(self):
        # Arrange: Prepare market
        tick = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        order = self.strategy.order_factory.stop_market(
            USDJPY_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
            Price.from_str("90.000"),
        )

        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Act
        tick2 = QuoteTick(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("89.997"),
            ask=Price.from_str("89.999"),
            bid_size=Quantity.from_int(100_000),
            ask_size=Quantity.from_int(100_000),
            ts_event=0,
            ts_init=0,
        )

        self.exchange.process_quote_tick(tick2)

        # Assert
        assert order.status == OrderStatus.FILLED
        assert len(self.exchange.get_open_orders()) == 0
        assert order.avg_px == Price.from_str("90.000")
        assert self.exchange.get_account().balance_total(USD) == Money(999998.00, USD)

    def test_process_quote_tick_fills_sell_limit_order(self):
        # Arrange: Prepare market
        tick = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        order = self.strategy.order_factory.limit(
            USDJPY_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
            Price.from_str("90.100"),
        )

        self.strategy.submit_order(order)
        self.exchange.process(0)

        # Act
        tick2 = QuoteTick(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.101"),
            ask=Price.from_str("90.102"),
            bid_size=Quantity.from_int(100_000),
            ask_size=Quantity.from_int(100_000),
            ts_event=0,
            ts_init=0,
        )

        self.exchange.process_quote_tick(tick2)

        # Assert
        assert order.status == OrderStatus.FILLED
        assert len(self.exchange.get_open_orders()) == 0
        assert order.avg_px == 90.100
        assert self.exchange.get_account().balance_total(USD) == Money(999998.00, USD)

    def test_realized_pnl_contains_commission(self):
        # Arrange: Prepare market
        tick = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        order = self.strategy.order_factory.market(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        # Act
        self.strategy.submit_order(order)
        self.exchange.process(0)
        position = self.cache.positions_open()[0]

        # Assert
        assert position.realized_pnl == Money(-180, JPY)
        assert position.commissions() == [Money(180, JPY)]

    def test_unrealized_pnl(self):
        # Arrange: Prepare market
        tick = TestDataStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        order_open = self.strategy.order_factory.market(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        # Act 1
        self.strategy.submit_order(order_open)
        self.exchange.process(0)

        quote = QuoteTick(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("100.003"),
            ask=Price.from_str("100.004"),
            bid_size=Quantity.from_int(100_000),
            ask_size=Quantity.from_int(100_000),
            ts_event=0,
            ts_init=0,
        )

        self.exchange.process_quote_tick(quote)
        self.portfolio.update_quote_tick(quote)

        order_reduce = self.strategy.order_factory.market(
            USDJPY_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(50_000),
        )

        position_id = PositionId("SIM-1-001")  # Generated by exchange

        # Act 2
        self.strategy.submit_order(order_reduce, position_id)
        self.exchange.process(0)

        # Assert
        position = self.cache.positions_open()[0]
        assert position.unrealized_pnl(Price.from_str("100.003")) == Money(499900, JPY)

    def test_adjust_account_changes_balance(self):
        # Arrange
        value = Money(1000, USD)

        # Act
        self.exchange.adjust_account(value)
        result = self.exchange.exec_client.get_account().balance_total(USD)

        # Assert
        assert result == Money(1001000.00, USD)

    def test_adjust_account_when_account_frozen_does_not_change_balance(self):
        # Arrange
        exchange = SimulatedExchange(
            venue=Venue("SIM"),
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
            frozen_account=True,  # <-- Freezing account
        )
        exchange.register_client(self.exec_client)
        exchange.reset()

        value = Money(1000, USD)

        # Act
        exchange.adjust_account(value)
        result = exchange.get_account().balance_total(USD)

        # Assert
        assert result == Money(1000000.00, USD)

    def test_position_flipped_when_reduce_order_exceeds_original_quantity(self):
        # Arrange: Prepare market
        open_quote = QuoteTick(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.003"),
            bid_size=Quantity.from_int(1_000_000),
            ask_size=Quantity.from_int(1_000_000),
            ts_event=0,
            ts_init=0,
        )

        self.data_engine.process(open_quote)
        self.exchange.process_quote_tick(open_quote)

        order_open = self.strategy.order_factory.market(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        # Act 1
        self.strategy.submit_order(order_open)
        self.exchange.process(0)

        reduce_quote = QuoteTick(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("100.003"),
            ask=Price.from_str("100.004"),
            bid_size=Quantity.from_int(1_000_000),
            ask_size=Quantity.from_int(1_000_000),
            ts_event=0,
            ts_init=0,
        )

        self.exchange.process_quote_tick(reduce_quote)
        self.portfolio.update_quote_tick(reduce_quote)

        order_reduce = self.strategy.order_factory.market(
            USDJPY_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(150_000),
        )

        # Act 2
        self.strategy.submit_order(order_reduce, PositionId("SIM-1-001"))  # Generated by exchange
        self.exchange.process(0)

        # Assert
        # TODO(cs): Current behaviour erases previous position from cache
        position_open = self.cache.positions_open()[0]
        position_closed = self.cache.positions_closed()[0]
        assert position_open.side == PositionSide.SHORT
        assert position_open.quantity == Quantity.from_int(50_000)
        assert position_closed.realized_pnl == Money(-100, JPY)
        assert position_closed.commissions() == [Money(100, JPY)]
        assert self.exchange.get_account().balance_total(USD) == Money(1011105.53, USD)

    def test_reduce_only_market_order_does_not_open_position_on_flip_scenario(self):
        # Arrange: Prepare market
        tick = QuoteTick(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("14.0"),
            ask=Price.from_str("13.0"),
            bid_size=Quantity.from_int(1_000_000),
            ask_size=Quantity.from_int(1_000_000),
            ts_event=0,
            ts_init=0,
        )
        self.exchange.process_quote_tick(tick)

        entry = self.strategy.order_factory.market(
            instrument_id=USDJPY_SIM.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(200_000),
        )
        self.strategy.submit_order(entry)
        self.exchange.process(0)

        exit = self.strategy.order_factory.market(
            instrument_id=USDJPY_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(300_000),  # <-- overfill to attempt flip
            reduce_only=True,
        )
        self.strategy.submit_order(exit, position_id=PositionId("SIM-1-001"))
        self.exchange.process(0)

        # Assert
        assert exit.status == OrderStatus.DENIED

    def test_reduce_only_limit_order_does_not_open_position_on_flip_scenario(self):
        # Arrange: Prepare market
        tick = QuoteTick(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("14.0"),
            ask=Price.from_str("13.0"),
            bid_size=Quantity.from_int(1_000_000),
            ask_size=Quantity.from_int(1_000_000),
            ts_event=0,
            ts_init=0,
        )
        self.exchange.process_quote_tick(tick)

        entry = self.strategy.order_factory.market(
            instrument_id=USDJPY_SIM.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(200_000),
        )
        self.strategy.submit_order(entry)
        self.exchange.process(0)

        exit = self.strategy.order_factory.limit(
            instrument_id=USDJPY_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(300_000),  # <-- overfill to attempt flip
            price=Price.from_str("11"),
            post_only=False,
            reduce_only=True,
        )
        self.strategy.submit_order(exit, position_id=PositionId("SIM-1-001"))
        self.exchange.process(0)

        tick = QuoteTick(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("10.0"),
            ask=Price.from_str("11.0"),
            bid_size=Quantity.from_int(1_000_000),
            ask_size=Quantity.from_int(1_000_000),
            ts_event=0,
            ts_init=0,
        )
        self.exchange.process_quote_tick(tick)

        # Assert
        assert exit.status == OrderStatus.DENIED

    def test_latency_model_submit_order(self):
        # Arrange
        self.exchange.set_latency_model(LatencyModel(secs_to_nanos(1)))
        entry = self.strategy.order_factory.limit(
            instrument_id=USDJPY_SIM.id,
            order_side=OrderSide.BUY,
            price=Price.from_int(100),
            quantity=Quantity.from_int(200_000),
        )

        # Act
        self.strategy.submit_order(entry)
        # Order still in submitted state
        self.exchange.process(0)
        self.exchange.process(secs_to_nanos(1))

        # Assert
        assert entry.status == OrderStatus.ACCEPTED

    def test_latency_model_cancel_order(self):
        # Arrange
        self.exchange.set_latency_model(LatencyModel(secs_to_nanos(1)))
        entry = self.strategy.order_factory.limit(
            instrument_id=USDJPY_SIM.id,
            order_side=OrderSide.BUY,
            price=Price.from_int(100),
            quantity=Quantity.from_int(200_000),
        )

        # Act
        self.strategy.submit_order(entry)
        self.exchange.process(secs_to_nanos(1))
        self.strategy.cancel_order(entry)
        self.strategy.cancel_order(entry)  # <-- handles multiple commands
        self.exchange.process(secs_to_nanos(2))

        # Assert
        assert entry.status == OrderStatus.CANCELED

    def test_latency_model_modify_order(self):
        # Arrange
        self.exchange.set_latency_model(LatencyModel(secs_to_nanos(1)))
        entry = self.strategy.order_factory.limit(
            instrument_id=USDJPY_SIM.id,
            order_side=OrderSide.BUY,
            price=Price.from_int(100),
            quantity=Quantity.from_int(200_000),
        )

        # Act
        self.strategy.submit_order(entry)
        self.exchange.process(secs_to_nanos(1))
        self.strategy.modify_order(entry, quantity=Quantity.from_int(100_000))
        self.exchange.process(secs_to_nanos(2))

        # Assert
        assert entry.status == OrderStatus.ACCEPTED
        assert entry.quantity == 100000

    def test_latency_model_large_int(self):
        # Arrange
        self.exchange.set_latency_model(LatencyModel(secs_to_nanos(10)))
        entry = self.strategy.order_factory.limit(
            instrument_id=USDJPY_SIM.id,
            order_side=OrderSide.BUY,
            price=Price.from_int(100),
            quantity=Quantity.from_int(200_000),
        )

        # Act
        self.strategy.submit_order(entry)
        self.exchange.process(secs_to_nanos(10))

        # Assert
        assert entry.status == OrderStatus.ACCEPTED
        assert entry.quantity == 200000
