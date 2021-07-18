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

from datetime import timedelta
from decimal import Decimal

import pytest

from nautilus_trader.backtest.exchange import SimulatedExchange
from nautilus_trader.backtest.execution import BacktestExecClient
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.model.commands.trading import CancelOrder
from nautilus_trader.model.commands.trading import UpdateOrder
from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.currencies import JPY
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import BookLevel
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OMSType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderState
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import VenueType
from nautilus_trader.model.events.order import OrderAccepted
from nautilus_trader.model.events.order import OrderRejected
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orderbook.book import OrderBook
from nautilus_trader.msgbus.message_bus import MessageBus
from nautilus_trader.risk.engine import RiskEngine
from nautilus_trader.trading.portfolio import Portfolio
from tests.test_kit.mocks import MockStrategy
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs
from tests.test_kit.stubs import UNIX_EPOCH


SIM = Venue("SIM")
AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
USDJPY_SIM = TestInstrumentProvider.default_fx_ccy("USD/JPY")
XBTUSD_BITMEX = TestInstrumentProvider.xbtusd_bitmex()


class TestSimulatedExchange:
    def setup(self):
        # Fixture Setup
        self.clock = TestClock()
        self.uuid_factory = UUIDFactory()
        self.logger = Logger(self.clock)

        self.msgbus = MessageBus(
            clock=self.clock,
            logger=self.logger,
        )

        self.cache = TestStubs.cache()

        self.portfolio = Portfolio(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.data_engine = DataEngine(
            portfolio=self.portfolio,
            clock=self.clock,
            cache=self.cache,
            logger=self.logger,
            config={"use_previous_close": False},  # To correctly reproduce historical data bars
        )

        self.trader_id = TestStubs.trader_id()
        self.account_id = AccountId("SIM", "001")

        self.exec_engine = ExecutionEngine(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.risk_engine = RiskEngine(
            exec_engine=self.exec_engine,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.exchange = SimulatedExchange(
            venue=SIM,
            venue_type=VenueType.ECN,
            oms_type=OMSType.HEDGING,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            starting_balances=[Money(1_000_000, USD)],
            is_frozen_account=False,
            instruments=[AUDUSD_SIM, USDJPY_SIM],
            modules=[],
            fill_model=FillModel(),
            cache=self.exec_engine.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.exec_client = BacktestExecClient(
            exchange=self.exchange,
            account_id=self.account_id,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            engine=self.exec_engine,
            clock=self.clock,
            logger=self.logger,
        )

        # Wire up components
        self.data_engine.cache.add_instrument(AUDUSD_SIM)
        self.data_engine.cache.add_instrument(USDJPY_SIM)

        self.exec_engine.register_client(self.exec_client)
        self.exchange.register_client(self.exec_client)

        self.exec_engine.cache.add_instrument(AUDUSD_SIM)
        self.exec_engine.cache.add_instrument(USDJPY_SIM)
        self.exec_engine.cache.add_instrument(XBTUSD_BITMEX)

        # Create mock strategy
        self.strategy = MockStrategy(bar_type=TestStubs.bartype_usdjpy_1min_bid())
        self.strategy.register(
            self.trader_id,
            self.msgbus,
            self.portfolio,
            self.data_engine,
            self.risk_engine,
            self.clock,
            self.logger,
        )

        # Start components
        self.exchange.reset()
        self.data_engine.start()
        self.exec_engine.start()
        self.strategy.start()

    def test_repr(self):
        # Arrange
        # Act
        # Assert
        assert repr(self.exchange) == "SimulatedExchange(SIM)"

    def test_check_residuals(self):
        # Arrange
        # Act
        self.exchange.check_residuals()
        # Assert
        assert True  # No exceptions raised

    def test_process_quote_tick_sets_market(self):
        # Arrange
        tick = TestStubs.quote_tick_3decimal(USDJPY_SIM.id)

        self.data_engine.process(tick)

        # Act
        self.exchange.process_tick(tick)

        # Assert
        book = self.exchange.get_book(USDJPY_SIM.id)
        assert book.best_ask_price() == 90.005
        assert book.best_bid_price() == 90.002

    def test_check_residuals_with_working_and_oco_orders(self):
        # Arrange
        # Prepare market
        tick = TestStubs.quote_tick_3decimal(USDJPY_SIM.id)
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        entry1 = self.strategy.order_factory.limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("90.000"),
        )

        entry2 = self.strategy.order_factory.limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("89.900"),
        )

        bracket1 = self.strategy.order_factory.bracket(
            entry_order=entry1,
            stop_loss=Price.from_str("89.900"),
            take_profit=Price.from_str("91.000"),
        )

        bracket2 = self.strategy.order_factory.bracket(
            entry_order=entry2,
            stop_loss=Price.from_str("89.800"),
            take_profit=Price.from_str("91.000"),
        )

        self.strategy.submit_bracket_order(bracket1)
        self.strategy.submit_bracket_order(bracket2)

        tick2 = QuoteTick(
            USDJPY_SIM.id,
            Price.from_str("89.998"),
            Price.from_str("89.999"),
            Quantity.from_int(100000),
            Quantity.from_int(100000),
            0,
            0,
        )

        self.exchange.process_tick(tick2)

        # Act
        self.exchange.check_residuals()

        # Assert
        # TODO: Revisit testing
        assert len(self.exchange.get_working_orders()) == 3
        assert bracket1.stop_loss in self.exchange.get_working_orders().values()
        assert bracket1.take_profit in self.exchange.get_working_orders().values()
        assert entry2 in self.exchange.get_working_orders().values()

    def test_get_working_orders_when_no_orders_returns_empty_dict(self):
        # Arrange
        # Act
        orders = self.exchange.get_working_orders()

        assert orders == {}

    def test_submit_buy_limit_order_with_no_market_accepts_order(self):
        # Arrange
        order = self.strategy.order_factory.limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("110.000"),
        )

        # Act
        self.strategy.submit_order(order)

        # Assert
        assert order.state == OrderState.ACCEPTED
        assert self.strategy.object_storer.count == 3
        assert isinstance(self.strategy.object_storer.get_store()[2], OrderAccepted)

    def test_submit_sell_limit_order_with_no_market_accepts_order(self):
        # Arrange
        order = self.strategy.order_factory.limit(
            USDJPY_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100000),
            Price.from_str("110.000"),
        )

        # Act
        self.strategy.submit_order(order)

        # Assert
        assert order.state == OrderState.ACCEPTED
        assert self.strategy.object_storer.count == 3
        assert isinstance(self.strategy.object_storer.get_store()[2], OrderAccepted)

    def test_submit_buy_market_order_with_no_market_rejects_order(self):
        # Arrange
        order = self.strategy.order_factory.market(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        # Act
        self.strategy.submit_order(order)

        # Assert
        assert order.state == OrderState.REJECTED
        assert self.strategy.object_storer.count == 3
        assert isinstance(self.strategy.object_storer.get_store()[2], OrderRejected)

    def test_submit_sell_market_order_with_no_market_rejects_order(self):
        # Arrange
        order = self.strategy.order_factory.market(
            USDJPY_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100000),
        )

        # Act
        self.strategy.submit_order(order)

        # Assert
        assert order.state == OrderState.REJECTED
        assert self.strategy.object_storer.count == 3
        assert isinstance(self.strategy.object_storer.get_store()[2], OrderRejected)

    def test_submit_order_with_invalid_price_gets_rejected(self):
        # Arrange: Prepare market
        tick = TestStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.exchange.process_tick(tick)
        self.portfolio.update_tick(tick)

        order = self.strategy.order_factory.stop_market(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("90.005"),  # Price at ask
        )

        # Act
        self.strategy.submit_order(order)

        # Assert
        assert order.state == OrderState.REJECTED

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
        assert order.state == OrderState.DENIED

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
        assert order.state == OrderState.DENIED

    def test_submit_market_order(self):
        # Arrange: Prepare market
        tick = TestStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        # Create order
        order = self.strategy.order_factory.market(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        # Act
        self.strategy.submit_order(order)

        # Assert
        assert order.state == OrderState.FILLED
        assert order.avg_px == Decimal("90.005")  # No slippage

    def test_submit_post_only_limit_order_when_marketable_then_rejects(self):
        # Arrange: Prepare market
        tick = TestStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        order = self.strategy.order_factory.limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("90.005"),
            post_only=True,
        )

        # Act
        self.strategy.submit_order(order)

        # Assert
        assert order.state == OrderState.REJECTED
        assert len(self.exchange.get_working_orders()) == 0

    def test_submit_limit_order(self):
        # Arrange: Prepare market
        tick = TestStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        order = self.strategy.order_factory.limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("90.001"),
        )

        # Act
        self.strategy.submit_order(order)

        # Assert
        assert order.state == OrderState.ACCEPTED
        assert len(self.exchange.get_working_orders()) == 1
        assert order.client_order_id in self.exchange.get_working_orders()

    def test_submit_limit_order_when_marketable_then_fills(self):
        # Arrange: Prepare market
        tick = TestStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        order = self.strategy.order_factory.limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("90.005"),  # <-- Limit price at the ask
            post_only=False,  # <-- Can be liquidity TAKER
        )

        # Act
        self.strategy.submit_order(order)

        # Assert
        assert order.state == OrderState.FILLED
        assert order.liquidity_side == LiquiditySide.TAKER
        assert len(self.exchange.get_working_orders()) == 0

    def test_submit_limit_order_fills_at_correct_price(self):
        # Arrange: Prepare market
        tick = TestStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        order = self.strategy.order_factory.limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("90.010"),  # <-- Limit price above the ask
            post_only=False,  # <-- Can be liquidity TAKER
        )

        # Act
        self.strategy.submit_order(order)

        # Assert
        assert order.state == OrderState.FILLED
        assert order.avg_px == tick.ask

    def test_submit_limit_order_fills_at_most_book_volume(self):
        # Arrange: Prepare market
        tick = TestStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        order = self.strategy.order_factory.limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(2_000_000),  # <-- Order volume greater than available ask volume
            Price.from_str("90.010"),
            post_only=False,  # <-- Can be liquidity TAKER
        )

        # Act
        self.strategy.submit_order(order)

        # Assert
        assert order.state == OrderState.PARTIALLY_FILLED
        assert order.filled_qty == 1_000_000

    def test_submit_stop_market_order_inside_market_rejects(self):
        # Arrange: Prepare market
        tick = TestStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        order = self.strategy.order_factory.stop_market(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("90.005"),
        )

        # Act
        self.strategy.submit_order(order)

        # Assert
        assert order.state == OrderState.REJECTED
        assert len(self.exchange.get_working_orders()) == 0

    def test_submit_stop_market_order(self):
        # Arrange: Prepare market
        tick = TestStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        order = self.strategy.order_factory.stop_market(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("90.010"),
        )

        # Act
        self.strategy.submit_order(order)

        # Assert
        assert order.state == OrderState.ACCEPTED
        assert len(self.exchange.get_working_orders()) == 1
        assert order.client_order_id in self.exchange.get_working_orders()

    def test_submit_stop_limit_order_when_inside_market_rejects(self):
        # Arrange: Prepare market
        tick = TestStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        order = self.strategy.order_factory.stop_limit(
            USDJPY_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100000),
            price=Price.from_str("90.010"),
            trigger=Price.from_str("90.02"),
        )

        # Act
        self.strategy.submit_order(order)

        # Assert
        assert order.state == OrderState.REJECTED
        assert len(self.exchange.get_working_orders()) == 0

    def test_submit_stop_limit_order(self):
        # Arrange: Prepare market
        tick = TestStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        order = self.strategy.order_factory.stop_limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            price=Price.from_str("90.000"),
            trigger=Price.from_str("90.010"),
        )

        # Act
        self.strategy.submit_order(order)

        # Assert
        assert order.state == OrderState.ACCEPTED
        assert len(self.exchange.get_working_orders()) == 1
        assert order.client_order_id in self.exchange.get_working_orders()

    def test_submit_bracket_market_order(self):
        # Arrange: Prepare market
        tick = TestStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        entry_order = self.strategy.order_factory.market(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        bracket_order = self.strategy.order_factory.bracket(
            entry_order=entry_order,
            stop_loss=Price.from_str("89.950"),
            take_profit=Price.from_str("90.050"),
        )

        # Act
        self.strategy.submit_bracket_order(bracket_order)

        # Assert
        stop_loss_order = self.exec_engine.cache.order(ClientOrderId("O-19700101-000000-000-001-2"))
        take_profit_order = self.exec_engine.cache.order(
            ClientOrderId("O-19700101-000000-000-001-3")
        )

        assert entry_order.state == OrderState.FILLED
        assert stop_loss_order.state == OrderState.ACCEPTED
        assert take_profit_order.state == OrderState.ACCEPTED

    def test_submit_stop_market_order_with_bracket(self):
        # Arrange: Prepare market
        tick = TestStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        entry_order = self.strategy.order_factory.stop_market(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("90.020"),
        )

        bracket_order = self.strategy.order_factory.bracket(
            entry_order=entry_order,
            stop_loss=Price.from_str("90.000"),
            take_profit=Price.from_str("90.040"),
        )

        # Act
        self.strategy.submit_bracket_order(bracket_order)

        # Assert
        stop_loss_order = self.exec_engine.cache.order(ClientOrderId("O-19700101-000000-000-001-2"))
        take_profit_order = self.exec_engine.cache.order(
            ClientOrderId("O-19700101-000000-000-001-3")
        )

        assert entry_order.state == OrderState.ACCEPTED
        assert stop_loss_order.state == OrderState.SUBMITTED
        assert take_profit_order.state == OrderState.SUBMITTED
        assert len(self.exchange.get_working_orders()) == 1
        assert entry_order.client_order_id in self.exchange.get_working_orders()

    def test_cancel_stop_order(self):
        # Arrange: Prepare market
        tick = TestStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        order = self.strategy.order_factory.stop_market(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("90.010"),
        )

        self.strategy.submit_order(order)

        # Act
        self.strategy.cancel_order(order)

        # Assert
        assert order.state == OrderState.CANCELED
        assert len(self.exchange.get_working_orders()) == 0

    def test_cancel_stop_order_when_order_does_not_exist_generates_cancel_reject(self):
        # Arrange
        command = CancelOrder(
            trader_id=self.trader_id,
            strategy_id=StrategyId("SCALPER-001"),
            instrument_id=USDJPY_SIM.id,
            client_order_id=ClientOrderId("O-123456"),
            venue_order_id=VenueOrderId("001"),
            command_id=self.uuid_factory.generate(),
            timestamp_ns=0,
        )

        # Act
        self.exchange.handle_cancel_order(command)

        # Assert
        assert self.exec_engine.event_count == 2

    def test_update_stop_order_when_order_does_not_exist(self):
        # Arrange
        command = UpdateOrder(
            trader_id=self.trader_id,
            strategy_id=StrategyId("SCALPER-001"),
            instrument_id=USDJPY_SIM.id,
            client_order_id=ClientOrderId("O-123456"),
            venue_order_id=VenueOrderId("001"),
            quantity=Quantity.from_int(100000),
            price=Price.from_str("110.000"),
            trigger=None,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=0,
        )

        # Act
        self.exchange.handle_update_order(command)

        # Assert
        assert self.exec_engine.event_count == 2

    def test_update_order_with_zero_quantity_rejects_amendment(self):
        # Arrange: Prepare market
        tick = TestStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        order = self.strategy.order_factory.limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("90.001"),
            post_only=True,  # Default value
        )

        self.strategy.submit_order(order)

        # Act: Amending BUY LIMIT order limit price to ask will become marketable
        self.strategy.update_order(order, Quantity.zero(), Price.from_str("90.001"))

        # Assert
        assert order.state == OrderState.ACCEPTED
        assert len(self.exchange.get_working_orders()) == 1  # Order still working
        assert order.price == Price.from_str("90.001")  # Did not update

    def test_update_post_only_limit_order_when_marketable_then_rejects_amendment(self):
        # Arrange: Prepare market
        tick = TestStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        order = self.strategy.order_factory.limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("90.001"),
            post_only=True,  # Default value
        )

        self.strategy.submit_order(order)

        # Act: Amending BUY LIMIT order limit price to ask will become marketable
        self.strategy.update_order(order, order.quantity, Price.from_str("90.005"))

        # Assert
        assert order.state == OrderState.ACCEPTED
        assert len(self.exchange.get_working_orders()) == 1  # Order still working
        assert order.price == Price.from_str("90.001")  # Did not update

    def test_update_limit_order_when_marketable_then_fills_order(self):
        # Arrange: Prepare market
        tick = TestStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        order = self.strategy.order_factory.limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("90.001"),
            post_only=False,  # Ensures marketable on amendment
        )

        self.strategy.submit_order(order)

        # Act: Amending BUY LIMIT order limit price to ask will become marketable
        self.strategy.update_order(order, order.quantity, Price.from_str("90.005"))

        # Assert
        assert order.state == OrderState.FILLED
        assert len(self.exchange.get_working_orders()) == 0
        assert order.avg_px == Price.from_str("90.005")

    def test_update_stop_market_order_when_price_inside_market_then_rejects_amendment(
        self,
    ):
        # Arrange: Prepare market
        tick = TestStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        order = self.strategy.order_factory.stop_market(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("90.010"),
        )

        self.strategy.submit_order(order)

        # Act
        self.strategy.update_order(order, order.quantity, Price.from_str("90.005"))

        # Assert
        assert order.state == OrderState.ACCEPTED
        assert len(self.exchange.get_working_orders()) == 1
        assert order.price == Price.from_str("90.010")

    def test_update_stop_market_order_when_price_valid_then_amends(self):
        # Arrange: Prepare market
        tick = TestStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        order = self.strategy.order_factory.stop_market(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("90.010"),
        )

        self.strategy.submit_order(order)

        # Act
        self.strategy.update_order(order, order.quantity, Price.from_str("90.011"))

        # Assert
        assert order.state == OrderState.ACCEPTED
        assert len(self.exchange.get_working_orders()) == 1
        assert order.price == Price.from_str("90.011")

    def test_update_untriggered_stop_limit_order_when_price_inside_market_then_rejects_amendment(
        self,
    ):
        # Arrange: Prepare market
        tick = TestStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        order = self.strategy.order_factory.stop_limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            price=Price.from_str("90.000"),
            trigger=Price.from_str("90.010"),
        )

        self.strategy.submit_order(order)

        # Act
        self.strategy.update_order(order, order.quantity, Price.from_str("90.005"))

        # Assert
        assert order.state == OrderState.ACCEPTED
        assert len(self.exchange.get_working_orders()) == 1
        assert order.trigger == Price.from_str("90.010")

    def test_update_untriggered_stop_limit_order_when_price_valid_then_amends(self):
        # Arrange: Prepare market
        tick = TestStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        order = self.strategy.order_factory.stop_limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            price=Price.from_str("90.000"),
            trigger=Price.from_str("90.010"),
        )

        self.strategy.submit_order(order)

        # Act
        self.strategy.update_order(order, order.quantity, Price.from_str("90.011"))

        # Assert
        assert order.state == OrderState.ACCEPTED
        assert len(self.exchange.get_working_orders()) == 1
        assert order.trigger == Price.from_str("90.011")

    def test_update_triggered_post_only_stop_limit_order_when_price_inside_market_then_rejects_amendment(
        self,
    ):
        # Arrange: Prepare market
        tick1 = TestStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick1)
        self.exchange.process_tick(tick1)

        order = self.strategy.order_factory.stop_limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            price=Price.from_str("90.000"),
            trigger=Price.from_str("90.010"),
            post_only=True,
        )

        self.strategy.submit_order(order)

        # Trigger order
        tick2 = TestStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.009"),
            ask=Price.from_str("90.010"),
        )
        self.data_engine.process(tick2)
        self.exchange.process_tick(tick2)

        # Act
        self.strategy.update_order(order, order.quantity, Price.from_str("90.010"))

        # Assert
        assert order.state == OrderState.TRIGGERED
        assert order.is_triggered
        assert len(self.exchange.get_working_orders()) == 1
        assert order.price == Price.from_str("90.000")

    def test_update_triggered_stop_limit_order_when_price_inside_market_then_fills(
        self,
    ):
        # Arrange: Prepare market
        tick1 = TestStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick1)
        self.exchange.process_tick(tick1)

        order = self.strategy.order_factory.stop_limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            price=Price.from_str("90.000"),
            trigger=Price.from_str("90.010"),
            post_only=False,
        )

        self.strategy.submit_order(order)

        # Trigger order
        tick2 = TestStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.009"),
            ask=Price.from_str("90.010"),
        )
        self.data_engine.process(tick2)
        self.exchange.process_tick(tick2)

        # Act
        self.strategy.update_order(order, order.quantity, Price.from_str("90.010"))

        # Assert
        assert order.state == OrderState.FILLED
        assert order.is_triggered
        assert len(self.exchange.get_working_orders()) == 0
        assert order.price == Price.from_str("90.010")

    def test_update_triggered_stop_limit_order_when_price_valid_then_amends(self):
        # Arrange: Prepare market
        tick = TestStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        order = self.strategy.order_factory.stop_limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            price=Price.from_str("90.000"),
            trigger=Price.from_str("90.010"),
        )

        self.strategy.submit_order(order)

        # Trigger order
        tick2 = TestStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.009"),
            ask=Price.from_str("90.010"),
        )
        self.data_engine.process(tick2)
        self.exchange.process_tick(tick2)

        # Act
        self.strategy.update_order(order, order.quantity, Price.from_str("90.005"))

        # Assert
        assert order.state == OrderState.TRIGGERED
        assert order.is_triggered
        assert len(self.exchange.get_working_orders()) == 1
        assert order.price == Price.from_str("90.005")

    def test_update_bracket_orders_working_stop_loss(self):
        # Arrange: Prepare market
        tick = TestStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        entry_order = self.strategy.order_factory.market(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        bracket_order = self.strategy.order_factory.bracket(
            entry_order,
            stop_loss=Price.from_str("85.000"),
            take_profit=Price.from_str("91.000"),
        )

        self.strategy.submit_bracket_order(bracket_order)

        # Act
        self.strategy.update_order(
            bracket_order.stop_loss,
            bracket_order.entry.quantity,
            Price.from_str("85.100"),
        )

        # Assert
        assert bracket_order.stop_loss.state == OrderState.ACCEPTED
        assert bracket_order.stop_loss.price == Price.from_str("85.100")

    def test_order_fills_gets_commissioned(self):
        # Arrange: Prepare market
        tick = TestStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        order = self.strategy.order_factory.market(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        top_up_order = self.strategy.order_factory.market(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        reduce_order = self.strategy.order_factory.market(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(50000),
        )

        # Act
        self.strategy.submit_order(order)

        position_id = PositionId("2-001")  # Generated by platform

        self.strategy.submit_order(top_up_order, position_id)
        self.strategy.submit_order(reduce_order, position_id)
        fill_event1 = self.strategy.object_storer.get_store()[2]
        fill_event2 = self.strategy.object_storer.get_store()[6]
        fill_event3 = self.strategy.object_storer.get_store()[10]

        # Assert
        assert order.state == OrderState.FILLED
        assert fill_event1.commission == Money(180.01, JPY)
        assert fill_event2.commission == Money(180.01, JPY)
        assert fill_event3.commission == Money(90.00, JPY)
        assert Money(999995.00, USD), self.exchange.get_account().balance_total(USD)

    def test_expire_order(self):
        # Arrange: Prepare market
        tick1 = TestStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick1)
        self.exchange.process_tick(tick1)

        order = self.strategy.order_factory.stop_market(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("96.711"),
            time_in_force=TimeInForce.GTD,
            expire_time=UNIX_EPOCH + timedelta(minutes=1),
        )

        self.strategy.submit_order(order)

        tick2 = QuoteTick(
            USDJPY_SIM.id,
            Price.from_str("96.709"),
            Price.from_str("96.710"),
            Quantity.from_int(100000),
            Quantity.from_int(100000),
            1 * 60 * 1_000_000_000,  # 1 minute in nanoseconds
            1 * 60 * 1_000_000_000,  # 1 minute in nanoseconds
        )

        # Act
        self.exchange.process_tick(tick2)

        # Assert
        assert order.state == OrderState.EXPIRED
        assert len(self.exchange.get_working_orders()) == 0

    def test_process_quote_tick_fills_buy_stop_order(self):
        # Arrange: Prepare market
        tick = TestStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        order = self.strategy.order_factory.stop_market(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("96.711"),
        )

        self.strategy.submit_order(order)

        # Act
        tick2 = QuoteTick(
            AUDUSD_SIM.id,  # Different market
            Price.from_str("80.010"),
            Price.from_str("80.011"),
            Quantity.from_int(200000),
            Quantity.from_int(200000),
            0,
            0,
        )

        tick3 = QuoteTick(
            USDJPY_SIM.id,
            Price.from_str("96.710"),
            Price.from_str("96.711"),
            Quantity.from_int(100000),
            Quantity.from_int(100000),
            0,
            0,
        )

        self.exchange.process_tick(tick2)
        self.exchange.process_tick(tick3)

        # Assert
        assert len(self.exchange.get_working_orders()) == 0
        assert order.state == OrderState.FILLED
        assert order.avg_px == Price.from_str("96.711")
        assert self.exchange.get_account().balance_total(USD) == Money(999997.86, USD)

    def test_process_quote_tick_triggers_buy_stop_limit_order(self):
        # Arrange: Prepare market
        tick1 = TestStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick1)
        self.exchange.process_tick(tick1)

        order = self.strategy.order_factory.stop_limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("96.500"),  # LimitPx
            Price.from_str("96.710"),  # StopPx
        )

        self.strategy.submit_order(order)

        # Act
        tick2 = QuoteTick(
            USDJPY_SIM.id,
            Price.from_str("96.710"),
            Price.from_str("96.712"),
            Quantity.from_int(100000),
            Quantity.from_int(100000),
            0,
            0,
        )

        self.exchange.process_tick(tick2)

        # Assert
        assert order.state == OrderState.TRIGGERED
        assert len(self.exchange.get_working_orders()) == 1

    def test_process_quote_tick_rejects_triggered_post_only_buy_stop_limit_order(self):
        # Arrange: Prepare market
        tick1 = TestStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick1)
        self.exchange.process_tick(tick1)

        order = self.strategy.order_factory.stop_limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            price=Price.from_str("90.006"),
            trigger=Price.from_str("90.006"),
            post_only=True,
        )

        self.strategy.submit_order(order)

        # Act
        tick2 = QuoteTick(
            USDJPY_SIM.id,
            Price.from_str("90.005"),
            Price.from_str("90.006"),
            Quantity.from_int(100000),
            Quantity.from_int(100000),
            1_000_000_000,
            1_000_000_000,
        )

        self.exchange.process_tick(tick2)

        # Assert
        assert order.state == OrderState.REJECTED
        assert len(self.exchange.get_working_orders()) == 0

    def test_process_quote_tick_fills_triggered_buy_stop_limit_order(self):
        # Arrange: Prepare market
        tick1 = TestStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick1)
        self.exchange.process_tick(tick1)

        order = self.strategy.order_factory.stop_limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            price=Price.from_str("90.001"),
            trigger=Price.from_str("90.006"),
        )

        self.strategy.submit_order(order)

        tick2 = QuoteTick(
            USDJPY_SIM.id,
            Price.from_str("90.006"),
            Price.from_str("90.007"),
            Quantity.from_int(100000),
            Quantity.from_int(100000),
            0,
            0,
        )

        # Act
        tick3 = QuoteTick(
            USDJPY_SIM.id,
            Price.from_str("90.000"),
            Price.from_str("90.001"),
            Quantity.from_int(100000),
            Quantity.from_int(100000),
            0,
            0,
        )

        self.exchange.process_tick(tick2)
        self.exchange.process_tick(tick3)

        # Assert
        assert order.state == OrderState.FILLED
        assert len(self.exchange.get_working_orders()) == 0

    def test_process_quote_tick_fills_buy_limit_order(self):
        # Arrange: Prepare market
        tick1 = TestStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick1)
        self.exchange.process_tick(tick1)

        order = self.strategy.order_factory.limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("90.001"),
        )

        self.strategy.submit_order(order)

        # Act
        tick2 = QuoteTick(
            AUDUSD_SIM.id,  # Different market
            Price.from_str("80.010"),
            Price.from_str("80.011"),
            Quantity.from_int(200000),
            Quantity.from_int(200000),
            0,
            0,
        )

        tick3 = QuoteTick(
            USDJPY_SIM.id,
            Price.from_str("90.000"),
            Price.from_str("90.001"),
            Quantity.from_int(100000),
            Quantity.from_int(100000),
            0,
            0,
        )

        self.exchange.process_tick(tick2)
        self.exchange.process_tick(tick3)

        # Assert
        assert order.state == OrderState.FILLED
        assert len(self.exchange.get_working_orders()) == 0
        assert order.avg_px == Price.from_str("90.001")
        assert self.exchange.get_account().balance_total(USD) == Money(999998.00, USD)

    def test_process_quote_tick_fills_sell_stop_order(self):
        # Arrange: Prepare market
        tick = TestStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        order = self.strategy.order_factory.stop_market(
            USDJPY_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100000),
            Price.from_str("90.000"),
        )

        self.strategy.submit_order(order)

        # Act
        tick2 = QuoteTick(
            USDJPY_SIM.id,
            Price.from_str("89.997"),
            Price.from_str("89.999"),
            Quantity.from_int(100000),
            Quantity.from_int(100000),
            0,
            0,
        )

        self.exchange.process_tick(tick2)

        # Assert
        assert order.state == OrderState.FILLED
        assert len(self.exchange.get_working_orders()) == 0
        assert order.avg_px == Price.from_str("90.000")
        assert self.exchange.get_account().balance_total(USD) == Money(999998.00, USD)

    def test_process_quote_tick_fills_sell_limit_order(self):
        # Arrange: Prepare market
        tick = TestStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        order = self.strategy.order_factory.limit(
            USDJPY_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100000),
            Price.from_str("90.100"),
        )

        self.strategy.submit_order(order)

        # Act
        tick2 = QuoteTick(
            USDJPY_SIM.id,
            Price.from_str("90.101"),
            Price.from_str("90.102"),
            Quantity.from_int(100000),
            Quantity.from_int(100000),
            0,
            0,
        )

        self.exchange.process_tick(tick2)

        # Assert
        assert order.state == OrderState.FILLED
        assert len(self.exchange.get_working_orders()) == 0
        assert order.avg_px == Price.from_str("90.101")
        assert self.exchange.get_account().balance_total(USD) == Money(999998.00, USD)

    def test_process_quote_tick_fills_buy_limit_entry_with_bracket(self):
        # Arrange: Prepare market
        tick = TestStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        entry = self.strategy.order_factory.limit(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("90.000"),
        )

        bracket = self.strategy.order_factory.bracket(
            entry_order=entry,
            stop_loss=Price.from_str("89.900"),
            take_profit=Price.from_str("91.000"),
        )

        self.strategy.submit_bracket_order(bracket)

        # Act
        tick2 = QuoteTick(
            USDJPY_SIM.id,
            Price.from_str("89.998"),
            Price.from_str("89.999"),
            Quantity.from_int(100000),
            Quantity.from_int(100000),
            0,
            0,
        )

        self.exchange.process_tick(tick2)

        # Assert
        assert entry.state == OrderState.FILLED
        assert bracket.stop_loss.state == OrderState.ACCEPTED
        assert bracket.take_profit.state == OrderState.ACCEPTED
        assert len(self.exchange.get_working_orders()) == 2
        assert bracket.stop_loss in self.exchange.get_working_orders().values()
        assert self.exchange.get_account().balance_total(USD) == Money(999998.00, USD)

    def test_process_quote_tick_fills_sell_limit_entry_with_bracket(self):
        # Arrange: Prepare market
        tick = TestStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        entry = self.strategy.order_factory.limit(
            USDJPY_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100000),
            Price.from_str("91.100"),
        )

        bracket = self.strategy.order_factory.bracket(
            entry_order=entry,
            stop_loss=Price.from_str("91.200"),
            take_profit=Price.from_str("90.000"),
        )

        self.strategy.submit_bracket_order(bracket)

        # Act
        tick2 = QuoteTick(
            USDJPY_SIM.id,
            Price.from_str("91.101"),
            Price.from_str("91.102"),
            Quantity.from_int(100000),
            Quantity.from_int(100000),
            0,
            0,
        )

        self.exchange.process_tick(tick2)

        # Assert
        assert entry.state == OrderState.FILLED
        assert bracket.stop_loss.state == OrderState.ACCEPTED
        assert bracket.take_profit.state == OrderState.ACCEPTED
        assert len(self.exchange.get_working_orders()) == 2  # SL and TP
        assert bracket.stop_loss in self.exchange.get_working_orders().values()
        assert bracket.take_profit in self.exchange.get_working_orders().values()

    def test_process_trade_tick_fills_buy_limit_entry_bracket(self):
        # Arrange: Prepare market
        tick1 = TradeTick(
            AUDUSD_SIM.id,
            Price.from_str("1.00000"),
            Quantity.from_int(100000),
            AggressorSide.SELL,
            "123456789",
            0,
            0,
        )

        tick2 = TradeTick(
            AUDUSD_SIM.id,
            Price.from_str("1.00001"),
            Quantity.from_int(100000),
            AggressorSide.BUY,
            "123456790",
            0,
            0,
        )

        self.data_engine.process(tick1)
        self.data_engine.process(tick2)
        self.exchange.process_tick(tick1)
        self.exchange.process_tick(tick2)

        entry = self.strategy.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("0.99900"),
        )

        bracket = self.strategy.order_factory.bracket(
            entry_order=entry,
            stop_loss=Price.from_str("0.99800"),
            take_profit=Price.from_str("1.100"),
        )

        self.strategy.submit_bracket_order(bracket)

        # Act
        tick3 = TradeTick(
            AUDUSD_SIM.id,
            Price.from_str("0.99899"),
            Quantity.from_int(100000),
            AggressorSide.BUY,  # Lowers bid price
            "123456789",
            0,
            0,
        )

        self.exchange.process_tick(tick3)

        # Assert
        assert entry.state == OrderState.FILLED
        assert bracket.stop_loss.state == OrderState.ACCEPTED
        assert bracket.take_profit.state == OrderState.ACCEPTED
        assert len(self.exchange.get_working_orders()) == 2  # SL and TP only
        assert bracket.stop_loss in self.exchange.get_working_orders().values()
        assert bracket.take_profit in self.exchange.get_working_orders().values()

    def test_filling_oco_sell_cancels_other_order(self):
        # Arrange: Prepare market
        tick = TestStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        entry = self.strategy.order_factory.limit(
            USDJPY_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100000),
            Price.from_str("91.100"),
        )

        bracket = self.strategy.order_factory.bracket(
            entry_order=entry,
            stop_loss=Price.from_str("91.200"),
            take_profit=Price.from_str("90.000"),
        )

        self.strategy.submit_bracket_order(bracket)

        # Act
        tick2 = QuoteTick(
            USDJPY_SIM.id,
            Price.from_str("91.101"),
            Price.from_str("91.102"),
            Quantity.from_int(100000),
            Quantity.from_int(100000),
            0,
            0,
        )

        tick3 = QuoteTick(
            USDJPY_SIM.id,
            Price.from_str("91.201"),
            Price.from_str("91.203"),
            Quantity.from_int(100000),
            Quantity.from_int(100000),
            0,
            0,
        )

        self.exchange.process_tick(tick2)
        self.exchange.process_tick(tick3)

        # Assert
        print(self.exchange.cache.position(PositionId("2-001")))
        assert entry.state == OrderState.FILLED
        assert bracket.stop_loss.state == OrderState.FILLED
        assert bracket.take_profit.state == OrderState.CANCELED
        assert len(self.exchange.get_working_orders()) == 0
        # TODO: WIP - fix handling of OCO orders
        # assert len(self.exchange.cache.positions_open()) == 0

    def test_realized_pnl_contains_commission(self):
        # Arrange: Prepare market
        tick = TestStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        order = self.strategy.order_factory.market(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        # Act
        self.strategy.submit_order(order)
        position = self.exec_engine.cache.positions_open()[0]

        # Assert
        assert position.realized_pnl == Money(-180.01, JPY)
        assert position.commissions() == [Money(180.01, JPY)]

    def test_unrealized_pnl(self):
        # Arrange: Prepare market
        tick = TestStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("90.002"),
            ask=Price.from_str("90.005"),
        )
        self.data_engine.process(tick)
        self.exchange.process_tick(tick)

        order_open = self.strategy.order_factory.market(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        # Act 1
        self.strategy.submit_order(order_open)

        quote = QuoteTick(
            USDJPY_SIM.id,
            Price.from_str("100.003"),
            Price.from_str("100.004"),
            Quantity.from_int(100000),
            Quantity.from_int(100000),
            0,
            0,
        )

        self.exchange.process_tick(quote)
        self.portfolio.update_tick(quote)

        order_reduce = self.strategy.order_factory.market(
            USDJPY_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(50000),
        )

        position_id = PositionId("2-001")  # Generated by platform

        # Act 2
        self.strategy.submit_order(order_reduce, position_id)

        # Assert
        position = self.exec_engine.cache.positions_open()[0]
        assert position.unrealized_pnl(Price.from_str("100.003")) == Money(499900.00, JPY)

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
            venue=SIM,
            venue_type=VenueType.ECN,
            oms_type=OMSType.HEDGING,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            is_frozen_account=True,  # <-- Freezing account
            starting_balances=[Money(1_000_000, USD)],
            instruments=[AUDUSD_SIM, USDJPY_SIM],
            modules=[],
            fill_model=FillModel(),
            cache=self.exec_engine.cache,
            clock=self.clock,
            logger=self.logger,
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
            USDJPY_SIM.id,
            Price.from_str("90.002"),
            Price.from_str("90.003"),
            Quantity.from_int(1_000_000),
            Quantity.from_int(1_000_000),
            0,
            0,
        )

        self.data_engine.process(open_quote)
        self.exchange.process_tick(open_quote)

        order_open = self.strategy.order_factory.market(
            USDJPY_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        # Act 1
        self.strategy.submit_order(order_open)

        reduce_quote = QuoteTick(
            USDJPY_SIM.id,
            Price.from_str("100.003"),
            Price.from_str("100.004"),
            Quantity.from_int(1_000_000),
            Quantity.from_int(1_000_000),
            0,
            0,
        )

        self.exchange.process_tick(reduce_quote)
        self.portfolio.update_tick(reduce_quote)

        order_reduce = self.strategy.order_factory.market(
            USDJPY_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(150000),
        )

        # Act 2
        self.strategy.submit_order(order_reduce, PositionId("2-001"))  # Generated by platform

        # Assert
        position_open = self.exec_engine.cache.positions_open()[0]
        position_closed = self.exec_engine.cache.positions_closed()[0]
        assert position_open.side == PositionSide.SHORT
        assert position_open.quantity == Quantity.from_int(50000)
        assert position_closed.realized_pnl == Money(999619.98, JPY)
        assert position_closed.commissions() == [Money(380.02, JPY)]
        assert self.exchange.get_account().balance_total(USD) == Money(1016660.97, USD)


class TestBitmexExchange:
    def setup(self):
        # Fixture Setup
        self.strategies = [MockStrategy(TestStubs.bartype_btcusdt_binance_100tick_last())]

        self.clock = TestClock()
        self.uuid_factory = UUIDFactory()
        self.logger = Logger(self.clock)

        self.msgbus = MessageBus(
            clock=self.clock,
            logger=self.logger,
        )

        self.cache = TestStubs.cache()

        self.portfolio = Portfolio(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.data_engine = DataEngine(
            portfolio=self.portfolio,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
            config={"use_previous_close": False},  # To correctly reproduce historical data bars
        )

        self.trader_id = TestStubs.trader_id()
        self.account_id = AccountId("BITMEX", "001")

        self.exec_engine = ExecutionEngine(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.risk_engine = RiskEngine(
            exec_engine=self.exec_engine,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.exchange = SimulatedExchange(
            venue=Venue("BITMEX"),
            venue_type=VenueType.EXCHANGE,
            oms_type=OMSType.NETTING,
            account_type=AccountType.MARGIN,
            base_currency=BTC,
            starting_balances=[Money(20, BTC)],
            is_frozen_account=False,
            cache=self.exec_engine.cache,
            instruments=[XBTUSD_BITMEX],
            modules=[],
            fill_model=FillModel(),
            clock=self.clock,
            logger=self.logger,
        )

        self.exec_client = BacktestExecClient(
            exchange=self.exchange,
            account_id=self.account_id,
            account_type=AccountType.MARGIN,
            base_currency=BTC,
            engine=self.exec_engine,
            clock=self.clock,
            logger=self.logger,
        )

        # Wire up components
        self.data_engine.cache.add_instrument(XBTUSD_BITMEX)

        self.exec_engine.register_client(self.exec_client)
        self.exchange.register_client(self.exec_client)

        self.exec_engine.cache.add_instrument(XBTUSD_BITMEX)

        self.strategy = MockStrategy(bar_type=TestStubs.bartype_btcusdt_binance_100tick_last())
        self.strategy.register(
            self.trader_id,
            self.msgbus,
            self.portfolio,
            self.data_engine,
            self.risk_engine,
            self.clock,
            self.logger,
        )

        self.exchange.reset()
        self.data_engine.start()
        self.exec_engine.start()
        self.strategy.start()

    def test_commission_maker_taker_order(self):
        # Arrange
        # Prepare market
        quote1 = QuoteTick(
            XBTUSD_BITMEX.id,
            Price.from_str("11493.0"),
            Price.from_str("11493.5"),
            Quantity.from_int(1500000),
            Quantity.from_int(1500000),
            0,
            0,
        )

        self.data_engine.process(quote1)
        self.exchange.process_tick(quote1)

        order_market = self.strategy.order_factory.market(
            XBTUSD_BITMEX.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        order_limit = self.strategy.order_factory.limit(
            XBTUSD_BITMEX.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("11492.5"),
        )

        # Act
        self.strategy.submit_order(order_market)
        self.strategy.submit_order(order_limit)

        quote2 = QuoteTick(
            XBTUSD_BITMEX.id,
            Price.from_str("11491.0"),
            Price.from_str("11491.5"),
            Quantity.from_int(1500000),
            Quantity.from_int(1500000),
            0,
            0,
        )

        self.exchange.process_tick(quote2)  # Fill the limit order
        self.portfolio.update_tick(quote2)

        # Assert
        assert self.strategy.object_storer.get_store()[2].liquidity_side == LiquiditySide.TAKER
        assert self.strategy.object_storer.get_store()[7].liquidity_side == LiquiditySide.MAKER
        assert self.strategy.object_storer.get_store()[2].commission == Money(0.00652543, BTC)
        assert self.strategy.object_storer.get_store()[7].commission == Money(-0.00217552, BTC)


class TestOrderBookExchange:
    def setup(self):
        # Fixture Setup
        self.clock = TestClock()
        self.uuid_factory = UUIDFactory()
        self.logger = Logger(self.clock)

        self.msgbus = MessageBus(
            clock=self.clock,
            logger=self.logger,
        )

        self.cache = TestStubs.cache()

        self.portfolio = Portfolio(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.data_engine = DataEngine(
            portfolio=self.portfolio,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
            config={"use_previous_close": False},  # To correctly reproduce historical data bars
        )

        self.trader_id = TestStubs.trader_id()
        self.account_id = AccountId("SIM", "001")

        self.exec_engine = ExecutionEngine(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.risk_engine = RiskEngine(
            exec_engine=self.exec_engine,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.exchange = SimulatedExchange(
            venue=SIM,
            venue_type=VenueType.ECN,
            oms_type=OMSType.HEDGING,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            starting_balances=[Money(1_000_000, USD)],
            is_frozen_account=False,
            instruments=[AUDUSD_SIM, USDJPY_SIM],
            modules=[],
            fill_model=FillModel(),
            cache=self.exec_engine.cache,
            clock=self.clock,
            logger=self.logger,
            exchange_order_book_level=BookLevel.L2,
        )

        self.exec_client = BacktestExecClient(
            exchange=self.exchange,
            account_id=self.account_id,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            engine=self.exec_engine,
            clock=self.clock,
            logger=self.logger,
        )

        # Prepare components
        self.data_engine.cache.add_instrument(AUDUSD_SIM)
        self.data_engine.cache.add_instrument(USDJPY_SIM)
        self.exec_engine.cache.add_instrument(AUDUSD_SIM)
        self.exec_engine.cache.add_instrument(USDJPY_SIM)
        self.data_engine.cache.add_order_book(
            OrderBook.create(
                instrument=USDJPY_SIM,
                level=BookLevel.L2,
            )
        )

        self.exec_engine.register_client(self.exec_client)
        self.exchange.register_client(self.exec_client)

        self.strategy = MockStrategy(bar_type=TestStubs.bartype_usdjpy_1min_bid())
        self.strategy.register(
            self.trader_id,
            self.msgbus,
            self.portfolio,
            self.data_engine,
            self.risk_engine,
            self.clock,
            self.logger,
        )

        self.exchange.reset()
        self.data_engine.start()
        self.exec_engine.start()
        self.strategy.start()

    def test_submit_limit_order_aggressive_multiple_levels(self):
        # Arrange: Prepare market
        self.exec_engine.cache.add_instrument(USDJPY_SIM)

        quote = QuoteTick(
            USDJPY_SIM.id,
            Price.from_str("110.000"),
            Price.from_str("110.010"),
            Quantity.from_int(1500000),
            Quantity.from_int(1500000),
            0,
            0,
        )
        self.data_engine.process(quote)
        snapshot = TestStubs.order_book_snapshot(
            instrument_id=USDJPY_SIM.id,
            bid_volume=1000,
            ask_volume=1000,
        )
        self.data_engine.process(snapshot)
        self.exchange.process_order_book(snapshot)

        # Create order
        order = self.strategy.order_factory.limit(
            instrument_id=USDJPY_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(2000),
            price=Price.from_int(20),
            post_only=False,
        )

        # Act
        self.strategy.submit_order(order)

        # Assert
        assert order.state == OrderState.FILLED
        assert order.filled_qty == Decimal("2000.0")  # No slippage
        assert order.avg_px == Decimal("15.33333333333333333333333333")
        assert self.exchange.get_account().balance_total(USD) == Money(999999.98, USD)

    def test_aggressive_partial_fill(self):
        # Arrange: Prepare market
        self.exec_engine.cache.add_instrument(USDJPY_SIM)

        quote = QuoteTick(
            USDJPY_SIM.id,
            Price.from_str("110.000"),
            Price.from_str("110.010"),
            Quantity.from_int(1500000),
            Quantity.from_int(1500000),
            0,
            0,
        )
        self.data_engine.process(quote)
        snapshot = TestStubs.order_book_snapshot(
            instrument_id=USDJPY_SIM.id,
            bid_volume=1000,
            ask_volume=1000,
        )
        self.data_engine.process(snapshot)
        self.exchange.process_order_book(snapshot)

        # Act
        order = self.strategy.order_factory.limit(
            instrument_id=USDJPY_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(7000),
            price=Price.from_int(20),
            post_only=False,
        )
        self.strategy.submit_order(order)

        # Assert
        assert order.state == OrderState.PARTIALLY_FILLED
        assert order.filled_qty == Quantity.from_str("6000.0")  # No slippage
        assert order.avg_px == Decimal("15.93333333333333333333333333")
        assert self.exchange.get_account().balance_total(USD) == Money(999999.94, USD)

    def test_passive_post_only_insert(self):
        # Arrange: Prepare market
        self.exec_engine.cache.add_instrument(USDJPY_SIM)
        # Market is 10 @ 15
        snapshot = TestStubs.order_book_snapshot(
            instrument_id=USDJPY_SIM.id, bid_volume=1000, ask_volume=1000
        )
        self.data_engine.process(snapshot)
        self.exchange.process_order_book(snapshot)

        # Act
        order = self.strategy.order_factory.limit(
            instrument_id=USDJPY_SIM.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(2000),
            price=Price.from_str("14"),
            post_only=True,
        )
        self.strategy.submit_order(order)

        # Assert
        assert order.state == OrderState.ACCEPTED

    # TODO - Need to discuss how we are going to support passive quotes trading now
    @pytest.mark.skip
    def test_passive_partial_fill(self):
        # Arrange: Prepare market
        self.exec_engine.cache.add_instrument(USDJPY_SIM)
        # Market is 10 @ 15
        snapshot = TestStubs.order_book_snapshot(
            instrument_id=USDJPY_SIM.id, bid_volume=1000, ask_volume=1000
        )
        self.data_engine.process(snapshot)
        self.exchange.process_order_book(snapshot)

        order = self.strategy.order_factory.limit(
            instrument_id=USDJPY_SIM.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(1000),
            price=Price.from_str("14"),
            post_only=False,
        )
        self.strategy.submit_order(order)

        # Act
        tick = TestStubs.quote_tick_3decimal(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("15"),
            bid_volume=Quantity.from_int(1000),
            ask=Price.from_str("16"),
            ask_volume=Quantity.from_int(1000),
        )
        # New tick will be in cross with our order
        self.exchange.process_tick(tick)

        # Assert
        assert order.state == OrderState.PARTIALLY_FILLED
        assert order.filled_qty == Quantity.from_str("1000.0")
        assert order.avg_px == Decimal("15.0")

    # TODO - Need to discuss how we are going to support passive quotes trading now
    @pytest.mark.skip
    def test_passive_fill_on_trade_tick(self):
        # Arrange: Prepare market
        # Market is 10 @ 15
        snapshot = TestStubs.order_book_snapshot(
            instrument_id=USDJPY_SIM.id, bid_volume=1000, ask_volume=1000
        )
        self.data_engine.process(snapshot)
        self.exchange.process_order_book(snapshot)

        order = self.strategy.order_factory.limit(
            instrument_id=USDJPY_SIM.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(2000),
            price=Price.from_str("14"),
            post_only=False,
        )
        self.strategy.submit_order(order)

        # Act
        tick1 = TradeTick(
            USDJPY_SIM.id,
            Price.from_str("14.0"),
            Quantity.from_int(1000),
            OrderSide.SELL,
            "123456789",
            0,
            0,
        )
        self.exchange.process_tick(tick1)

        # Assert
        assert order.state == OrderState.PARTIALLY_FILLED
        assert order.filled_qty == Quantity.from_str("1000.0")  # No slippage
        assert order.avg_px == Decimal("14.0")
