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

from nautilus_trader.backtest.exchange import SimulatedExchange
from nautilus_trader.backtest.execution_client import BacktestExecClient
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.backtest.models import LatencyModel
from nautilus_trader.backtest.models import MakerTakerFeeModel
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.component import TestClock
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.execution.emulator import OrderEmulator
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.model.currencies import ETH
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.risk.engine import RiskEngine
from nautilus_trader.test_kit.mocks.strategies import MockStrategy
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


BINANCE = Venue("BINANCE")
ETHUSDT_PERP_BINANCE = TestInstrumentProvider.ethusdt_perp_binance()


class TestSimulatedExchangeEmulatedContingencyOrders:
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
            clock=self.clock,
            cache=self.cache,
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

        self.emulator = OrderEmulator(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.exchange = SimulatedExchange(
            venue=BINANCE,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            base_currency=None,  # Multi-asset wallet
            starting_balances=[Money(200, ETH), Money(1_000_000, USDT)],
            default_leverage=Decimal(10),
            leverages={},
            modules=[],
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            latency_model=LatencyModel(0),
            reject_stop_orders=False,
        )
        self.exchange.add_instrument(ETHUSDT_PERP_BINANCE)

        self.exec_client = BacktestExecClient(
            exchange=self.exchange,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Wire up components
        self.exec_engine.register_client(self.exec_client)
        self.exchange.register_client(self.exec_client)

        self.cache.add_instrument(ETHUSDT_PERP_BINANCE)

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

    @pytest.mark.parametrize(
        ("emulation_trigger", "order_side", "sl_trigger_price", "tp_price"),
        [
            [
                TriggerType.NO_TRIGGER,
                OrderSide.BUY,
                3050.00,
                3150.00,
            ],
            [
                TriggerType.BID_ASK,
                OrderSide.BUY,
                3050.00,
                3150.00,
            ],
            [
                TriggerType.NO_TRIGGER,
                OrderSide.SELL,
                3150.00,
                3050.00,
            ],
            [
                TriggerType.BID_ASK,
                OrderSide.SELL,
                3150.00,
                3050.00,
            ],
        ],
    )
    def test_bracket_market_entry_accepts_sl_and_tp(
        self,
        emulation_trigger: TriggerType,
        order_side: OrderSide,
        sl_trigger_price: Price,
        tp_price: Price,
    ) -> None:
        # Arrange: Prepare market
        tick = TestDataStubs.quote_tick(
            instrument=ETHUSDT_PERP_BINANCE,
            bid_price=3100.00,
            ask_price=3100.00,
        )
        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=order_side,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10.000),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(sl_trigger_price),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(tp_price),
            emulation_trigger=emulation_trigger,
        )

        # Act
        self.strategy.submit_order_list(bracket)
        self.exchange.process(0)

        # Assert
        entry_order = bracket.orders[0]
        sl_order = bracket.orders[1]
        tp_order = bracket.orders[2]
        assert entry_order.status == OrderStatus.FILLED
        assert (
            sl_order.status == OrderStatus.EMULATED
            if sl_order.is_emulated
            else OrderStatus.ACCEPTED
        )
        assert (
            tp_order.status == OrderStatus.EMULATED
            if tp_order.is_emulated
            else OrderStatus.ACCEPTED
        )

    @pytest.mark.parametrize(
        (
            "emulation_trigger",
            "order_side",
            "entry_trigger_price",
            "sl_trigger_price",
            "tp_trigger_price",
            "next_tick_price",
            "expected_bracket_status",
        ),
        [
            [
                TriggerType.NO_TRIGGER,
                OrderSide.BUY,
                3090.00,  # entry_trigger_price
                3050.00,  # sl_trigger_price
                3150.00,  # tp_price
                3090.00,  # next_tick_price (hits trigger)
                OrderStatus.ACCEPTED,
            ],
            [
                TriggerType.BID_ASK,
                OrderSide.BUY,
                3090.00,  # entry_trigger_price
                3050.00,  # sl_trigger_price
                3150.00,  # tp_price
                3090.00,  # next_tick_price (hits trigger)
                OrderStatus.EMULATED,
            ],
            [
                TriggerType.NO_TRIGGER,
                OrderSide.SELL,
                3110.00,  # entry_trigger_price,
                3150.00,  # sl_trigger_price
                3050.00,  # tp_price
                3110.00,  # next_tick_price (hits trigger)
                OrderStatus.ACCEPTED,
            ],
            [
                TriggerType.BID_ASK,
                OrderSide.SELL,
                3110.00,  # entry_trigger_price,
                3150.00,  # sl_trigger_price
                3050.00,  # tp_price
                3110.00,  # next_tick_price (hits trigger)
                OrderStatus.EMULATED,
            ],
        ],
    )
    def test_bracket_market_if_touched_entry_triggers_passively_then_sl_tp_working(
        self,
        emulation_trigger: TriggerType,
        order_side: OrderSide,
        entry_trigger_price: Price,
        sl_trigger_price: Price,
        tp_trigger_price: Price,
        next_tick_price: Price,
        expected_bracket_status: OrderStatus,
    ) -> None:
        # Arrange: Prepare market
        tick1 = QuoteTick(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            bid_price=ETHUSDT_PERP_BINANCE.make_price(3100.0),
            ask_price=ETHUSDT_PERP_BINANCE.make_price(3100.0),
            bid_size=ETHUSDT_PERP_BINANCE.make_qty(10.000),
            ask_size=ETHUSDT_PERP_BINANCE.make_qty(10.000),
            ts_event=0,
            ts_init=0,
        )
        self.data_engine.process(tick1)
        self.exchange.process_quote_tick(tick1)

        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=order_side,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10.000),
            entry_trigger_price=ETHUSDT_PERP_BINANCE.make_price(entry_trigger_price),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(sl_trigger_price),
            tp_trigger_price=ETHUSDT_PERP_BINANCE.make_price(tp_trigger_price),
            entry_order_type=OrderType.MARKET_IF_TOUCHED,
            tp_order_type=OrderType.MARKET_IF_TOUCHED,
            emulation_trigger=emulation_trigger,
        )

        self.strategy.submit_order_list(bracket)
        self.exchange.process(0)

        # Act
        tick2 = TestDataStubs.quote_tick(
            instrument=ETHUSDT_PERP_BINANCE,
            bid_price=next_tick_price,
            ask_price=next_tick_price,
        )
        self.data_engine.process(tick2)
        self.exchange.process_quote_tick(tick2)
        self.emulator.on_quote_tick(tick2)
        self.exchange.process(tick2.ts_init)

        # Assert
        entry_order = self.cache.order(bracket.orders[0].client_order_id)
        sl_order = self.cache.order(bracket.orders[1].client_order_id)
        tp_order = self.cache.order(bracket.orders[2].client_order_id)
        assert entry_order.status == OrderStatus.FILLED
        assert sl_order.status == expected_bracket_status
        assert tp_order.status == expected_bracket_status

    @pytest.mark.parametrize(
        (
            "emulation_trigger",
            "order_side",
            "entry_trigger_price",
            "entry_price",
            "sl_trigger_price",
            "tp_price",
            "next_tick_price",
        ),
        [
            [
                TriggerType.NO_TRIGGER,
                OrderSide.BUY,
                3090.00,  # entry_trigger_price
                3089.00,  # entry_price
                3050.00,  # sl_trigger_price
                3150.00,  # tp_price
                3090.00,  # next_tick_price (hits trigger)
            ],
            [
                TriggerType.BID_ASK,
                OrderSide.BUY,
                3090.00,  # entry_trigger_price
                3089.00,  # entry_price
                3050.00,  # sl_trigger_price
                3150.00,  # tp_price
                3090.00,  # next_tick_price (hits trigger)
            ],
            [
                TriggerType.NO_TRIGGER,
                OrderSide.SELL,
                3110.00,  # entry_trigger_price,
                3111.00,  # entry_price
                3150.00,  # sl_trigger_price
                3050.00,  # tp_price
                3110.00,  # next_tick_price (hits trigger)
            ],
            [
                TriggerType.BID_ASK,
                OrderSide.SELL,
                3110.00,  # entry_trigger_price,
                3111.00,  # entry_price
                3150.00,  # sl_trigger_price
                3050.00,  # tp_price
                3110.00,  # next_tick_price (hits trigger)
            ],
        ],
    )
    def test_bracket_limit_if_touched_entry_triggers_passively_then_limit_working(
        self,
        emulation_trigger: TriggerType,
        order_side: OrderSide,
        entry_trigger_price: Price,
        entry_price: Price,
        sl_trigger_price: Price,
        tp_price: Price,
        next_tick_price: Price,
    ) -> None:
        # Arrange: Prepare market
        tick1 = QuoteTick(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            bid_price=ETHUSDT_PERP_BINANCE.make_price(3100.0),
            ask_price=ETHUSDT_PERP_BINANCE.make_price(3100.0),
            bid_size=ETHUSDT_PERP_BINANCE.make_qty(10.000),
            ask_size=ETHUSDT_PERP_BINANCE.make_qty(10.000),
            ts_event=0,
            ts_init=0,
        )
        self.data_engine.process(tick1)
        self.exchange.process_quote_tick(tick1)

        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=order_side,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10.000),
            entry_trigger_price=ETHUSDT_PERP_BINANCE.make_price(entry_trigger_price),
            entry_price=ETHUSDT_PERP_BINANCE.make_price(entry_price),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(sl_trigger_price),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(tp_price),
            entry_order_type=OrderType.LIMIT_IF_TOUCHED,
            emulation_trigger=emulation_trigger,
        )

        self.strategy.submit_order_list(bracket)
        self.exchange.process(0)

        # Act
        tick2 = TestDataStubs.quote_tick(
            instrument=ETHUSDT_PERP_BINANCE,
            bid_price=next_tick_price,
            ask_price=next_tick_price,
        )
        self.data_engine.process(tick2)
        self.exchange.process_quote_tick(tick2)
        self.emulator.on_quote_tick(tick2)
        self.exchange.process(tick2.ts_init)

        # Assert
        entry_order = self.cache.order(bracket.orders[0].client_order_id)
        sl_order = self.cache.order(bracket.orders[1].client_order_id)
        tp_order = self.cache.order(bracket.orders[2].client_order_id)
        assert (
            entry_order.status == OrderStatus.ACCEPTED
            if sl_order.is_emulated
            else OrderStatus.TRIGGERED
        )
        assert (
            sl_order.status == OrderStatus.EMULATED
            if sl_order.is_emulated
            else OrderStatus.ACCEPTED
        )
        assert (
            tp_order.status == OrderStatus.EMULATED
            if tp_order.is_emulated
            else OrderStatus.ACCEPTED
        )

    @pytest.mark.parametrize(
        (
            "emulation_trigger",
            "order_side",
            "entry_trigger_price",
            "entry_price",
            "sl_trigger_price",
            "tp_price",
            "next_tick_price",
        ),
        [
            [
                TriggerType.NO_TRIGGER,
                OrderSide.BUY,
                3099.00,  # entry_trigger_price
                3095.00,  # entry_price
                3050.00,  # sl_trigger_price
                3150.00,  # tp_price
                3094.00,  # next_tick_price (moves through limit price)
            ],
            [
                TriggerType.BID_ASK,
                OrderSide.BUY,
                3099.00,  # entry_trigger_price
                3095.00,  # entry_price
                3050.00,  # sl_trigger_price
                3150.00,  # tp_price
                3094.00,  # next_tick_price (moves through limit price)
            ],
            [
                TriggerType.NO_TRIGGER,
                OrderSide.SELL,
                3101.00,  # entry_trigger_price
                3105.00,  # entry price
                3150.00,  # sl_trigger_price
                3050.00,  # tp_price
                3106.00,  # next_tick_price (moves through limit price)
            ],
            [
                TriggerType.BID_ASK,
                OrderSide.SELL,
                3101.00,  # entry_trigger_price
                3105.00,  # entry price
                3150.00,  # sl_trigger_price
                3050.00,  # tp_price
                3106.00,  # next_tick_price (moves through limit price)
            ],
        ],
    )
    def test_bracket_limit_if_touched_entry_triggers_passively_and_fills_passively(
        self,
        emulation_trigger: TriggerType,
        order_side: OrderSide,
        entry_trigger_price: Price,
        entry_price: Price,
        sl_trigger_price: Price,
        tp_price: Price,
        next_tick_price: Price,
    ) -> None:
        # Arrange: Prepare market
        tick1 = QuoteTick(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            bid_price=ETHUSDT_PERP_BINANCE.make_price(3100.0),
            ask_price=ETHUSDT_PERP_BINANCE.make_price(3100.0),
            bid_size=ETHUSDT_PERP_BINANCE.make_qty(10.000),
            ask_size=ETHUSDT_PERP_BINANCE.make_qty(10.000),
            ts_event=0,
            ts_init=0,
        )
        self.data_engine.process(tick1)
        self.exchange.process_quote_tick(tick1)

        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=order_side,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10.000),
            entry_trigger_price=ETHUSDT_PERP_BINANCE.make_price(entry_trigger_price),
            entry_price=ETHUSDT_PERP_BINANCE.make_price(entry_price),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(sl_trigger_price),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(tp_price),
            entry_order_type=OrderType.LIMIT_IF_TOUCHED,
            emulation_trigger=emulation_trigger,
        )

        self.strategy.submit_order_list(bracket)
        self.exchange.process(0)

        # Act
        tick2 = TestDataStubs.quote_tick(
            instrument=ETHUSDT_PERP_BINANCE,
            bid_price=next_tick_price,
            ask_price=next_tick_price,
        )
        self.data_engine.process(tick2)
        self.exchange.process_quote_tick(tick2)
        self.emulator.on_quote_tick(tick2)
        self.exchange.process(0)

        # Assert
        entry_order = self.cache.order(bracket.orders[0].client_order_id)
        sl_order = self.cache.order(bracket.orders[1].client_order_id)
        tp_order = self.cache.order(bracket.orders[2].client_order_id)
        assert entry_order.status == OrderStatus.FILLED
        assert entry_order.avg_px == entry_order.price  # <-- fills at limit price
        assert (
            sl_order.status == OrderStatus.EMULATED
            if sl_order.is_emulated
            else OrderStatus.ACCEPTED
        )
        assert (
            tp_order.status == OrderStatus.EMULATED
            if tp_order.is_emulated
            else OrderStatus.ACCEPTED
        )

    @pytest.mark.parametrize(
        (
            "emulation_trigger",
            "order_side",
            "entry_trigger_price",
            "entry_price",
            "sl_trigger_price",
            "tp_price",
            "next_tick_price",
        ),
        [
            [
                TriggerType.NO_TRIGGER,
                OrderSide.BUY,
                3099.00,  # entry_trigger_price
                3100.00,  # entry_price
                3050.00,  # sl_trigger_price
                3150.00,  # tp_price
                3098.00,  # next_tick_price (moves through trigger price)
            ],
            [
                TriggerType.BID_ASK,
                OrderSide.BUY,
                3099.00,  # entry_trigger_price
                3100.00,  # entry_price
                3050.00,  # sl_trigger_price
                3150.00,  # tp_price
                3098.00,  # next_tick_price (moves through trigger price)
            ],
            [
                TriggerType.NO_TRIGGER,
                OrderSide.SELL,
                3101.00,  # entry_trigger_price
                3099.00,  # entry price
                3150.00,  # sl_trigger_price
                3050.00,  # tp_price
                3102.00,  # next_tick_price (moves through trigger price)
            ],
            [
                TriggerType.BID_ASK,
                OrderSide.SELL,
                3101.00,  # entry_trigger_price
                3099.00,  # entry price
                3150.00,  # sl_trigger_price
                3050.00,  # tp_price
                3102.00,  # next_tick_price (moves through trigger price)
            ],
        ],
    )
    def test_bracket_limit_if_touched_entry_triggers_passively_and_fills_immediately(
        self,
        emulation_trigger: TriggerType,
        order_side: OrderSide,
        entry_trigger_price: Price,
        entry_price: Price,
        sl_trigger_price: Price,
        tp_price: Price,
        next_tick_price: Price,
    ) -> None:
        # Arrange: Prepare market
        tick1 = QuoteTick(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            bid_price=ETHUSDT_PERP_BINANCE.make_price(3100.0),
            ask_price=ETHUSDT_PERP_BINANCE.make_price(3100.0),
            bid_size=ETHUSDT_PERP_BINANCE.make_qty(10.000),
            ask_size=ETHUSDT_PERP_BINANCE.make_qty(10.000),
            ts_event=0,
            ts_init=0,
        )
        self.data_engine.process(tick1)
        self.exchange.process_quote_tick(tick1)

        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=order_side,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10.000),
            entry_trigger_price=ETHUSDT_PERP_BINANCE.make_price(entry_trigger_price),
            entry_price=ETHUSDT_PERP_BINANCE.make_price(entry_price),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(sl_trigger_price),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(tp_price),
            entry_order_type=OrderType.LIMIT_IF_TOUCHED,
            emulation_trigger=emulation_trigger,
        )

        self.strategy.submit_order_list(bracket)
        self.exchange.process(0)

        # Act
        tick2 = TestDataStubs.quote_tick(
            instrument=ETHUSDT_PERP_BINANCE,
            bid_price=next_tick_price,
            ask_price=next_tick_price,
        )
        self.data_engine.process(tick2)
        self.exchange.process_quote_tick(tick2)
        self.emulator.on_quote_tick(tick2)
        self.exchange.process(0)

        # Assert
        entry_order = self.cache.order(bracket.orders[0].client_order_id)
        sl_order = self.cache.order(bracket.orders[1].client_order_id)
        tp_order = self.cache.order(bracket.orders[2].client_order_id)
        assert entry_order.status == OrderStatus.FILLED
        assert entry_order.avg_px == entry_trigger_price  # <-- fills where market is at trigger
        assert sl_order.status in (OrderStatus.ACCEPTED, OrderStatus.EMULATED)
        assert tp_order.status in (OrderStatus.ACCEPTED, OrderStatus.EMULATED)

    @pytest.mark.parametrize(
        (
            "emulation_trigger",
            "order_side",
            "entry_trigger_price",
            "entry_price",
            "sl_trigger_price",
            "tp_price",
            "next_tick_price",
            "expected_bracket_status",
        ),
        [
            [
                TriggerType.NO_TRIGGER,
                OrderSide.BUY,
                3100.00,  # entry_trigger_price (triggers immediately)
                3095.00,  # entry_price
                3050.00,  # sl_trigger_price
                3150.00,  # tp_price
                3094.00,  # next_tick_price (moves through limit price)
                OrderStatus.ACCEPTED,
            ],
            [
                TriggerType.BID_ASK,
                OrderSide.BUY,
                3100.00,  # entry_trigger_price (triggers immediately)
                3095.00,  # entry_price
                3050.00,  # sl_trigger_price
                3150.00,  # tp_price
                3094.00,  # next_tick_price (moves through limit price)
                OrderStatus.EMULATED,
            ],
            [
                TriggerType.NO_TRIGGER,
                OrderSide.SELL,
                3100.00,  # entry_trigger_price (triggers immediately)
                3105.00,  # entry price
                3150.00,  # sl_trigger_price
                3050.00,  # tp_price
                3106.00,  # next_tick_price (moves through limit price)
                OrderStatus.ACCEPTED,
            ],
            [
                TriggerType.BID_ASK,
                OrderSide.SELL,
                3100.00,  # entry_trigger_price (triggers immediately)
                3105.00,  # entry price
                3150.00,  # sl_trigger_price
                3050.00,  # tp_price
                3106.00,  # next_tick_price (moves through limit price)
                OrderStatus.EMULATED,
            ],
        ],
    )
    def test_bracket_limit_if_touched_entry_triggers_immediately_and_fills_passively(
        self,
        emulation_trigger: TriggerType,
        order_side: OrderSide,
        entry_trigger_price: Price,
        entry_price: Price,
        sl_trigger_price: Price,
        tp_price: Price,
        next_tick_price: Price,
        expected_bracket_status: OrderStatus,
    ) -> None:
        # Arrange: Prepare market
        tick1 = QuoteTick(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            bid_price=ETHUSDT_PERP_BINANCE.make_price(3100.0),
            ask_price=ETHUSDT_PERP_BINANCE.make_price(3100.0),
            bid_size=ETHUSDT_PERP_BINANCE.make_qty(10.000),
            ask_size=ETHUSDT_PERP_BINANCE.make_qty(10.000),
            ts_event=0,
            ts_init=0,
        )
        self.data_engine.process(tick1)
        self.exchange.process_quote_tick(tick1)

        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=order_side,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10.000),
            entry_trigger_price=ETHUSDT_PERP_BINANCE.make_price(entry_trigger_price),
            entry_price=ETHUSDT_PERP_BINANCE.make_price(entry_price),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(sl_trigger_price),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(tp_price),
            entry_order_type=OrderType.LIMIT_IF_TOUCHED,
            emulation_trigger=emulation_trigger,
        )

        self.strategy.submit_order_list(bracket)
        self.exchange.process(0)

        # Act
        tick2 = TestDataStubs.quote_tick(
            instrument=ETHUSDT_PERP_BINANCE,
            bid_price=next_tick_price,
            ask_price=next_tick_price,
        )
        self.data_engine.process(tick2)
        self.exchange.process_quote_tick(tick2)
        self.emulator.on_quote_tick(tick2)
        self.exchange.process(0)

        # Assert
        entry_order = self.cache.order(bracket.orders[0].client_order_id)
        sl_order = self.cache.order(bracket.orders[1].client_order_id)
        tp_order = self.cache.order(bracket.orders[2].client_order_id)
        assert entry_order.status == OrderStatus.FILLED
        assert entry_order.avg_px == entry_order.price  # <-- fills at limit price
        assert sl_order.status == expected_bracket_status
        assert tp_order.status == expected_bracket_status

    @pytest.mark.parametrize(
        (
            "emulation_trigger",
            "order_side",
            "entry_trigger_price",
            "entry_price",
            "sl_trigger_price",
            "tp_price",
            "expected_bracket_status",
        ),
        [
            [
                TriggerType.NO_TRIGGER,
                OrderSide.BUY,
                3102.00,  # entry_trigger_price (triggers immediately)
                3103.00,  # entry_price
                3050.00,  # sl_trigger_price
                3150.00,  # tp_price
                OrderStatus.ACCEPTED,
            ],
            [
                TriggerType.BID_ASK,
                OrderSide.BUY,
                3102.00,  # entry_trigger_price (triggers immediately)
                3103.00,  # entry_price
                3050.00,  # sl_trigger_price
                3150.00,  # tp_price
                OrderStatus.EMULATED,
            ],
            [
                TriggerType.NO_TRIGGER,
                OrderSide.SELL,
                3098.00,  # entry_trigger_price (triggers immediately)
                3097.00,  # entry_price
                3150.00,  # sl_trigger_price
                3050.00,  # tp_price
                OrderStatus.ACCEPTED,
            ],
            [
                TriggerType.BID_ASK,
                OrderSide.SELL,
                3098.00,  # entry_trigger_price (triggers immediately)
                3097.00,  # entry_price
                3150.00,  # sl_trigger_price
                3050.00,  # tp_price
                OrderStatus.EMULATED,
            ],
        ],
    )
    def test_bracket_limit_if_touched_entry_triggers_immediately_and_fills_immediately(
        self,
        emulation_trigger: TriggerType,
        order_side: OrderSide,
        entry_trigger_price: Price,
        entry_price: Price,
        sl_trigger_price: Price,
        tp_price: Price,
        expected_bracket_status: OrderStatus,
    ) -> None:
        # Arrange: Prepare market
        self.emulator.create_matching_core(
            ETHUSDT_PERP_BINANCE.id,
            ETHUSDT_PERP_BINANCE.price_increment,
        )

        tick1 = QuoteTick(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            bid_price=ETHUSDT_PERP_BINANCE.make_price(3100.0),
            ask_price=ETHUSDT_PERP_BINANCE.make_price(3100.0),
            bid_size=ETHUSDT_PERP_BINANCE.make_qty(10.000),
            ask_size=ETHUSDT_PERP_BINANCE.make_qty(10.000),
            ts_event=0,
            ts_init=0,
        )
        self.data_engine.process(tick1)
        self.exchange.process_quote_tick(tick1)
        self.emulator.on_quote_tick(tick1)

        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=order_side,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10.000),
            entry_trigger_price=ETHUSDT_PERP_BINANCE.make_price(entry_trigger_price),
            entry_price=ETHUSDT_PERP_BINANCE.make_price(entry_price),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(sl_trigger_price),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(tp_price),
            entry_order_type=OrderType.LIMIT_IF_TOUCHED,
            emulation_trigger=emulation_trigger,
        )

        # Act
        self.strategy.submit_order_list(bracket)
        self.exchange.process(0)

        # Assert
        entry_order = self.cache.order(bracket.orders[0].client_order_id)
        sl_order = self.cache.order(bracket.orders[1].client_order_id)
        tp_order = self.cache.order(bracket.orders[2].client_order_id)
        assert entry_order.status == OrderStatus.FILLED
        assert entry_order.avg_px == 3100.00  # <-- fills where market is
        assert sl_order.status == expected_bracket_status
        assert tp_order.status == expected_bracket_status
