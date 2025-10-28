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

import pytest

from nautilus_trader.backtest.engine import SimulatedExchange
from nautilus_trader.backtest.execution_client import BacktestExecClient
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.backtest.models import LatencyModel
from nautilus_trader.backtest.models import MakerTakerFeeModel
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.component import TestClock
from nautilus_trader.config import ExecEngineConfig
from nautilus_trader.config import RiskEngineConfig
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.model.currencies import JPY
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import TrailingOffsetType
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.risk.engine import RiskEngine
from nautilus_trader.test_kit.mocks.strategies import MockStrategy
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
USDJPY_SIM = TestInstrumentProvider.default_fx_ccy("USD/JPY")


class TestSimulatedExchange:
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
            clock=self.clock,
            cache=self.cache,
        )

        self.exec_engine = ExecutionEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            config=ExecEngineConfig(debug=True),
        )

        self.risk_engine = RiskEngine(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            config=RiskEngineConfig(debug=True),
        )

        self.exchange = SimulatedExchange(
            venue=Venue("SIM"),
            oms_type=OmsType.HEDGING,
            account_type=AccountType.MARGIN,
            base_currency=None,
            starting_balances=[
                Money(1_000_000, USD),
                Money(100_000_000, JPY),
            ],
            default_leverage=Decimal(50),
            leverages={AUDUSD_SIM.id: Decimal(10)},
            modules=[],
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            latency_model=LatencyModel(0),
        )
        self.exchange.add_instrument(USDJPY_SIM)

        self.exec_client = BacktestExecClient(
            exchange=self.exchange,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
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
        )

        # Start components
        self.exchange.reset()
        self.data_engine.start()
        self.exec_engine.start()
        self.strategy.start()

    def test_trailing_stop_market_order_for_unsupported_offset_type_raises_runtime_error(
        self,
    ) -> None:
        # Arrange: Prepare market
        trailing_stop = self.strategy.order_factory.trailing_stop_market(
            instrument_id=USDJPY_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(200_000),
            trailing_offset_type=TrailingOffsetType.BASIS_POINTS,
            trailing_offset=Decimal("1.0"),
            trigger_type=TriggerType.BID_ASK,
        )
        self.strategy.submit_order(trailing_stop)

        # Assert
        with pytest.raises(RuntimeError):
            self.exchange.process(0)

    def test_trailing_stop_market_order_bid_ask_when_no_quote_ticks_raises_runtime_error(
        self,
    ) -> None:
        # Arrange: Prepare market
        trailing_stop = self.strategy.order_factory.trailing_stop_market(
            instrument_id=USDJPY_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(200_000),
            trailing_offset_type=TrailingOffsetType.PRICE,
            trailing_offset=Decimal("1.0"),
            trigger_type=TriggerType.BID_ASK,
        )
        self.strategy.submit_order(trailing_stop)

        # Assert
        with pytest.raises(RuntimeError):
            self.exchange.process(0)

    def test_trailing_stop_market_order_last_when_no_quote_ticks_raises_runtime_error(self) -> None:
        # Arrange: Prepare market
        trailing_stop = self.strategy.order_factory.trailing_stop_market(
            instrument_id=USDJPY_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(200_000),
            trailing_offset_type=TrailingOffsetType.PRICE,
            trailing_offset=Decimal("1.0"),
            trigger_type=TriggerType.LAST_PRICE,
        )
        self.strategy.submit_order(trailing_stop)

        # Assert
        with pytest.raises(RuntimeError):
            self.exchange.process(0)

    def test_trailing_stop_market_order_last_or_bid_ask_when_no_market_raises_runtime_error(
        self,
    ) -> None:
        # Arrange: Prepare market
        trailing_stop = self.strategy.order_factory.trailing_stop_market(
            instrument_id=USDJPY_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(200_000),
            trailing_offset_type=TrailingOffsetType.PRICE,
            trailing_offset=Decimal("1.0"),
            trigger_type=TriggerType.LAST_OR_BID_ASK,
        )
        self.strategy.submit_order(trailing_stop)

        # Assert
        with pytest.raises(RuntimeError):
            self.exchange.process(0)

    @pytest.mark.parametrize(
        (
            "order_side",
            "trailing_offset_type",
            "trailing_offset",
            "trigger_type",
            "expected_activation",
            "expected_trigger",
        ),
        [
            [
                OrderSide.BUY,
                TrailingOffsetType.PRICE,
                Decimal("1.0"),
                TriggerType.BID_ASK,
                Price.from_str("14.000"),
                Price.from_str("15.000"),
            ],
            [
                OrderSide.SELL,
                TrailingOffsetType.PRICE,
                Decimal("1.0"),
                TriggerType.BID_ASK,
                Price.from_str("13.000"),
                Price.from_str("12.000"),
            ],
        ],
    )
    def test_trailing_stop_market_order_bid_ask_with_no_trigger_updates_order(
        self,
        order_side: OrderSide,
        trailing_offset_type: TrailingOffsetType,
        trailing_offset: Decimal,
        trigger_type: TriggerType,
        expected_activation: Price,
        expected_trigger: Price,
    ) -> None:
        # Arrange: Prepare market
        quote = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=13.0,
            ask_price=14.0,
        )
        self.exchange.process_quote_tick(quote)
        self.data_engine.process(quote)
        self.portfolio.update_quote_tick(quote)

        # Act
        trailing_stop = self.strategy.order_factory.trailing_stop_market(
            instrument_id=USDJPY_SIM.id,
            order_side=order_side,
            quantity=Quantity.from_int(200_000),
            trailing_offset_type=trailing_offset_type,
            trailing_offset=trailing_offset,
            trigger_type=trigger_type,
        )
        self.strategy.submit_order(trailing_stop)
        self.exchange.process(0)

        # Assert
        assert trailing_stop.activation_price == expected_activation
        assert trailing_stop.trigger_price == expected_trigger

    @pytest.mark.parametrize(
        (
            "order_side",
            "trailing_offset_type",
            "trailing_offset",
            "trigger_type",
            "expected_activation",
            "expected_trigger",
        ),
        [
            [
                OrderSide.BUY,
                TrailingOffsetType.PRICE,
                Decimal("1.0"),
                TriggerType.LAST_PRICE,
                Price.from_str("14.000"),
                Price.from_str("15.000"),
            ],
            [
                OrderSide.SELL,
                TrailingOffsetType.PRICE,
                Decimal("1.0"),
                TriggerType.LAST_PRICE,
                Price.from_str("14.000"),
                Price.from_str("13.000"),
            ],
            [
                OrderSide.BUY,
                TrailingOffsetType.PRICE,
                Decimal("1.0"),
                TriggerType.LAST_OR_BID_ASK,
                Price.from_str("14.000"),
                Price.from_str("15.000"),
            ],
            [
                OrderSide.SELL,
                TrailingOffsetType.PRICE,
                Decimal("1.0"),
                TriggerType.LAST_OR_BID_ASK,
                Price.from_str("14.000"),
                Price.from_str("13.000"),
            ],
        ],
    )
    def test_trailing_stop_market_order_last_with_no_trigger_updates_order(
        self,
        order_side: OrderSide,
        trailing_offset_type: TrailingOffsetType,
        trailing_offset: Decimal,
        trigger_type: TriggerType,
        expected_activation: Price,
        expected_trigger: Price,
    ) -> None:
        # Arrange: Prepare market
        quote = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=13.0,
            ask_price=14.0,
        )
        self.exchange.process_quote_tick(quote)
        self.data_engine.process(quote)
        self.portfolio.update_quote_tick(quote)

        trade = TradeTick(
            instrument_id=USDJPY_SIM.id,
            price=Price.from_str("14.000"),
            size=Quantity.from_int(1),
            aggressor_side=AggressorSide.BUYER,
            trade_id=TradeId("123456"),
            ts_event=0,
            ts_init=0,
        )
        self.exchange.process_trade_tick(trade)
        self.data_engine.process(trade)

        # Act
        trailing_stop = self.strategy.order_factory.trailing_stop_market(
            instrument_id=USDJPY_SIM.id,
            order_side=order_side,
            quantity=Quantity.from_int(200_000),
            trailing_offset_type=trailing_offset_type,
            trailing_offset=trailing_offset,
            trigger_type=trigger_type,
        )
        self.strategy.submit_order(trailing_stop)
        self.exchange.process(0)

        # Assert
        assert trailing_stop.activation_price == expected_activation
        assert trailing_stop.trigger_price == expected_trigger

    def test_trailing_stop_market_order_buy_bid_ask_price_when_offset_activated_updates_order(
        self,
    ) -> None:
        # Arrange: Prepare market
        quote1 = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=13.0,
            ask_price=14.0,
        )
        self.exchange.process_quote_tick(quote1)
        self.data_engine.process(quote1)
        self.portfolio.update_quote_tick(quote1)

        trailing_stop = self.strategy.order_factory.trailing_stop_market(
            instrument_id=USDJPY_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(200_000),
            trigger_price=Price.from_str("15.000"),
            trailing_offset_type=TrailingOffsetType.PRICE,
            trailing_offset=Decimal("1.0"),
            trigger_type=TriggerType.BID_ASK,
        )
        self.strategy.submit_order(trailing_stop)
        self.exchange.process(0)

        quote2 = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=12.0,
            ask_price=13.0,
        )
        self.exchange.process_quote_tick(quote2)

        # Act: market moves against trailing stop (should not update)
        quote3 = QuoteTick(
            instrument_id=USDJPY_SIM.id,
            bid_price=Price.from_str("12.500"),
            ask_price=Price.from_str("13.500"),
            bid_size=Quantity.from_int(1_000_000),
            ask_size=Quantity.from_int(1_000_000),
            ts_event=0,
            ts_init=0,
        )
        self.exchange.process_quote_tick(quote3)

        # Assert
        assert trailing_stop.trigger_price == Price.from_str("14.0")

    def test_trailing_stop_market_order_sell_bid_ask_price_when_offset_activated_updates_order(
        self,
    ) -> None:
        # Arrange: Prepare market
        quote1 = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=13.0,
            ask_price=14.0,
        )
        self.exchange.process_quote_tick(quote1)
        self.data_engine.process(quote1)
        self.portfolio.update_quote_tick(quote1)

        trailing_stop = self.strategy.order_factory.trailing_stop_market(
            instrument_id=USDJPY_SIM.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(200_000),
            trigger_price=Price.from_str("12.000"),
            trailing_offset_type=TrailingOffsetType.PRICE,
            trailing_offset=Decimal("1.0"),
            trigger_type=TriggerType.BID_ASK,
        )
        self.strategy.submit_order(trailing_stop)
        self.exchange.process(0)

        quote2 = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=14.0,
            ask_price=15.0,
        )
        self.exchange.process_quote_tick(quote2)

        # Act: market moves against trailing stop (should not update)
        tick = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=13.5,
            ask_price=14.5,
        )
        self.exchange.process_quote_tick(tick)

        # Assert
        assert trailing_stop.trigger_price == Price.from_str("13.000")

    def test_trailing_stop_market_order_trail_activate_and_sell(
        self,
    ) -> None:
        # Arrange: Prepare market
        tick = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=13.0,
            ask_price=14.0,
        )
        self.exchange.process_quote_tick(tick)
        self.data_engine.process(tick)
        self.portfolio.update_quote_tick(tick)

        trailing_stop = self.strategy.order_factory.trailing_stop_market(
            instrument_id=USDJPY_SIM.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(200_000),
            activation_price=Price.from_str("15.000"),
            trailing_offset_type=TrailingOffsetType.PRICE,
            trailing_offset=Decimal("1.0"),
            trigger_type=TriggerType.BID_ASK,
        )
        self.strategy.submit_order(trailing_stop)
        self.exchange.process(0)

        # When the activation_price is set higher than the current market price,
        # the order should remain inactive until the market reaches the activation price.
        assert not trailing_stop.is_activated
        assert trailing_stop.activation_price == Price.from_str("15.000")
        assert trailing_stop.trigger_price is None

        tick = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=15.5,  # causes activation of the order
            ask_price=16.0,
        )
        self.exchange.process_quote_tick(tick)

        # When the market reaches the activation price,
        # the trigger_price should be set based on the given offset and continue to trail the market.
        assert trailing_stop.is_activated
        assert trailing_stop.activation_price == Price.from_str("15.000")
        assert trailing_stop.trigger_price == Price.from_str("14.500")

        tick = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=16.5,  # raises trigger_price of the order
            ask_price=17.0,
        )
        self.exchange.process_quote_tick(tick)

        # When the market moves in a favorable direction,
        # the trigger_price should continue to adjust, trailing the market.
        assert trailing_stop.activation_price == Price.from_str("15.000")
        assert trailing_stop.trigger_price == Price.from_str("15.500")

        tick = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=16.0,
            ask_price=16.5,
        )
        self.exchange.process_quote_tick(tick)

        # When the market moves in an unfavorable direction,
        # the trigger_price should remain unchanged until it is triggered.
        assert trailing_stop.trigger_price == Price.from_str("15.500")

        tick = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=15.0,
            ask_price=15.5,
        )
        self.exchange.process_quote_tick(tick)

        # When the market reaches the trigger price, the order should be triggered and filled.
        assert trailing_stop.trigger_price == Price.from_str("15.500")
        assert trailing_stop.status == OrderStatus.FILLED
        assert trailing_stop.liquidity_side == LiquiditySide.TAKER
        assert trailing_stop.filled_qty == 200_000

    def test_trailing_stop_market_order_trail_activate_and_buy(
        self,
    ) -> None:
        # Arrange: Prepare market
        tick = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=13.0,
            ask_price=14.0,
        )
        self.exchange.process_quote_tick(tick)
        self.data_engine.process(tick)
        self.portfolio.update_quote_tick(tick)

        trailing_stop = self.strategy.order_factory.trailing_stop_market(
            instrument_id=USDJPY_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(200_000),
            activation_price=Price.from_str("12.000"),
            trailing_offset_type=TrailingOffsetType.PRICE,
            trailing_offset=Decimal("1.0"),
            trigger_type=TriggerType.BID_ASK,
        )
        self.strategy.submit_order(trailing_stop)
        self.exchange.process(0)

        # When the activation_price is set higher than the current market price,
        # the order should remain inactive until the market reaches the activation price.
        assert not trailing_stop.is_activated
        assert trailing_stop.activation_price == Price.from_str("12.000")
        assert trailing_stop.trigger_price is None

        tick = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=11.0,
            ask_price=11.5,  # causes activation of the order
        )
        self.exchange.process_quote_tick(tick)

        # When the market reaches the activation price,
        # the trigger_price should be set based on the given offset and continue to trail the market.
        assert trailing_stop.is_activated
        assert trailing_stop.activation_price == Price.from_str("12.000")
        assert trailing_stop.trigger_price == Price.from_str("12.500")

        tick = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=10.0,
            ask_price=10.5,  # lowers trigger_price of the order
        )
        self.exchange.process_quote_tick(tick)

        # When the market moves down in a favorable direction,
        # the trigger_price should continue to adjust, trailing the market.
        assert trailing_stop.activation_price == Price.from_str("12.000")
        assert trailing_stop.trigger_price == Price.from_str("11.500")

        tick = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=10.5,
            ask_price=11.0,
        )
        self.exchange.process_quote_tick(tick)

        # When the market moves upward in an unfavorable direction,
        # the trigger_price should remain unchanged until it is triggered.
        assert trailing_stop.trigger_price == Price.from_str("11.500")

        tick = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=11.0,
            ask_price=12.0,
        )
        self.exchange.process_quote_tick(tick)

        # When the market moves above the trigger price, the order should be triggered, and filled
        assert trailing_stop.trigger_price == Price.from_str("11.500")
        assert trailing_stop.status == OrderStatus.FILLED
        assert trailing_stop.filled_qty == 200_000
        assert trailing_stop.liquidity_side == LiquiditySide.TAKER

    @pytest.mark.parametrize(
        (
            "order_side",
            "trailing_offset_type",
            "trailing_offset",
            "trigger_type",
            "expected_activation",
            "expected_trigger",
            "expected_price",
        ),
        [
            [
                OrderSide.BUY,
                TrailingOffsetType.PRICE,
                Decimal("1.0"),
                TriggerType.BID_ASK,
                Price.from_str("14.000"),
                Price.from_str("15.000"),
                Price.from_str("15.000"),
            ],
            [
                OrderSide.SELL,
                TrailingOffsetType.PRICE,
                Decimal("1.0"),
                TriggerType.BID_ASK,
                Price.from_str("13.000"),
                Price.from_str("12.000"),
                Price.from_str("12.000"),
            ],
            [
                OrderSide.BUY,
                TrailingOffsetType.BASIS_POINTS,
                Decimal("100"),
                TriggerType.BID_ASK,
                Price.from_str("14.000"),
                Price.from_str("14.140"),
                Price.from_str("14.140"),
            ],
            [
                OrderSide.SELL,
                TrailingOffsetType.BASIS_POINTS,
                Decimal("100"),
                TriggerType.BID_ASK,
                Price.from_str("13.000"),
                Price.from_str("12.870"),
                Price.from_str("12.870"),
            ],
        ],
    )
    def test_trailing_stop_limit_order_bid_ask_with_no_trigger_updates_order(
        self,
        order_side: OrderSide,
        trailing_offset_type: TrailingOffsetType,
        trailing_offset: Decimal,
        trigger_type: TriggerType,
        expected_activation: Price,
        expected_trigger: Price,
        expected_price: Price,
    ) -> None:
        # Arrange: Prepare market
        quote = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=13.0,
            ask_price=14.0,
        )
        self.exchange.process_quote_tick(quote)
        self.data_engine.process(quote)
        self.portfolio.update_quote_tick(quote)

        # Act
        trailing_stop = self.strategy.order_factory.trailing_stop_limit(
            instrument_id=USDJPY_SIM.id,
            order_side=order_side,
            quantity=Quantity.from_int(200_000),
            trailing_offset_type=trailing_offset_type,
            trailing_offset=trailing_offset,
            limit_offset=trailing_offset,
            trigger_type=trigger_type,
        )
        self.strategy.submit_order(trailing_stop)
        self.exchange.process(0)

        # Assert
        assert trailing_stop.activation_price == expected_activation
        assert trailing_stop.trigger_price == expected_trigger
        assert trailing_stop.price == expected_price

    @pytest.mark.parametrize(
        (
            "order_side",
            "trailing_offset_type",
            "trailing_offset",
            "trigger_type",
            "expected_trigger",
            "expected_price",
        ),
        [
            [
                OrderSide.BUY,
                TrailingOffsetType.PRICE,
                Decimal("1.0"),
                TriggerType.LAST_PRICE,
                Price.from_str("15.000"),
                Price.from_str("15.000"),
            ],
            [
                OrderSide.SELL,
                TrailingOffsetType.PRICE,
                Decimal("1.0"),
                TriggerType.LAST_PRICE,
                Price.from_str("13.000"),
                Price.from_str("13.000"),
            ],
            [
                OrderSide.BUY,
                TrailingOffsetType.PRICE,
                Decimal("1.0"),
                TriggerType.LAST_OR_BID_ASK,
                Price.from_str("15.000"),
                Price.from_str("15.000"),
            ],
            [
                OrderSide.SELL,
                TrailingOffsetType.PRICE,
                Decimal("1.0"),
                TriggerType.LAST_OR_BID_ASK,
                Price.from_str("13.000"),
                Price.from_str("13.000"),
            ],
            [
                OrderSide.BUY,
                TrailingOffsetType.BASIS_POINTS,
                Decimal("100"),
                TriggerType.LAST_PRICE,
                Price.from_str("14.140"),
                Price.from_str("14.140"),
            ],
            [
                OrderSide.SELL,
                TrailingOffsetType.BASIS_POINTS,
                Decimal("100"),
                TriggerType.LAST_PRICE,
                Price.from_str("13.860"),
                Price.from_str("13.860"),
            ],
            [
                OrderSide.BUY,
                TrailingOffsetType.BASIS_POINTS,
                Decimal("100"),
                TriggerType.LAST_OR_BID_ASK,
                Price.from_str("14.140"),
                Price.from_str("14.140"),
            ],
            [
                OrderSide.SELL,
                TrailingOffsetType.BASIS_POINTS,
                Decimal("100"),
                TriggerType.LAST_OR_BID_ASK,
                Price.from_str("13.860"),
                Price.from_str("13.860"),
            ],
        ],
    )
    def test_trailing_stop_limit_order_last_with_no_trigger_updates_order(
        self,
        order_side: OrderSide,
        trailing_offset_type: TrailingOffsetType,
        trailing_offset: Decimal,
        trigger_type: TriggerType,
        expected_trigger: Price,
        expected_price: Price,
    ) -> None:
        # Arrange: Prepare market
        quote = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=13.0,
            ask_price=14.0,
        )
        self.exchange.process_quote_tick(quote)
        self.data_engine.process(quote)
        self.portfolio.update_quote_tick(quote)

        trade = TradeTick(
            instrument_id=USDJPY_SIM.id,
            price=Price.from_str("14.000"),
            size=Quantity.from_int(1),
            aggressor_side=AggressorSide.BUYER,
            trade_id=TradeId("123456"),
            ts_event=0,
            ts_init=0,
        )
        self.exchange.process_trade_tick(trade)
        self.data_engine.process(trade)

        # Act
        trailing_stop = self.strategy.order_factory.trailing_stop_limit(
            instrument_id=USDJPY_SIM.id,
            order_side=order_side,
            quantity=Quantity.from_int(200_000),
            trailing_offset_type=trailing_offset_type,
            trailing_offset=trailing_offset,
            limit_offset=trailing_offset,
            trigger_type=trigger_type,
        )
        self.strategy.submit_order(trailing_stop)
        self.exchange.process(0)

        # Assert
        assert trailing_stop.trigger_price == expected_trigger
        assert trailing_stop.price == expected_price

    @pytest.mark.parametrize(
        (
            "order_side",
            "first_tick",
            "subsequent_ticks",
            "initial_price",
            "initial_trigger",
            "expected_trigger",
            "expected_price",
        ),
        [
            pytest.param(
                OrderSide.BUY,
                (13.0, 14.0),
                [(12.0, 13.0), (12.5, 13.5)],
                Price.from_str("15.000"),
                Price.from_str("15.000"),
                Price.from_str("14.000"),
                Price.from_str("14.000"),
                id="buy_price_offset",
            ),
            pytest.param(
                OrderSide.SELL,
                (13.0, 14.0),
                [(14.0, 15.0), (13.5, 14.5)],
                Price.from_str("12.000"),
                Price.from_str("12.000"),
                Price.from_str("13.000"),
                Price.from_str("13.000"),
                id="sell_price_offset",
            ),
        ],
    )
    def test_trailing_stop_limit_order_bid_ask_price_offset_activated_does_not_update(
        self,
        order_side: OrderSide,
        first_tick: tuple[float, float],
        subsequent_ticks: list[tuple[float, float]],
        initial_price: Price,
        initial_trigger: Price,
        expected_trigger: Price,
        expected_price: Price,
    ) -> None:
        # Arrange initial market tick
        bid0, ask0 = first_tick
        quote1 = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=bid0,
            ask_price=ask0,
        )
        self.exchange.process_quote_tick(quote1)
        self.data_engine.process(quote1)
        self.portfolio.update_quote_tick(quote1)

        # Submit trailing stop limit order
        trailing_stop = self.strategy.order_factory.trailing_stop_limit(
            instrument_id=USDJPY_SIM.id,
            order_side=order_side,
            quantity=Quantity.from_int(200_000),
            price=initial_price,
            trigger_price=initial_trigger,
            trailing_offset_type=TrailingOffsetType.PRICE,
            trailing_offset=Decimal("1.0"),
            limit_offset=Decimal("1.0"),
            trigger_type=TriggerType.BID_ASK,
        )
        self.strategy.submit_order(trailing_stop)
        self.exchange.process(0)

        # Act: market moves against trailing stop (should not adjust)
        for bid, ask in subsequent_ticks:
            quote = TestDataStubs.quote_tick(
                instrument=USDJPY_SIM,
                bid_price=bid,
                ask_price=ask,
            )
            self.exchange.process_quote_tick(quote)

        # Assert
        assert trailing_stop.trigger_price == expected_trigger
        assert trailing_stop.price == expected_price

    @pytest.mark.parametrize(
        (
            "order_side",
            "first_tick",
            "subsequent_ticks",
            "initial_price",
            "initial_trigger",
            "expected_trigger",
            "expected_price",
        ),
        [
            pytest.param(
                OrderSide.BUY,
                (13.0, 14.0),
                [(12.0, 13.0), (12.5, 13.5)],
                Price.from_str("15.000"),
                Price.from_str("15.000"),
                Price.from_str("13.260"),
                Price.from_str("13.260"),
                id="buy_basis_points",
            ),
            pytest.param(
                OrderSide.SELL,
                (13.0, 14.0),
                [(14.0, 15.0), (13.5, 14.5)],
                Price.from_str("12.000"),
                Price.from_str("12.000"),
                Price.from_str("13.720"),
                Price.from_str("13.720"),
                id="sell_basis_points",
            ),
        ],
    )
    def test_trailing_stop_limit_order_bid_ask_basis_points_offset_activated_does_not_update(
        self,
        order_side: OrderSide,
        first_tick: tuple[float, float],
        subsequent_ticks: list[tuple[float, float]],
        initial_price: Price,
        initial_trigger: Price,
        expected_trigger: Price,
        expected_price: Price,
    ) -> None:
        # Arrange initial market tick
        bid0, ask0 = first_tick
        quote1 = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=bid0,
            ask_price=ask0,
        )
        self.exchange.process_quote_tick(quote1)
        self.data_engine.process(quote1)
        self.portfolio.update_quote_tick(quote1)

        # Prepare fixed offset for basis-points
        offset = Decimal("200")
        # Submit trailing stop limit order
        trailing_stop = self.strategy.order_factory.trailing_stop_limit(
            instrument_id=USDJPY_SIM.id,
            order_side=order_side,
            quantity=Quantity.from_int(200_000),
            price=initial_price,
            trigger_price=initial_trigger,
            trailing_offset_type=TrailingOffsetType.BASIS_POINTS,
            trailing_offset=offset,
            limit_offset=offset,
            trigger_type=TriggerType.BID_ASK,
        )
        self.strategy.submit_order(trailing_stop)
        self.exchange.process(0)

        # Act: market moves against trailing stop (should not adjust)
        for bid, ask in subsequent_ticks:
            quote = TestDataStubs.quote_tick(
                instrument=USDJPY_SIM,
                bid_price=bid,
                ask_price=ask,
            )
            self.exchange.process_quote_tick(quote)

        # Assert
        assert trailing_stop.trigger_price == expected_trigger
        assert trailing_stop.price == expected_price

    @pytest.mark.parametrize(
        (
            "order_side",
            "first_tick",
            "trade_price",
            "subsequent_ticks",
            "initial_price",
            "initial_trigger",
            "expected_trigger",
            "expected_price",
        ),
        [
            pytest.param(
                OrderSide.BUY,
                (13.0, 14.0),
                Price.from_str("13.000"),
                [(12.0, 13.0), (12.5, 13.5)],
                Price.from_str("15.000"),
                Price.from_str("15.000"),
                Price.from_str("13.020"),
                Price.from_str("13.020"),
                id="buy_last_ticks",
            ),
            pytest.param(
                OrderSide.SELL,
                (13.0, 14.0),
                Price.from_str("14.000"),
                [(14.0, 15.0), (13.5, 14.5)],
                Price.from_str("12.000"),
                Price.from_str("12.000"),
                Price.from_str("13.980"),
                Price.from_str("13.980"),
                id="sell_last_ticks",
            ),
        ],
    )
    def test_trailing_stop_limit_order_last_ticks_offset_activated_does_not_update(
        self,
        order_side: OrderSide,
        first_tick: tuple[float, float],
        trade_price: Price,
        subsequent_ticks: list[tuple[float, float]],
        initial_price: Price,
        initial_trigger: Price,
        expected_trigger: Price,
        expected_price: Price,
    ) -> None:
        # Arrange initial market tick
        bid0, ask0 = first_tick
        quote1 = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=bid0,
            ask_price=ask0,
        )
        self.exchange.process_quote_tick(quote1)
        self.data_engine.process(quote1)
        self.portfolio.update_quote_tick(quote1)

        # Process trade tick for LAST_PRICE
        trade = TradeTick(
            instrument_id=USDJPY_SIM.id,
            price=trade_price,
            size=Quantity.from_int(1),
            aggressor_side=(
                AggressorSide.SELLER if order_side == OrderSide.BUY else AggressorSide.BUYER
            ),
            trade_id=TradeId("123456"),
            ts_event=0,
            ts_init=0,
        )
        self.exchange.process_trade_tick(trade)
        self.data_engine.process(trade)

        # Prepare fixed offset for ticks
        offset = Decimal("20")
        # Submit trailing stop limit order
        trailing_stop = self.strategy.order_factory.trailing_stop_limit(
            instrument_id=USDJPY_SIM.id,
            order_side=order_side,
            quantity=Quantity.from_int(200_000),
            price=initial_price,
            trigger_price=initial_trigger,
            trailing_offset_type=TrailingOffsetType.TICKS,
            trailing_offset=offset,
            limit_offset=offset,
            trigger_type=TriggerType.LAST_PRICE,
        )
        self.strategy.submit_order(trailing_stop)
        self.exchange.process(0)

        # Act: market moves against trailing stop (should not adjust)
        for bid, ask in subsequent_ticks:
            quote = TestDataStubs.quote_tick(
                instrument=USDJPY_SIM,
                bid_price=bid,
                ask_price=ask,
            )
            self.exchange.process_quote_tick(quote)

        # Assert
        assert trailing_stop.trigger_price == expected_trigger
        assert trailing_stop.price == expected_price

    @pytest.mark.parametrize(
        (
            "order_side",
            "first_tick",
            "subsequent_ticks",
            "initial_price",
            "initial_trigger",
            "expected_trigger",
            "expected_price",
        ),
        [
            pytest.param(
                OrderSide.BUY,
                (13.0, 14.0),
                [(12.0, 13.0), (12.5, 13.5)],
                Price.from_str("15.000"),
                Price.from_str("15.000"),
                Price.from_str("13.020"),
                Price.from_str("13.020"),
                id="buy_bid_ask_ticks",
            ),
            pytest.param(
                OrderSide.SELL,
                (13.0, 14.0),
                [(14.0, 15.0), (13.5, 14.5)],
                Price.from_str("12.000"),
                Price.from_str("12.000"),
                Price.from_str("13.980"),
                Price.from_str("13.980"),
                id="sell_bid_ask_ticks",
            ),
        ],
    )
    def test_trailing_stop_limit_order_bid_ask_ticks_offset_activated_does_not_update(
        self,
        order_side: OrderSide,
        first_tick: tuple[float, float],
        subsequent_ticks: list[tuple[float, float]],
        initial_price: Price,
        initial_trigger: Price,
        expected_trigger: Price,
        expected_price: Price,
    ) -> None:
        # Arrange initial market tick
        bid0, ask0 = first_tick
        quote1 = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=bid0,
            ask_price=ask0,
        )
        self.exchange.process_quote_tick(quote1)
        self.data_engine.process(quote1)
        self.portfolio.update_quote_tick(quote1)

        # Prepare fixed offset for bid/ask ticks
        offset = Decimal("20")

        # Submit trailing stop limit order
        trailing_stop = self.strategy.order_factory.trailing_stop_limit(
            instrument_id=USDJPY_SIM.id,
            order_side=order_side,
            quantity=Quantity.from_int(200_000),
            price=initial_price,
            trigger_price=initial_trigger,
            trailing_offset_type=TrailingOffsetType.TICKS,
            trailing_offset=offset,
            limit_offset=offset,
            trigger_type=TriggerType.BID_ASK,
        )
        self.strategy.submit_order(trailing_stop)
        self.exchange.process(0)

        # Act: market moves against trailing stop (should not adjust)
        for bid, ask in subsequent_ticks:
            quote = TestDataStubs.quote_tick(
                instrument=USDJPY_SIM,
                bid_price=bid,
                ask_price=ask,
            )
            self.exchange.process_quote_tick(quote)

        # Assert
        assert trailing_stop.trigger_price == expected_trigger
        assert trailing_stop.price == expected_price

    def test_trailing_stop_market_order_buy_fill(
        self,
    ) -> None:
        # Arrange: Prepare market
        quote1 = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=13.0,
            ask_price=14.0,
        )
        self.exchange.process_quote_tick(quote1)
        self.data_engine.process(quote1)
        self.portfolio.update_quote_tick(quote1)

        trailing_stop = self.strategy.order_factory.trailing_stop_market(
            instrument_id=USDJPY_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(100_000),
            trigger_price=Price.from_str("15.000"),
            trailing_offset_type=TrailingOffsetType.TICKS,
            trailing_offset=Decimal("10"),
            trigger_type=TriggerType.BID_ASK,
        )
        self.strategy.submit_order(trailing_stop)
        self.exchange.process(0)

        # Act: market moves to fill order
        quote2 = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=16.0,
            ask_price=16.5,
            bid_size=100_000,
            ask_size=100_000,
        )
        self.exchange.process_quote_tick(quote2)

        # Assert
        assert trailing_stop.status == OrderStatus.FILLED
        assert trailing_stop.event_count == 4
        assert trailing_stop.events[-1].last_px == Price.from_str("15.000")
        assert trailing_stop.events[-1].last_qty == Quantity.from_int(100_000)
        assert trailing_stop.avg_px == Decimal("15")

    def test_trailing_stop_market_order_sell_fill(
        self,
    ) -> None:
        # Arrange: Prepare market
        quote1 = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=13.0,
            ask_price=14.0,
        )
        self.exchange.process_quote_tick(quote1)
        self.data_engine.process(quote1)
        self.portfolio.update_quote_tick(quote1)

        trailing_stop = self.strategy.order_factory.trailing_stop_market(
            instrument_id=USDJPY_SIM.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(100_000),
            trigger_price=Price.from_str("12.000"),
            trailing_offset_type=TrailingOffsetType.TICKS,
            trailing_offset=Decimal("10"),
            trigger_type=TriggerType.BID_ASK,
        )
        self.strategy.submit_order(trailing_stop)
        self.exchange.process(0)

        # Act: market moves to fill order
        quote2 = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=11.0,
            ask_price=11.5,
            bid_size=100_000,
            ask_size=100_000,
        )
        self.exchange.process_quote_tick(quote2)

        # Assert
        assert trailing_stop.status == OrderStatus.FILLED
        assert trailing_stop.event_count == 4
        assert trailing_stop.events[-1].last_px == Price.from_str("12.000")
        assert trailing_stop.events[-1].last_qty == Quantity.from_int(100_000)
        assert trailing_stop.avg_px == Decimal("12")

    def test_trailing_stop_market_order_buy_fill_when_quantity_exceeds_top_level(
        self,
    ) -> None:
        # Arrange: Prepare market
        quote1 = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=13.0,
            ask_price=14.0,
            bid_size=100_000,
            ask_size=100_000,
        )
        self.exchange.process_quote_tick(quote1)
        self.data_engine.process(quote1)
        self.portfolio.update_quote_tick(quote1)

        trailing_stop = self.strategy.order_factory.trailing_stop_market(
            instrument_id=USDJPY_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(200_000),  # <-- Exceeds top-level size
            trigger_price=Price.from_str("15.000"),
            trailing_offset_type=TrailingOffsetType.TICKS,
            trailing_offset=Decimal("10"),
            trigger_type=TriggerType.BID_ASK,
        )
        self.strategy.submit_order(trailing_stop)
        self.exchange.process(0)

        # Act: market moves to fill order
        quote2 = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=16.0,
            ask_price=16.5,
        )
        self.exchange.process_quote_tick(quote2)

        # Assert
        assert trailing_stop.status == OrderStatus.FILLED
        assert trailing_stop.event_count == 5
        assert trailing_stop.events[-2].last_px == Price.from_str("15.000")
        assert trailing_stop.events[-1].last_px == Price.from_str("15.001")  # <-- Slipped one tick
        assert trailing_stop.events[-2].last_qty == Quantity.from_int(100_000)
        assert trailing_stop.events[-1].last_qty == Quantity.from_int(100_000)

    def test_trailing_stop_market_order_sell_fill_when_quantity_exceeds_top_level(self) -> None:
        # Arrange: Prepare market
        quote1 = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=13.0,
            ask_price=14.0,
            bid_size=100_000,
            ask_size=100_000,
        )
        self.exchange.process_quote_tick(quote1)
        self.data_engine.process(quote1)
        self.portfolio.update_quote_tick(quote1)

        trailing_stop = self.strategy.order_factory.trailing_stop_market(
            instrument_id=USDJPY_SIM.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(200_000),  # <-- Exceeds top-level size
            trigger_price=Price.from_str("12.000"),
            trailing_offset_type=TrailingOffsetType.TICKS,
            trailing_offset=Decimal("10"),
            trigger_type=TriggerType.BID_ASK,
        )
        self.strategy.submit_order(trailing_stop)
        self.exchange.process(0)

        # Act: market moves to fill order
        quote2 = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=11.0,
            ask_price=11.5,
        )
        self.exchange.process_quote_tick(quote2)

        # Assert
        assert trailing_stop.status == OrderStatus.FILLED
        assert trailing_stop.event_count == 5
        assert trailing_stop.events[-2].last_px == Price.from_str("12.000")
        assert trailing_stop.events[-1].last_px == Price.from_str("11.999")  # <-- Slipped one tick
        assert trailing_stop.events[-2].last_qty == Quantity.from_int(100_000)
        assert trailing_stop.events[-1].last_qty == Quantity.from_int(100_000)

    def test_trailing_stop_limit_order_trail_activate_and_sell(self) -> None:
        # Arrange: Prepare market
        tick = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=13.0,
            ask_price=14.0,
        )
        self.exchange.process_quote_tick(tick)
        self.data_engine.process(tick)
        self.portfolio.update_quote_tick(tick)

        trailing_stop = self.strategy.order_factory.trailing_stop_limit(
            instrument_id=USDJPY_SIM.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(200_000),
            activation_price=Price.from_str("15.000"),
            trailing_offset_type=TrailingOffsetType.PRICE,
            trailing_offset=Decimal("1.0"),
            limit_offset=Decimal("1.0"),
            trigger_type=TriggerType.BID_ASK,
        )
        self.strategy.submit_order(trailing_stop)
        self.exchange.process(0)

        # When the activation_price is set higher than the current market price,
        # the order should remain inactive until the market reaches the activation price.
        assert not trailing_stop.is_activated
        assert trailing_stop.activation_price == Price.from_str("15.000")
        assert trailing_stop.trigger_price is None
        assert not trailing_stop.is_triggered

        tick = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=15.5,  # causes activation of the order
            ask_price=16.0,
        )
        self.exchange.process_quote_tick(tick)

        # When the market reaches the activation price,
        # the trigger_price should be set based on the given offset and continue to trail the market.
        assert trailing_stop.is_activated
        assert trailing_stop.activation_price == Price.from_str("15.000")
        assert trailing_stop.trigger_price == Price.from_str("14.500")
        assert not trailing_stop.is_triggered
        assert trailing_stop.price == Price.from_str("14.500")

        tick = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=16.5,  # raises trigger_price of the order
            ask_price=17.0,
        )
        self.exchange.process_quote_tick(tick)

        # When the market moves in a favorable direction,
        # the trigger_price should continue to adjust, trailing the market.
        assert trailing_stop.activation_price == Price.from_str("15.000")
        assert trailing_stop.trigger_price == Price.from_str("15.500")
        assert not trailing_stop.is_triggered
        assert trailing_stop.price == Price.from_str("15.500")

        tick = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=16.0,
            ask_price=16.5,
        )
        self.exchange.process_quote_tick(tick)

        # When the market moves in an unfavorable direction,
        # the trigger_price should remain unchanged until it is triggered.
        assert trailing_stop.trigger_price == Price.from_str("15.500")
        assert not trailing_stop.is_triggered
        assert trailing_stop.price == Price.from_str("15.500")

        tick = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=15.0,
            ask_price=15.5,
            bid_size=100_000,
            ask_size=200_000,
        )
        self.exchange.process_quote_tick(tick)

        # When the market reaches the trigger price, the order should be triggered,
        # but not filled because the order's limit price is higher than the bid price.
        assert trailing_stop.is_triggered
        assert trailing_stop.trigger_price == Price.from_str("15.500")
        assert trailing_stop.price == Price.from_str("15.500")
        assert trailing_stop.status == OrderStatus.TRIGGERED
        assert trailing_stop.filled_qty == 0

        tick = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=15.5,
            ask_price=16.0,
            bid_size=100_000,
            ask_size=200_000,
        )
        self.exchange.process_quote_tick(tick)

        # When the market bid price reaches the order's limit price,
        # the order should be filled up to the available quantity.
        assert trailing_stop.status == OrderStatus.PARTIALLY_FILLED
        assert trailing_stop.liquidity_side == LiquiditySide.MAKER
        assert trailing_stop.filled_qty == 100_000

    def test_trailing_stop_limit_order_trail_activate_and_buy(self) -> None:
        # Arrange: Prepare market
        quote1 = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=13.0,
            ask_price=14.0,
        )
        self.exchange.process_quote_tick(quote1)
        self.data_engine.process(quote1)
        self.portfolio.update_quote_tick(quote1)

        trailing_stop = self.strategy.order_factory.trailing_stop_limit(
            instrument_id=USDJPY_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(200_000),
            activation_price=Price.from_str("12.000"),
            trailing_offset_type=TrailingOffsetType.PRICE,
            trailing_offset=Decimal("1.0"),
            limit_offset=Decimal("1.0"),
            trigger_type=TriggerType.BID_ASK,
        )
        self.strategy.submit_order(trailing_stop)
        self.exchange.process(0)

        # When the activation_price is set higher than the current market price,
        # the order should remain inactive until the market reaches the activation price.
        assert not trailing_stop.is_activated
        assert trailing_stop.activation_price == Price.from_str("12.000")
        assert trailing_stop.trigger_price is None
        assert not trailing_stop.is_triggered

        quote2 = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=11.0,
            ask_price=11.5,  # causes activation of the order
        )
        self.exchange.process_quote_tick(quote2)

        # When the market reaches the activation price,
        # the trigger_price should be set based on the given offset and continue to trail the market.
        assert trailing_stop.is_activated
        assert trailing_stop.activation_price == Price.from_str("12.000")
        assert trailing_stop.trigger_price == Price.from_str("12.500")
        assert not trailing_stop.is_triggered
        assert trailing_stop.price == Price.from_str("12.500")

        quote3 = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=10.0,
            ask_price=10.5,  # lowers trigger_price of the order
        )
        self.exchange.process_quote_tick(quote3)

        # When the market moves down in a favorable direction,
        # the trigger_price should continue to adjust, trailing the market.
        assert trailing_stop.activation_price == Price.from_str("12.000")
        assert not trailing_stop.is_triggered
        assert trailing_stop.trigger_price == Price.from_str("11.500")
        assert trailing_stop.price == Price.from_str("11.500")

        quote4 = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=10.5,
            ask_price=11.0,
        )
        self.exchange.process_quote_tick(quote4)

        # When the market moves upward in an unfavorable direction,
        # the trigger_price should remain unchanged until it is triggered.
        assert trailing_stop.trigger_price == Price.from_str("11.500")
        assert not trailing_stop.is_triggered
        assert trailing_stop.price == Price.from_str("11.500")
        assert trailing_stop.price == Price.from_str("11.500")

        quote5 = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=11.0,
            ask_price=12.0,
            bid_size=100_000,
            ask_size=200_000,
        )
        self.exchange.process_quote_tick(quote5)

        # When the market moves over the trigger price, the order should be triggered,
        # but not filled because the order's limit price is lower than the ask price.
        assert trailing_stop.is_triggered
        assert trailing_stop.trigger_price == Price.from_str("11.500")
        assert trailing_stop.price == Price.from_str("11.500")
        assert trailing_stop.status == OrderStatus.TRIGGERED
        assert trailing_stop.filled_qty == 0

        quote6 = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=10.5,
            ask_price=11.5,
            bid_size=100_000,
            ask_size=200_000,
        )
        self.exchange.process_quote_tick(quote6)

        # When the market ask price moves down again and reaches the order's limit price,
        # the order should be filled up to the available quantity.
        assert trailing_stop.status == OrderStatus.FILLED
        assert trailing_stop.liquidity_side == LiquiditySide.MAKER
        assert trailing_stop.filled_qty == 200_000

    def test_trailing_stop_market_buy_order_modify(self) -> None:
        """
        Test various scenarios of modifying a buy-side trailing stop market order.
        """
        # Arrange: Prepare market
        quote1 = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=13.0,
            ask_price=14.0,
        )
        self.exchange.process_quote_tick(quote1)
        self.data_engine.process(quote1)
        self.portfolio.update_quote_tick(quote1)

        trailing_stop = self.strategy.order_factory.trailing_stop_market(
            instrument_id=USDJPY_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(200_000),
            activation_price=None,
            trailing_offset_type=TrailingOffsetType.PRICE,
            trailing_offset=Decimal("1.0"),
            trigger_type=TriggerType.BID_ASK,
        )
        self.strategy.submit_order(trailing_stop)
        self.exchange.process(0)

        # When activation_price is set to None, it defaults to the ask price for BUY orders,
        # and trigger_price is set by applying trailing_offset.
        assert trailing_stop.is_activated
        assert trailing_stop.activation_price == Price.from_str("14.000")
        assert trailing_stop.trigger_price == Price.from_str("15.000")
        assert trailing_stop.status == OrderStatus.ACCEPTED

        # Modify the order quantity
        new_quantity = Quantity.from_int(100_000)
        self.strategy.modify_order(trailing_stop, new_quantity)
        self.exchange.process(0)

        assert trailing_stop.quantity == new_quantity

        # Add quote to trigger and fill the order
        quote2 = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=14.0,
            ask_price=15.0,
            bid_size=200_000,
            ask_size=200_000,
        )
        self.exchange.process_quote_tick(quote2)

        # When the market ask price moves up and reaches the order's trigger price,
        # the order should be filled up to the modified quantity.
        assert trailing_stop.status == OrderStatus.FILLED
        assert trailing_stop.filled_qty == 100_000

    def test_trailing_stop_market_sell_order_modify(self) -> None:
        """
        Test various scenarios of modifying a sell-side trailing stop market order.
        """
        # Arrange: Prepare market
        quote1 = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=13.0,
            ask_price=14.0,
        )
        self.exchange.process_quote_tick(quote1)
        self.data_engine.process(quote1)
        self.portfolio.update_quote_tick(quote1)

        trailing_stop = self.strategy.order_factory.trailing_stop_market(
            instrument_id=USDJPY_SIM.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(200_000),
            activation_price=None,
            trailing_offset_type=TrailingOffsetType.PRICE,
            trailing_offset=Decimal("1.0"),
            trigger_type=TriggerType.BID_ASK,
        )
        self.strategy.submit_order(trailing_stop)
        self.exchange.process(0)

        # When activation_price is set to None, it defaults to the bid price for SELL orders,
        # and trigger_price is set by applying trailing_offset.
        assert trailing_stop.is_activated
        assert trailing_stop.activation_price == Price.from_str("13.000")
        assert trailing_stop.trigger_price == Price.from_str("12.000")
        assert trailing_stop.status == OrderStatus.ACCEPTED

        # Modify the order quantity
        new_quantity = Quantity.from_int(100_000)
        self.strategy.modify_order(trailing_stop, new_quantity)
        self.exchange.process(0)

        assert trailing_stop.quantity == new_quantity

        # Add quote to trigger and fill the order
        quote2 = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=12.0,
            ask_price=13.0,
            bid_size=200_000,
            ask_size=200_000,
        )
        self.exchange.process_quote_tick(quote2)

        # When the market bid price moves down and reaches the order's trigger price,
        # the order should be filled up to the modified quantity.
        assert trailing_stop.status == OrderStatus.FILLED
        assert trailing_stop.filled_qty == 100_000

    @pytest.mark.parametrize(
        ("bid_price", "ask_price", "expected_status", "expected_filled_qty"),
        [
            pytest.param(11.0, 12.0, OrderStatus.ACCEPTED, 0, id="at_activation_price"),
            pytest.param(15.0, 16.0, OrderStatus.TRIGGERED, 0, id="through_trigger_price"),
            pytest.param(14.0, 15.0, OrderStatus.FILLED, 100_000, id="at_trigger_price"),
        ],
    )
    def test_trailing_stop_limit_buy_order_modify(
        self,
        bid_price: float,
        ask_price: float,
        expected_status: OrderStatus,
        expected_filled_qty: int,
    ) -> None:
        # Arrange: Prepare market
        quote1 = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=13.0,
            ask_price=14.0,
            bid_size=200_000,
            ask_size=200_000,
        )
        self.exchange.process_quote_tick(quote1)
        self.data_engine.process(quote1)
        self.portfolio.update_quote_tick(quote1)

        trailing_stop = self.strategy.order_factory.trailing_stop_limit(
            instrument_id=USDJPY_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(200_000),
            activation_price=None,
            trailing_offset_type=TrailingOffsetType.PRICE,
            trailing_offset=Decimal("1.0"),
            limit_offset=Decimal("1.0"),
            trigger_type=TriggerType.BID_ASK,
        )
        self.strategy.submit_order(trailing_stop)
        self.exchange.process(0)

        # Assert initial activation
        assert trailing_stop.is_activated
        assert trailing_stop.activation_price == Price.from_str("14.000")
        assert trailing_stop.trigger_price == Price.from_str("15.000")
        assert trailing_stop.status == OrderStatus.ACCEPTED

        # Modify the order quantity
        new_quantity = Quantity.from_int(100_000)
        self.strategy.modify_order(trailing_stop, new_quantity)
        self.exchange.process(0)
        assert trailing_stop.quantity == new_quantity

        # Act: market moves for parameterized tick
        quote2 = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=bid_price,
            ask_price=ask_price,
            bid_size=200_000,
            ask_size=200_000,
        )
        self.exchange.process_quote_tick(quote2)

        # Assert
        assert trailing_stop.status == expected_status
        assert trailing_stop.filled_qty == expected_filled_qty

    def test_modify_unactivated_trailing_stop_order_before_activation(self) -> None:
        # Arrange: seed simple market quote so order activation will succeed if triggered
        quote = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=12.0,
            ask_price=13.0,
        )
        self.exchange.process_quote_tick(quote)
        self.data_engine.process(quote)
        self.portfolio.update_quote_tick(quote)

        # Submit a trailing-stop market order without activation
        trailing_stop = self.strategy.order_factory.trailing_stop_market(
            instrument_id=USDJPY_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(100_000),
            trailing_offset=Decimal("1.0"),
            trailing_offset_type=TrailingOffsetType.PRICE,
            trigger_type=TriggerType.BID_ASK,
        )
        self.strategy.submit_order(trailing_stop)

        # Immediately modify before activation occurs
        new_qty = Quantity.from_int(50_000)
        self.strategy.modify_order(trailing_stop, new_qty)

        # Process without errors and update quantity
        self.exchange.process(0)
        assert trailing_stop.quantity == new_qty

        # Engine auto-activates trailing stops on submit when activation_price unset
        assert trailing_stop.is_activated
        assert trailing_stop.trigger_price == Price.from_str("14.000")

    @pytest.mark.parametrize(
        ("bid_price", "ask_price", "expected_status", "expected_filled_qty"),
        [
            pytest.param(14.0, 15.0, OrderStatus.ACCEPTED, 0, id="at_activation_price"),
            pytest.param(11.0, 12.0, OrderStatus.TRIGGERED, 0, id="through_trigger_price"),
            pytest.param(12.0, 13.0, OrderStatus.FILLED, 100_000, id="at_trigger_price"),
        ],
    )
    def test_trailing_stop_limit_sell_order_modify(
        self,
        bid_price: float,
        ask_price: float,
        expected_status: OrderStatus,
        expected_filled_qty: int,
    ) -> None:
        """
        Test various scenarios of modifying a sell-side trailing stop limit order.
        """
        # Arrange: Prepare market
        quote1 = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=13.0,
            ask_price=14.0,
        )
        self.exchange.process_quote_tick(quote1)
        self.data_engine.process(quote1)
        self.portfolio.update_quote_tick(quote1)

        trailing_stop = self.strategy.order_factory.trailing_stop_limit(
            instrument_id=USDJPY_SIM.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(200_000),
            activation_price=None,
            trailing_offset_type=TrailingOffsetType.PRICE,
            trailing_offset=Decimal("1.0"),
            limit_offset=Decimal("1.0"),
            trigger_type=TriggerType.BID_ASK,
        )
        self.strategy.submit_order(trailing_stop)
        self.exchange.process(0)

        # When activation_price is set to None, it defaults to the bid price for SELL orders,
        # and trigger_price is set by applying trailing_offset.
        assert trailing_stop.is_activated
        assert trailing_stop.activation_price == Price.from_str("13.000")
        assert trailing_stop.trigger_price == Price.from_str("12.000")
        assert not trailing_stop.is_triggered
        assert trailing_stop.status == OrderStatus.ACCEPTED

        # Modify the order quantity
        new_quantity = Quantity.from_int(100_000)
        self.strategy.modify_order(trailing_stop, new_quantity)
        self.exchange.process(0)

        assert trailing_stop.quantity == new_quantity

        # Act: market moves according to parameterized tick
        quote2 = TestDataStubs.quote_tick(
            instrument=USDJPY_SIM,
            bid_price=bid_price,
            ask_price=ask_price,
            bid_size=200_000,
            ask_size=200_000,
        )
        self.exchange.process_quote_tick(quote2)

        # Assert
        assert trailing_stop.status == expected_status
        assert trailing_stop.filled_qty == expected_filled_qty
