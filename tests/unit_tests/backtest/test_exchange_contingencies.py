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

from nautilus_trader.backtest.exchange import SimulatedExchange
from nautilus_trader.backtest.execution_client import BacktestExecClient
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.backtest.models import LatencyModel
from nautilus_trader.backtest.models import MakerTakerFeeModel
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.component import TestClock
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.model.currencies import ETH
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
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


BINANCE = Venue("BINANCE")
ETHUSDT_PERP_BINANCE = TestInstrumentProvider.ethusdt_perp_binance()


class TestSimulatedExchangeContingencyAdvancedOrders:
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

    def test_submit_bracket_market_entry_buy_accepts_sl_and_tp(self):
        # Arrange: Prepare market
        tick = QuoteTick(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            bid_price=ETHUSDT_PERP_BINANCE.make_price(3090.2),
            ask_price=ETHUSDT_PERP_BINANCE.make_price(3090.5),
            bid_size=ETHUSDT_PERP_BINANCE.make_qty(15.100),
            ask_size=ETHUSDT_PERP_BINANCE.make_qty(15.100),
            ts_event=0,
            ts_init=0,
        )

        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10.000),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(3050.0),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(3150.0),
        )

        # Act
        self.strategy.submit_order_list(bracket)
        self.exchange.process(0)

        # Assert
        assert bracket.orders[0].status == OrderStatus.FILLED
        assert bracket.orders[1].status == OrderStatus.ACCEPTED
        assert bracket.orders[2].status == OrderStatus.ACCEPTED

    def test_submit_bracket_market_entry_sell_accepts_sl_and_tp(self):
        # Arrange: Prepare market
        tick = QuoteTick(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            bid_price=ETHUSDT_PERP_BINANCE.make_price(3090.2),
            ask_price=ETHUSDT_PERP_BINANCE.make_price(3090.5),
            bid_size=ETHUSDT_PERP_BINANCE.make_qty(15.100),
            ask_size=ETHUSDT_PERP_BINANCE.make_qty(15.100),
            ts_event=0,
            ts_init=0,
        )

        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.SELL,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10.000),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(3150.0),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(3050.0),
        )

        # Act
        self.strategy.submit_order_list(bracket)
        self.exchange.process(0)

        # Assert
        assert bracket.orders[0].status == OrderStatus.FILLED
        assert bracket.orders[1].status == OrderStatus.ACCEPTED
        assert bracket.orders[2].status == OrderStatus.ACCEPTED

    def test_submit_bracket_market_entry_with_immediate_modify_accepts_sl_and_tp(self):
        # Arrange: Prepare market
        tick = QuoteTick(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            bid_price=ETHUSDT_PERP_BINANCE.make_price(3090.2),
            ask_price=ETHUSDT_PERP_BINANCE.make_price(3090.5),
            bid_size=ETHUSDT_PERP_BINANCE.make_qty(15.100),
            ask_size=ETHUSDT_PERP_BINANCE.make_qty(15.100),
            ts_event=0,
            ts_init=0,
        )

        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.SELL,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10.000),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(3150.0),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(3050.0),
        )

        sl_order = bracket.orders[1]
        tp_order = bracket.orders[2]

        new_sl_trigger_price = ETHUSDT_PERP_BINANCE.make_price(3151.0)
        new_tp_price = ETHUSDT_PERP_BINANCE.make_price(3051.0)

        # Act
        self.strategy.submit_order_list(bracket)
        self.strategy.modify_order(sl_order, trigger_price=new_sl_trigger_price)
        self.strategy.modify_order(tp_order, price=new_tp_price)
        self.exchange.process(0)

        # Assert
        assert bracket.orders[0].status == OrderStatus.FILLED
        assert sl_order.status == OrderStatus.ACCEPTED
        assert tp_order.status == OrderStatus.ACCEPTED
        assert sl_order.trigger_price == new_sl_trigger_price
        assert tp_order.price == new_tp_price

    def test_submit_bracket_market_entry_with_immediate_cancel(self):
        # Arrange: Prepare market
        tick = QuoteTick(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            bid_price=ETHUSDT_PERP_BINANCE.make_price(3090.2),
            ask_price=ETHUSDT_PERP_BINANCE.make_price(3090.5),
            bid_size=ETHUSDT_PERP_BINANCE.make_qty(15.100),
            ask_size=ETHUSDT_PERP_BINANCE.make_qty(15.100),
            ts_event=0,
            ts_init=0,
        )

        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.SELL,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10.000),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(3150.0),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(3050.0),
        )

        sl_order = bracket.orders[1]
        tp_order = bracket.orders[2]

        # Act
        self.strategy.submit_order_list(bracket)
        self.strategy.cancel_order(sl_order)
        self.strategy.cancel_order(tp_order)
        self.exchange.process(0)

        # Assert
        assert bracket.orders[0].status == OrderStatus.FILLED
        assert sl_order.status == OrderStatus.CANCELED
        assert tp_order.status == OrderStatus.CANCELED

    def test_submit_bracket_limit_entry_buy_has_sl_tp_pending(self):
        # Arrange: Prepare market
        tick = QuoteTick(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            bid_price=ETHUSDT_PERP_BINANCE.make_price(3090.2),
            ask_price=ETHUSDT_PERP_BINANCE.make_price(3090.5),
            bid_size=ETHUSDT_PERP_BINANCE.make_qty(15.100),
            ask_size=ETHUSDT_PERP_BINANCE.make_qty(15.100),
            ts_event=0,
            ts_init=0,
        )

        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10.000),
            entry_price=ETHUSDT_PERP_BINANCE.make_price(3090.0),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(3050.0),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(3150.0),
            entry_order_type=OrderType.LIMIT,
        )

        # Act
        self.strategy.submit_order_list(bracket)
        self.exchange.process(0)
        #
        # # Assert
        assert bracket.orders[0].status == OrderStatus.ACCEPTED
        assert bracket.orders[1].status == OrderStatus.SUBMITTED
        assert bracket.orders[2].status == OrderStatus.SUBMITTED

    def test_submit_bracket_limit_entry_sell_has_sl_tp_pending(self):
        # Arrange: Prepare market
        tick = QuoteTick(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            bid_price=ETHUSDT_PERP_BINANCE.make_price(3090.2),
            ask_price=ETHUSDT_PERP_BINANCE.make_price(3090.5),
            bid_size=ETHUSDT_PERP_BINANCE.make_qty(15.100),
            ask_size=ETHUSDT_PERP_BINANCE.make_qty(15.100),
            ts_event=0,
            ts_init=0,
        )

        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.SELL,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10.000),
            entry_price=ETHUSDT_PERP_BINANCE.make_price(3100.0),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(3150.0),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(3050.0),
            entry_order_type=OrderType.LIMIT,
        )

        # Act
        self.strategy.submit_order_list(bracket)
        self.exchange.process(0)
        #
        # # Assert
        assert bracket.orders[0].status == OrderStatus.ACCEPTED
        assert bracket.orders[1].status == OrderStatus.SUBMITTED
        assert bracket.orders[2].status == OrderStatus.SUBMITTED

    def test_submit_bracket_limit_entry_buy_fills_then_triggers_sl_and_tp(self):
        # Arrange: Prepare market
        tick = QuoteTick(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            bid_price=ETHUSDT_PERP_BINANCE.make_price(3090.2),
            ask_price=ETHUSDT_PERP_BINANCE.make_price(3090.5),
            bid_size=ETHUSDT_PERP_BINANCE.make_qty(15.100),
            ask_size=ETHUSDT_PERP_BINANCE.make_qty(15.100),
            ts_event=0,
            ts_init=0,
        )

        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10.000),
            entry_price=ETHUSDT_PERP_BINANCE.make_price(3100.0),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(3050.0),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(3150.0),
            entry_order_type=OrderType.LIMIT,
        )

        # Act
        self.strategy.submit_order_list(bracket)
        self.exchange.process(0)

        # Assert
        assert bracket.orders[0].status == OrderStatus.FILLED
        assert bracket.orders[1].status == OrderStatus.ACCEPTED
        assert bracket.orders[2].status == OrderStatus.ACCEPTED
        assert len(self.exchange.get_open_orders()) == 2
        assert bracket.orders[1] in self.exchange.get_open_orders()
        assert bracket.orders[2] in self.exchange.get_open_orders()

    def test_submit_bracket_limit_entry_sell_fills_then_triggers_sl_and_tp(self):
        # Arrange: Prepare market
        tick = QuoteTick(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            bid_price=ETHUSDT_PERP_BINANCE.make_price(3090.2),
            ask_price=ETHUSDT_PERP_BINANCE.make_price(3090.5),
            bid_size=ETHUSDT_PERP_BINANCE.make_qty(15.100),
            ask_size=ETHUSDT_PERP_BINANCE.make_qty(15.100),
            ts_event=0,
            ts_init=0,
        )

        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.SELL,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10.000),
            entry_price=ETHUSDT_PERP_BINANCE.make_price(3050.0),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(3150.0),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(3000.0),
            entry_order_type=OrderType.LIMIT,
        )

        # Act
        self.strategy.submit_order_list(bracket)
        self.exchange.process(0)

        # Assert
        assert bracket.orders[0].status == OrderStatus.FILLED
        assert bracket.orders[1].status == OrderStatus.ACCEPTED
        assert bracket.orders[2].status == OrderStatus.ACCEPTED
        assert len(self.exchange.get_open_orders()) == 2
        assert bracket.orders[1] in self.exchange.get_open_orders()
        assert bracket.orders[2] in self.exchange.get_open_orders()

    def test_reject_bracket_entry_then_rejects_sl_and_tp(self):
        # Arrange: Prepare market
        tick = QuoteTick(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            bid_price=ETHUSDT_PERP_BINANCE.make_price(3090.2),
            ask_price=ETHUSDT_PERP_BINANCE.make_price(3090.5),
            bid_size=ETHUSDT_PERP_BINANCE.make_qty(15.100),
            ask_size=ETHUSDT_PERP_BINANCE.make_qty(15.100),
            ts_event=0,
            ts_init=0,
        )

        self.data_engine.process(tick)
        self.exchange.process_quote_tick(tick)

        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.SELL,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10.000),
            entry_price=ETHUSDT_PERP_BINANCE.make_price(3050.0),  # <-- in the market
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(3150.0),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(3000.0),
            entry_post_only=True,  # <-- will reject placed into the market
            entry_order_type=OrderType.LIMIT,
        )

        # Act
        self.strategy.submit_order_list(bracket)
        self.exchange.process(0)

        # Assert
        assert bracket.orders[0].status == OrderStatus.REJECTED
        assert bracket.orders[1].status == OrderStatus.REJECTED
        assert bracket.orders[2].status == OrderStatus.REJECTED
        assert len(self.exchange.get_open_orders()) == 0
        assert bracket.orders[1] not in self.exchange.get_open_orders()
        assert bracket.orders[2] not in self.exchange.get_open_orders()

    def test_filling_bracket_sl_cancels_tp_order(self):
        # Arrange: Prepare market
        tick1 = QuoteTick(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            bid_price=ETHUSDT_PERP_BINANCE.make_price(3090.2),
            ask_price=ETHUSDT_PERP_BINANCE.make_price(3090.5),
            bid_size=ETHUSDT_PERP_BINANCE.make_qty(15.100),
            ask_size=ETHUSDT_PERP_BINANCE.make_qty(15.100),
            ts_event=0,
            ts_init=0,
        )

        self.data_engine.process(tick1)
        self.exchange.process_quote_tick(tick1)

        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10.000),
            entry_price=ETHUSDT_PERP_BINANCE.make_price(3100.0),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(3050.0),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(3150.0),
            entry_order_type=OrderType.LIMIT,
        )

        self.strategy.submit_order_list(bracket)
        self.exchange.process(0)

        tick2 = QuoteTick(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            bid_price=ETHUSDT_PERP_BINANCE.make_price(3150.0),
            ask_price=ETHUSDT_PERP_BINANCE.make_price(3151.0),
            bid_size=ETHUSDT_PERP_BINANCE.make_qty(10.000),
            ask_size=ETHUSDT_PERP_BINANCE.make_qty(10.000),
            ts_event=0,
            ts_init=0,
        )

        # Act
        self.exchange.process_quote_tick(tick2)

        # Assert
        assert bracket.orders[0].status == OrderStatus.FILLED
        assert bracket.orders[1].status == OrderStatus.CANCELED
        assert bracket.orders[2].status == OrderStatus.FILLED
        assert len(self.exchange.get_open_orders()) == 0
        assert len(self.exchange.cache.positions_open()) == 0

    def test_filling_bracket_tp_cancels_sl_order(self):
        # Arrange: Prepare market
        tick1 = QuoteTick(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            bid_price=ETHUSDT_PERP_BINANCE.make_price(3090.2),
            ask_price=ETHUSDT_PERP_BINANCE.make_price(3090.5),
            bid_size=ETHUSDT_PERP_BINANCE.make_qty(15.100),
            ask_size=ETHUSDT_PERP_BINANCE.make_qty(15.100),
            ts_event=0,
            ts_init=0,
        )

        self.data_engine.process(tick1)
        self.exchange.process_quote_tick(tick1)

        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10.000),
            entry_price=ETHUSDT_PERP_BINANCE.make_price(3100.0),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(3050.0),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(3150.0),
            entry_order_type=OrderType.LIMIT,
        )

        self.strategy.submit_order_list(bracket)
        self.exchange.process(0)

        # Act
        tick2 = QuoteTick(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            bid_price=ETHUSDT_PERP_BINANCE.make_price(3150.0),
            ask_price=ETHUSDT_PERP_BINANCE.make_price(3151.0),
            bid_size=ETHUSDT_PERP_BINANCE.make_qty(10.000),
            ask_size=ETHUSDT_PERP_BINANCE.make_qty(10.000),
            ts_event=0,
            ts_init=0,
        )

        self.exchange.process_quote_tick(tick2)

        # Assert
        assert bracket.orders[0].status == OrderStatus.FILLED
        assert bracket.orders[1].status == OrderStatus.CANCELED
        assert bracket.orders[2].status == OrderStatus.FILLED
        assert len(self.exchange.get_open_orders()) == 0
        assert len(self.exchange.cache.positions_open()) == 0

    def test_partial_fill_bracket_tp_updates_sl_order(self):
        # Arrange: Prepare market
        tick1 = QuoteTick(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            bid_price=ETHUSDT_PERP_BINANCE.make_price(3090.2),
            ask_price=ETHUSDT_PERP_BINANCE.make_price(3090.5),
            bid_size=ETHUSDT_PERP_BINANCE.make_qty(15.100),
            ask_size=ETHUSDT_PERP_BINANCE.make_qty(15.100),
            ts_event=0,
            ts_init=0,
        )

        self.data_engine.process(tick1)
        self.exchange.process_quote_tick(tick1)

        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10.000),
            entry_price=ETHUSDT_PERP_BINANCE.make_price(3100.0),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(3050.0),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(3150.0),
            entry_order_type=OrderType.LIMIT,
        )

        en = bracket.orders[0]
        sl = bracket.orders[1]
        tp = bracket.orders[2]

        self.strategy.submit_order_list(bracket)
        self.exchange.process(0)

        # Act
        tick2 = QuoteTick(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            bid_price=ETHUSDT_PERP_BINANCE.make_price(3150.0),
            ask_price=ETHUSDT_PERP_BINANCE.make_price(3151.0),
            bid_size=ETHUSDT_PERP_BINANCE.make_qty(5.000),
            ask_size=ETHUSDT_PERP_BINANCE.make_qty(5.1000),
            ts_event=0,
            ts_init=0,
        )

        self.exchange.process_quote_tick(tick2)

        # Assert
        assert en.status == OrderStatus.FILLED
        assert sl.status == OrderStatus.ACCEPTED
        assert tp.status == OrderStatus.PARTIALLY_FILLED
        assert sl.quantity == Quantity.from_int(5)
        assert tp.leaves_qty == Quantity.from_int(5)
        assert tp.quantity == Quantity.from_int(10)
        assert len(self.exchange.get_open_orders()) == 2
        assert len(self.exchange.cache.positions_open()) == 1

    def test_modifying_bracket_tp_updates_sl_order(self):
        # Arrange: Prepare market
        tick1 = QuoteTick(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            bid_price=ETHUSDT_PERP_BINANCE.make_price(3090.2),
            ask_price=ETHUSDT_PERP_BINANCE.make_price(3090.5),
            bid_size=ETHUSDT_PERP_BINANCE.make_qty(15.100),
            ask_size=ETHUSDT_PERP_BINANCE.make_qty(15.100),
            ts_event=0,
            ts_init=0,
        )

        self.data_engine.process(tick1)
        self.exchange.process_quote_tick(tick1)

        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10.000),
            entry_price=ETHUSDT_PERP_BINANCE.make_price(3100.0),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(3050.0),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(3150.0),
            entry_order_type=OrderType.LIMIT,
        )

        en = bracket.orders[0]
        sl = bracket.orders[1]
        tp = bracket.orders[2]

        self.strategy.submit_order_list(bracket)
        self.exchange.process(0)

        # Act
        self.strategy.modify_order(
            order=sl,
            quantity=Quantity.from_int(5),
            trigger_price=sl.trigger_price,
        )
        self.exchange.process(0)

        # Assert
        assert en.status == OrderStatus.FILLED
        assert sl.status == OrderStatus.ACCEPTED
        assert tp.status == OrderStatus.ACCEPTED
        assert sl.quantity == Quantity.from_int(5)
        assert tp.quantity == Quantity.from_int(5)
        assert len(self.exchange.get_open_orders()) == 2
        assert len(self.exchange.cache.positions_open()) == 1

    def test_closing_position_cancels_bracket_ocos(self):
        # Arrange: Prepare market
        tick1 = QuoteTick(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            bid_price=ETHUSDT_PERP_BINANCE.make_price(3090.2),
            ask_price=ETHUSDT_PERP_BINANCE.make_price(3090.5),
            bid_size=ETHUSDT_PERP_BINANCE.make_qty(15.100),
            ask_size=ETHUSDT_PERP_BINANCE.make_qty(15.100),
            ts_event=0,
            ts_init=0,
        )

        self.data_engine.process(tick1)
        self.exchange.process_quote_tick(tick1)

        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10.000),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(3050.0),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(3150.0),
        )

        en = bracket.orders[0]
        sl = bracket.orders[1]
        tp = bracket.orders[2]

        self.strategy.submit_order_list(bracket)
        self.exchange.process(0)

        # Act
        self.strategy.close_position(self.strategy.cache.position(en.position_id))
        self.exchange.process(0)

        # Assert
        assert en.status == OrderStatus.FILLED
        assert sl.status == OrderStatus.CANCELED
        assert tp.status == OrderStatus.CANCELED
        assert len(self.exchange.get_open_orders()) == 0
        assert len(self.exchange.cache.positions_open()) == 0

    def test_partially_filling_position_updates_bracket_ocos(self):
        # Arrange: Prepare market
        tick1 = QuoteTick(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            bid_price=ETHUSDT_PERP_BINANCE.make_price(3090.2),
            ask_price=ETHUSDT_PERP_BINANCE.make_price(3090.5),
            bid_size=ETHUSDT_PERP_BINANCE.make_qty(15.100),
            ask_size=ETHUSDT_PERP_BINANCE.make_qty(15.100),
            ts_event=0,
            ts_init=0,
        )

        self.data_engine.process(tick1)
        self.exchange.process_quote_tick(tick1)

        bracket = self.strategy.order_factory.bracket(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.BUY,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(10.000),
            sl_trigger_price=ETHUSDT_PERP_BINANCE.make_price(3050.0),
            tp_price=ETHUSDT_PERP_BINANCE.make_price(3150.0),
        )

        en = bracket.orders[0]
        sl = bracket.orders[1]
        tp = bracket.orders[2]

        self.strategy.submit_order_list(bracket)
        self.exchange.process(0)

        # Act
        reduce_order = self.strategy.order_factory.market(
            instrument_id=ETHUSDT_PERP_BINANCE.id,
            order_side=OrderSide.SELL,
            quantity=ETHUSDT_PERP_BINANCE.make_qty(5.000),
        )
        self.strategy.submit_order(
            reduce_order,
            position_id=self.cache.position_for_order(en.client_order_id).id,
        )
        self.exchange.process(0)

        # Assert
        assert en.status == OrderStatus.FILLED
        assert sl.status == OrderStatus.ACCEPTED
        assert tp.status == OrderStatus.ACCEPTED
        assert sl.quantity == ETHUSDT_PERP_BINANCE.make_qty(5.000)
        assert tp.quantity == ETHUSDT_PERP_BINANCE.make_qty(5.000)
        assert len(self.exchange.get_open_orders()) == 2
        assert len(self.exchange.cache.positions_open()) == 1
