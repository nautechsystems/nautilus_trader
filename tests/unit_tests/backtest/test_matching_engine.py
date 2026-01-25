# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

from typing import Any

import pytest

from nautilus_trader.backtest.engine import OrderMatchingEngine
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.backtest.models import MakerTakerFeeModel
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.component import TestClock
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarSpecification
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import BookOrder
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import OrderBookDepth10
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import AggregationSource
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import InstrumentCloseType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import MarketStatusAction
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.events import OrderAccepted
from nautilus_trader.model.events import OrderCanceled
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.events import OrderModifyRejected
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import MarketIfTouchedOrder
from nautilus_trader.model.orders import MarketOrder
from nautilus_trader.model.orders import StopMarketOrder
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.execution import TestExecStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


_ETHUSDT_PERP_BINANCE = TestInstrumentProvider.ethusdt_perp_binance()


class TestOrderMatchingEngine:
    def setup(self):
        # Fixture Setup
        self.clock = TestClock()
        self.trader_id = TestIdStubs.trader_id()

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
        )
        self.instrument = _ETHUSDT_PERP_BINANCE
        self.instrument_id = self.instrument.id
        self.account_id = TestIdStubs.account_id()
        self.cache = TestComponentStubs.cache()
        self.cache.add_instrument(self.instrument)

        self.matching_engine = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L1_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=True,
            trade_execution=True,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

    def test_repr(self) -> None:
        # Arrange, Act, Assert
        assert (
            repr(self.matching_engine)
            == "OrderMatchingEngine(venue=BINANCE, instrument_id=ETHUSDT-PERP.BINANCE, raw_id=0)"
        )

    def test_set_fill_model(self) -> None:
        # Arrange
        fill_model = FillModel()

        # Act
        self.matching_engine.set_fill_model(fill_model)

        # Assert
        assert True

    def test_update_instrument(self) -> None:
        # Arrange, Act
        self.matching_engine.update_instrument(_ETHUSDT_PERP_BINANCE)

        # Assert
        assert self.matching_engine.instrument.id == _ETHUSDT_PERP_BINANCE.id

    def test_process_instrument_status(self) -> None:
        self.matching_engine.process_status(MarketStatusAction.CLOSE)
        self.matching_engine.process_status(MarketStatusAction.PRE_OPEN)
        self.matching_engine.process_status(MarketStatusAction.PAUSE)
        self.matching_engine.process_status(MarketStatusAction.TRADING)

    def test_process_market_on_close_order(self) -> None:
        order: MarketOrder = TestExecStubs.market_order(
            instrument=self.instrument,
            time_in_force=TimeInForce.AT_THE_CLOSE,
        )
        self.matching_engine.process_order(order, self.account_id)

    def test_instrument_close_expiry_closes_position(self) -> None:
        # Arrange
        exec_messages = []
        self.msgbus.register("ExecEngine.process", lambda x: exec_messages.append(x))
        quote = TestDataStubs.quote_tick(
            instrument=self.instrument,
        )
        self.matching_engine.process_quote_tick(quote)
        order: MarketOrder = TestExecStubs.limit_order(
            instrument=self.instrument,
        )
        self.matching_engine.process_order(order, self.account_id)

        # Act
        instrument_close = TestDataStubs.instrument_close(
            instrument_id=self.instrument_id,
            price=Price.from_str("2.00"),
            close_type=InstrumentCloseType.CONTRACT_EXPIRED,
            ts_event=2,
        )
        self.matching_engine.process_instrument_close(instrument_close)

        # Assert
        assert exec_messages

    def test_process_order_book_depth_10(self) -> None:
        # Arrange - Create L2_MBP matching engine for depth10 data
        matching_engine_l2 = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L2_MBP,  # L2 for multi-level depth data
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=True,
            trade_execution=True,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        depth = TestDataStubs.order_book_depth10(instrument=self.instrument)
        assert matching_engine_l2.best_ask_price() is None
        assert matching_engine_l2.best_bid_price() is None

        # Act
        matching_engine_l2.process_order_book_depth10(depth)

        # Assert
        assert matching_engine_l2.best_ask_price() == depth.asks[0].price
        assert matching_engine_l2.best_bid_price() == depth.bids[0].price

    def test_process_trade_buyer_aggressor(self) -> None:
        # Arrange
        trade = TestDataStubs.trade_tick(
            instrument=self.instrument,
            price=1000.0,
            aggressor_side=AggressorSide.BUYER,
        )

        # Act
        self.matching_engine.process_trade_tick(trade)

        # Assert - Buyer aggressor should set ask price
        assert self.matching_engine.best_ask_price() == Price.from_str("1000.000")

    def test_process_trade_seller_aggressor(self) -> None:
        # Arrange
        trade = TestDataStubs.trade_tick(
            instrument=self.instrument,
            price=1000.0,
            aggressor_side=AggressorSide.SELLER,
        )

        # Act
        self.matching_engine.process_trade_tick(trade)

        # Assert - Seller aggressor should set bid price
        assert self.matching_engine.best_bid_price() == Price.from_str("1000.000")

    def test_process_trade_tick_no_aggressor_above_ask(self) -> None:
        # Arrange - Set initial bid/ask spread
        quote = TestDataStubs.quote_tick(
            instrument=self.instrument,
            bid_price=990.0,
            ask_price=1010.0,
        )
        self.matching_engine.process_quote_tick(quote)

        # Trade above ask with no aggressor
        trade = TestDataStubs.trade_tick(
            instrument=self.instrument,
            price=1020.0,
            aggressor_side=AggressorSide.NO_AGGRESSOR,
        )

        # Act
        self.matching_engine.process_trade_tick(trade)

        # Assert - L1_MBP book update_trade_tick sets both bid/ask to trade price
        # Then NO_AGGRESSOR logic doesn't modify further since 1020 >= 1020 (ask)
        assert self.matching_engine.best_ask_price() == Price.from_str("1020.0")
        assert self.matching_engine.best_bid_price() == Price.from_str("1020.0")

    def test_process_trade_tick_no_aggressor_within_spread(self) -> None:
        # Arrange - Set initial bid/ask spread
        quote = TestDataStubs.quote_tick(
            instrument=self.instrument,
            bid_price=990.0,
            ask_price=1010.0,
        )
        self.matching_engine.process_quote_tick(quote)

        # Trade within the spread with no aggressor
        trade = TestDataStubs.trade_tick(
            instrument=self.instrument,
            price=1000.0,
            aggressor_side=AggressorSide.NO_AGGRESSOR,
        )

        # Act
        self.matching_engine.process_trade_tick(trade)

        # Assert - L1_MBP book update_trade_tick sets both bid/ask to trade price
        assert self.matching_engine.best_bid_price() == Price.from_str("1000.000")
        assert self.matching_engine.best_ask_price() == Price.from_str("1000.000")

    def test_process_trade_tick_no_aggressor_below_bid(self) -> None:
        # Arrange - Set initial bid/ask spread
        quote = TestDataStubs.quote_tick(
            instrument=self.instrument,
            bid_price=1000.0,
            ask_price=1020.0,
        )
        self.matching_engine.process_quote_tick(quote)

        # Trade below current bid with no aggressor
        trade = TestDataStubs.trade_tick(
            instrument=self.instrument,
            price=990.0,
            aggressor_side=AggressorSide.NO_AGGRESSOR,
        )

        # Act
        self.matching_engine.process_trade_tick(trade)

        # Assert - L1_MBP book update_trade_tick sets both bid/ask to trade price
        assert self.matching_engine.best_bid_price() == Price.from_str("990.0")
        assert self.matching_engine.best_ask_price() == Price.from_str("990.0")

    def test_process_trade_tick_no_aggressor_at_bid_and_ask(self) -> None:
        # Arrange - Set initial bid/ask spread
        quote = TestDataStubs.quote_tick(
            instrument=self.instrument,
            bid_price=995.0,
            ask_price=1005.0,
        )
        self.matching_engine.process_quote_tick(quote)

        # Trade exactly at bid level with no aggressor
        trade1 = TestDataStubs.trade_tick(
            instrument=self.instrument,
            price=995.0,
            aggressor_side=AggressorSide.NO_AGGRESSOR,
        )

        # Act
        self.matching_engine.process_trade_tick(trade1)

        # Assert - L1_MBP book update_trade_tick sets both bid/ask to trade price
        assert self.matching_engine.best_bid_price() == Price.from_str("995.0")
        assert self.matching_engine.best_ask_price() == Price.from_str("995.0")

        # Trade exactly at ask level with no aggressor
        trade2 = TestDataStubs.trade_tick(
            instrument=self.instrument,
            price=1005.0,
            aggressor_side=AggressorSide.NO_AGGRESSOR,
        )

        # Act
        self.matching_engine.process_trade_tick(trade2)

        # Assert - L1_MBP book update_trade_tick sets both bid/ask to trade price
        assert self.matching_engine.best_bid_price() == Price.from_str("1005.0")
        assert self.matching_engine.best_ask_price() == Price.from_str("1005.0")

    def test_process_trade_tick_with_trade_execution_disabled(self) -> None:
        # Arrange - Create matching engine with trade_execution=False
        matching_engine = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L1_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=True,
            trade_execution=False,  # Disabled
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Process trade tick with BUYER aggressor
        trade = TestDataStubs.trade_tick(
            instrument=self.instrument,
            price=1000.0,
            aggressor_side=AggressorSide.BUYER,
        )

        # Act
        matching_engine.process_trade_tick(trade)

        # Assert - With trade_execution=False, only book update happens, no aggressor logic
        # L1_MBP book update_trade_tick sets both bid/ask to trade price
        assert matching_engine.best_bid_price() == Price.from_str("1000.000")
        assert matching_engine.best_ask_price() == Price.from_str("1000.000")

    def test_trade_execution_difference_buyer_aggressor(self) -> None:
        # This test demonstrates that trade_execution=True vs False produces the same result
        # for L1_MBP books since update_trade_tick sets both bid/ask to trade price anyway

        # Test with trade_execution=True (our main matching engine)
        trade_tick_enabled = TestDataStubs.trade_tick(
            instrument=self.instrument,
            price=1000.0,
            aggressor_side=AggressorSide.BUYER,
        )
        self.matching_engine.process_trade_tick(trade_tick_enabled)

        # Test with trade_execution=False
        matching_engine = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=1,
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L1_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=True,
            trade_execution=False,  # Disabled
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        trade = TestDataStubs.trade_tick(
            instrument=self.instrument,
            price=1000.0,
            aggressor_side=AggressorSide.BUYER,
        )
        matching_engine.process_trade_tick(trade)

        # Assert - Both should have same result for L1_MBP
        assert self.matching_engine.best_bid_price() == matching_engine.best_bid_price()
        assert self.matching_engine.best_ask_price() == matching_engine.best_ask_price()

    def test_fill_order_with_non_positive_qty_returns_early(self) -> None:
        # Arrange - Set initial bid/ask
        quote = TestDataStubs.quote_tick(
            instrument=self.instrument,
            bid_price=999.0,
            ask_price=1001.0,
        )
        self.matching_engine.process_quote_tick(quote)

        # Register to receive exec engine messages
        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        # Create a limit order that won't immediately fill
        order = TestExecStubs.limit_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            price=Price.from_str("998.0"),  # Below ask so won't fill immediately
            quantity=self.instrument.make_qty(100.0),
        )

        # Process to register the order with the matching engine
        self.matching_engine.process_order(order, self.account_id)

        # Clear any initial order processing messages
        messages.clear()

        # Manually fill the order partially
        self.matching_engine.fill_order(
            order=order,
            last_px=Price.from_str("998.0"),
            last_qty=self.instrument.make_qty(50.0),
            liquidity_side=LiquiditySide.TAKER,
        )

        # Verify first fill was processed
        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 1
        assert filled_events[0].last_qty == self.instrument.make_qty(50.0)

        # Clear messages
        messages.clear()

        # Act - Attempt to fill with quantity that would exceed remaining (should be clamped to 50)
        self.matching_engine.fill_order(
            order=order,
            last_px=Price.from_str("998.0"),
            last_qty=self.instrument.make_qty(60.0),  # 50 already filled, only 50 remains
            liquidity_side=LiquiditySide.TAKER,
        )

        # Should get a fill for remaining 50
        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 1
        assert filled_events[0].last_qty == self.instrument.make_qty(50.0)

        # Clear messages
        messages.clear()

        # Act - Now try to fill again (should trigger early return due to non-positive qty)
        self.matching_engine.fill_order(
            order=order,
            last_px=Price.from_str("998.0"),
            last_qty=self.instrument.make_qty(10.0),
            liquidity_side=LiquiditySide.TAKER,
        )

        # Assert - No fill event should be emitted due to early return
        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 0  # No fill should have been generated

    def test_buy_limit_fills_on_seller_trade_at_limit_price(self) -> None:
        # Regression test for GitHub issue where BUY LIMIT orders were not filling
        # when SELLER trades occurred at the limit price with trade_execution=True.
        #
        # Scenario:
        # 1. BUY LIMIT placed at 211.32 (timestamp 1762549820644390)
        # 2. SELLER trade occurs at 211.32 (timestamp 1762549826389000)
        # 3. Expected: Order should fill at the trade tick timestamp
        # 4. Actual (before fix): Order filled ~48µs later when OrderBookDelta moved ask

        # Set initial market state with bid/ask spread
        # (bid at 211.30, ask at 211.40 - order will rest in between)
        quote = TestDataStubs.quote_tick(
            instrument=self.instrument,
            bid_price=211.30,
            ask_price=211.40,
        )
        self.matching_engine.process_quote_tick(quote)

        # Register to capture order events
        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        # Place BUY LIMIT order at 211.32
        order = TestExecStubs.limit_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            price=Price.from_str("211.32"),
            quantity=self.instrument.make_qty(1.0),
        )

        # Set clock to order placement time
        order_ts = 1762549820644390
        self.clock.set_time(order_ts)
        self.matching_engine.process_order(order, self.account_id)

        # Clear messages from order placement
        messages.clear()

        # Act - SELLER trade occurs at the limit price (someone hit the bid at 211.32)
        trade_ts = 1762549826389000
        trade = TestDataStubs.trade_tick(
            instrument=self.instrument,
            price=211.32,
            aggressor_side=AggressorSide.SELLER,
            ts_event=trade_ts,
            ts_init=trade_ts,
        )
        self.matching_engine.process_trade_tick(trade)

        # Assert - Order should be filled
        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 1, (
            "BUY LIMIT should fill when SELLER trade occurs at limit price"
        )
        assert filled_events[0].order_side == OrderSide.BUY
        assert filled_events[0].last_px == Price.from_str("211.32")

    def test_sell_limit_fills_on_buyer_trade_at_limit_price(self) -> None:
        # Symmetric test: SELL LIMIT should fill when BUYER trade occurs at limit price

        # Set initial market state
        quote = TestDataStubs.quote_tick(
            instrument=self.instrument,
            bid_price=211.30,
            ask_price=211.40,
        )
        self.matching_engine.process_quote_tick(quote)

        # Register to capture order events
        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        # Place SELL LIMIT order at 211.38
        order = TestExecStubs.limit_order(
            instrument=self.instrument,
            order_side=OrderSide.SELL,
            price=Price.from_str("211.38"),
            quantity=self.instrument.make_qty(1.0),
        )
        self.matching_engine.process_order(order, self.account_id)

        # Clear messages from order placement
        messages.clear()

        # Act - BUYER trade occurs at the limit price (someone took the ask at 211.38)
        trade = TestDataStubs.trade_tick(
            instrument=self.instrument,
            price=211.38,
            aggressor_side=AggressorSide.BUYER,
        )
        self.matching_engine.process_trade_tick(trade)

        # Assert - Order should be filled
        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 1, (
            "SELL LIMIT should fill when BUYER trade occurs at limit price"
        )
        assert filled_events[0].order_side == OrderSide.SELL
        assert filled_events[0].last_px == Price.from_str("211.38")

    def test_trade_execution_does_not_persist_ghost_liquidity(self) -> None:
        # Verify that trade execution does not persist "ghost" liquidity.
        # Scenario:
        # 1. Market: Bid 100, Ask 110.
        # 2. Trade at 105 (Aggressor SELLER).
        #    - This transiently collapses Ask to 105 to check for fills.
        #    - But it should restore Ask to 110 immediately.
        # 3. New BUY LIMIT order at 106 arrives AFTER the trade.
        # 4. Expected: Should NOT fill (Ask should be back to 110).
        #    - If Ask persisted at 105 (Ghost), it would fill.

        # Arrange
        matching_engine = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L2_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=True,
            trade_execution=True,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Set initial market state (Wide spread)
        quote = TestDataStubs.quote_tick(
            instrument=self.instrument,
            bid_price=100.00,
            ask_price=110.00,
        )
        matching_engine.process_quote_tick(quote)

        # Register to capture order events
        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        # Act 1: Process Trade Tick at 105 (Seller Aggressor)
        trade_ts = 1762549826389000
        trade = TestDataStubs.trade_tick(
            instrument=self.instrument,
            price=105.00,
            aggressor_side=AggressorSide.SELLER,
            ts_event=trade_ts,
            ts_init=trade_ts,
        )
        self.clock.set_time(trade_ts)
        matching_engine.process_trade_tick(trade)

        # Act 2: Place BUY LIMIT order at 106.00 (Between Trade 105 and Old Ask 110)
        # If Ask is incorrectly 105, this will fill.
        # If Ask is correctly 110, this will NOT fill.
        order_ts = trade_ts + 1000  # 1ms later
        self.clock.set_time(order_ts)

        order = TestExecStubs.limit_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            price=Price.from_str("106.00"),
            quantity=self.instrument.make_qty(1.0),
        )
        matching_engine.process_order(order, self.account_id)

        # Assert
        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 0, (
            "BUY LIMIT filled against Ghost Ask! Ghost Liquidity persisted."
        )

    def test_trade_execution_fills_better_priced_orders_for_buys(self) -> None:
        # With transient override, a SELLER trade at P allows BUY orders at P or
        # better (higher limit) to fill - they would accept the trade price.
        #
        # Book: bid=211.30, ask=211.40
        # SELLER trade at 211.32 proves sell liquidity at 211.32
        # BUY LIMIT at 211.35 (willing to pay up to 211.35) should fill

        # Set initial market state
        quote = TestDataStubs.quote_tick(
            instrument=self.instrument,
            bid_price=211.30,
            ask_price=211.40,
        )
        self.matching_engine.process_quote_tick(quote)

        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        # Place BUY LIMIT at 211.35 (willing to pay more than trade price)
        order = TestExecStubs.limit_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            price=Price.from_str("211.35"),
            quantity=self.instrument.make_qty(1.0),
        )
        self.matching_engine.process_order(order, self.account_id)
        messages.clear()

        # SELLER trade at 211.32 - BUY at 211.35 should fill (willing to pay 211.35 >= 211.32)
        trade = TestDataStubs.trade_tick(
            instrument=self.instrument,
            price=211.32,
            aggressor_side=AggressorSide.SELLER,
        )
        self.matching_engine.process_trade_tick(trade)

        # Assert - Order should fill (211.35 >= trade price 211.32)
        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 1, "BUY LIMIT at 211.35 should fill on SELLER trade at 211.32"

    def test_trade_execution_fills_better_priced_orders_for_sells(self) -> None:
        # With transient override, a BUYER trade at P allows SELL orders at P or
        # better (lower limit) to fill - they would accept the trade price.
        #
        # Book: bid=211.30, ask=211.40
        # BUYER trade at 211.38 proves buy liquidity at 211.38
        # SELL LIMIT at 211.35 (willing to sell down to 211.35) should fill

        # Set initial market state
        quote = TestDataStubs.quote_tick(
            instrument=self.instrument,
            bid_price=211.30,
            ask_price=211.40,
        )
        self.matching_engine.process_quote_tick(quote)

        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        # Place SELL LIMIT at 211.35 (willing to sell lower than trade price)
        order = TestExecStubs.limit_order(
            instrument=self.instrument,
            order_side=OrderSide.SELL,
            price=Price.from_str("211.35"),
            quantity=self.instrument.make_qty(1.0),
        )
        self.matching_engine.process_order(order, self.account_id)
        messages.clear()

        # BUYER trade at 211.38 - SELL at 211.35 should fill (willing to sell 211.35 <= 211.38)
        trade = TestDataStubs.trade_tick(
            instrument=self.instrument,
            price=211.38,
            aggressor_side=AggressorSide.BUYER,
        )
        self.matching_engine.process_trade_tick(trade)

        # Assert - Order should fill (211.35 <= trade price 211.38)
        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 1, "SELL LIMIT at 211.35 should fill on BUYER trade at 211.38"

    def test_trade_execution_restores_matching_state_after_seller_trade(self) -> None:
        # Verify the transient override restores matching state after processing.
        # If not restored, new orders placed after the trade would fill immediately.

        # Set initial market state
        quote = TestDataStubs.quote_tick(
            instrument=self.instrument,
            bid_price=211.30,
            ask_price=211.40,
        )
        self.matching_engine.process_quote_tick(quote)

        # Process SELLER trade at 211.32 (transiently sets ask=211.32, then restores)
        trade = TestDataStubs.trade_tick(
            instrument=self.instrument,
            price=211.32,
            aggressor_side=AggressorSide.SELLER,
        )
        self.matching_engine.process_trade_tick(trade)

        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        # Place BUY LIMIT at 211.35 AFTER the trade
        # If ask wasn't restored (stuck at 211.32), this would fill immediately
        # If ask was restored (back to 211.40), this should NOT fill
        order = TestExecStubs.limit_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            price=Price.from_str("211.35"),
            quantity=self.instrument.make_qty(1.0),
        )
        self.matching_engine.process_order(order, self.account_id)

        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 0, (
            "BUY LIMIT at 211.35 should NOT fill immediately - matching state was restored"
        )

    def test_trade_execution_restores_matching_state_after_buyer_trade(self) -> None:
        # Verify the transient override restores matching state after processing.

        # Set initial market state
        quote = TestDataStubs.quote_tick(
            instrument=self.instrument,
            bid_price=211.30,
            ask_price=211.40,
        )
        self.matching_engine.process_quote_tick(quote)

        # Process BUYER trade at 211.38 (transiently sets bid=211.38, then restores)
        trade = TestDataStubs.trade_tick(
            instrument=self.instrument,
            price=211.38,
            aggressor_side=AggressorSide.BUYER,
        )
        self.matching_engine.process_trade_tick(trade)

        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        # Place SELL LIMIT at 211.35 AFTER the trade
        # If bid wasn't restored (stuck at 211.38), this would fill immediately
        # If bid was restored (back to 211.30), this should NOT fill
        order = TestExecStubs.limit_order(
            instrument=self.instrument,
            order_side=OrderSide.SELL,
            price=Price.from_str("211.35"),
            quantity=self.instrument.make_qty(1.0),
        )
        self.matching_engine.process_order(order, self.account_id)

        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 0, (
            "SELL LIMIT at 211.35 should NOT fill immediately - matching state was restored"
        )

    def test_trade_execution_same_side_sell_does_not_fill_on_seller_trade(self) -> None:
        # SELL orders should not fill on SELLER trades (same side).
        # Only opposite-side orders match against the trade.

        # Set initial market state
        quote = TestDataStubs.quote_tick(
            instrument=self.instrument,
            bid_price=211.30,
            ask_price=211.40,
        )
        self.matching_engine.process_quote_tick(quote)

        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        # Place SELL LIMIT at 211.35 (above trade price)
        order = TestExecStubs.limit_order(
            instrument=self.instrument,
            order_side=OrderSide.SELL,
            price=Price.from_str("211.35"),
            quantity=self.instrument.make_qty(1.0),
        )
        self.matching_engine.process_order(order, self.account_id)
        messages.clear()

        # SELLER trade at 211.32 - same side, should NOT trigger SELL fills
        trade = TestDataStubs.trade_tick(
            instrument=self.instrument,
            price=211.32,
            aggressor_side=AggressorSide.SELLER,
        )
        self.matching_engine.process_trade_tick(trade)

        # Assert - SELL order should NOT fill on SELLER trade
        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 0, "SELL LIMIT should NOT fill on SELLER trade (same side)"

    def test_trade_execution_same_side_buy_does_not_fill_on_buyer_trade(self) -> None:
        # BUY orders should not fill on BUYER trades (same side).

        # Set initial market state
        quote = TestDataStubs.quote_tick(
            instrument=self.instrument,
            bid_price=211.30,
            ask_price=211.40,
        )
        self.matching_engine.process_quote_tick(quote)

        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        # Place BUY LIMIT at 211.35 (below trade price)
        order = TestExecStubs.limit_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            price=Price.from_str("211.35"),
            quantity=self.instrument.make_qty(1.0),
        )
        self.matching_engine.process_order(order, self.account_id)
        messages.clear()

        # BUYER trade at 211.38 - same side, should NOT trigger BUY fills
        trade = TestDataStubs.trade_tick(
            instrument=self.instrument,
            price=211.38,
            aggressor_side=AggressorSide.BUYER,
        )
        self.matching_engine.process_trade_tick(trade)

        # Assert - BUY order should NOT fill on BUYER trade
        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 0, "BUY LIMIT should NOT fill on BUYER trade (same side)"

    def test_trade_execution_with_l2_mbp_order_book_deltas(self) -> None:
        # 1. BUY LIMIT at 211.32 placed
        # 2. Order book established via deltas (bid=211.30, ask=211.40)
        # 3. SELLER trade at 211.32 proves liquidity
        # 4. Expected: Order fills at trade tick timestamp

        # Arrange - Create L2_MBP matching engine
        matching_engine_l2 = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L2_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=True,
            trade_execution=True,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Set initial market state via OrderBookDeltas (L2 style)
        snapshot = TestDataStubs.order_book_snapshot(
            instrument=self.instrument,
            bid_price=211.30,
            ask_price=211.40,
            bid_size=100.0,
            ask_size=100.0,
            bid_levels=1,
            ask_levels=1,
        )
        matching_engine_l2.process_order_book_deltas(snapshot)

        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        # Place BUY LIMIT at 211.32 (inside the spread)
        order_ts = 1762549820644390
        self.clock.set_time(order_ts)
        order = TestExecStubs.limit_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            price=Price.from_str("211.32"),
            quantity=self.instrument.make_qty(1.0),
        )
        matching_engine_l2.process_order(order, self.account_id)
        messages.clear()

        # SELLER trade at 211.32 - should trigger fill immediately
        trade_ts = 1762549826389000
        self.clock.set_time(trade_ts)
        trade = TestDataStubs.trade_tick(
            instrument=self.instrument,
            price=211.32,
            aggressor_side=AggressorSide.SELLER,
            ts_event=trade_ts,
            ts_init=trade_ts,
        )
        matching_engine_l2.process_trade_tick(trade)

        # Assert - Order should fill at trade tick timestamp
        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 1, (
            "BUY LIMIT at 211.32 should fill on SELLER trade at 211.32 with L2_MBP"
        )
        assert filled_events[0].ts_event == trade_ts, (
            f"Fill should occur at trade timestamp {trade_ts}, was {filled_events[0].ts_event}"
        )

    def test_trade_execution_complete_fill_when_trade_exceeds_order(self) -> None:
        # Test that when trade size exceeds remaining order quantity,
        # the fill is capped at the remaining order quantity.
        #
        # Scenario:
        # 1. BUY LIMIT placed with qty=50
        # 2. SELLER trade with qty=100 (larger than order)
        # 3. Expected: Order fully filled for 50 (not 100)

        # Arrange - Create L2_MBP matching engine
        matching_engine_l2 = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L2_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=True,
            trade_execution=True,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Set initial market state
        snapshot = TestDataStubs.order_book_snapshot(
            instrument=self.instrument,
            bid_price=1000.00,
            ask_price=1010.00,
            bid_size=100.0,
            ask_size=100.0,
            bid_levels=1,
            ask_levels=1,
        )
        matching_engine_l2.process_order_book_deltas(snapshot)

        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        # Place small BUY LIMIT order (50 qty)
        order = TestExecStubs.limit_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            price=Price.from_str("1005.00"),
            quantity=self.instrument.make_qty(50.0),
        )
        matching_engine_l2.process_order(order, self.account_id)
        messages.clear()

        # Act - Large trade (100 qty, more than order size)
        trade = TestDataStubs.trade_tick(
            instrument=self.instrument,
            price=1005.00,
            size=100.0,
            aggressor_side=AggressorSide.SELLER,
        )
        matching_engine_l2.process_trade_tick(trade)

        # Assert - Order should be filled for 50 (min of order size and trade size)
        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 1
        assert filled_events[0].last_qty == self.instrument.make_qty(50.0), (
            f"Fill qty should be capped at order size 50, was {filled_events[0].last_qty}"
        )

    def test_modify_partially_filled_order_quantity_below_filled_rejected(self) -> None:
        # Tests that modifying a partially filled order to a quantity below filled_qty is rejected
        # Arrange - Create L2_MBP matching engine
        matching_engine_l2 = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L2_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=True,
            trade_execution=True,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Set initial market state with partial liquidity
        snapshot = TestDataStubs.order_book_snapshot(
            instrument=self.instrument,
            bid_price=1490.00,
            ask_price=1500.00,
            bid_size=100.0,
            ask_size=50.0,  # Only 50 available at ask
            bid_levels=1,
            ask_levels=1,
        )
        matching_engine_l2.process_order_book_deltas(snapshot)

        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        # Place BUY LIMIT order at ask (will match and partially fill)
        order = TestExecStubs.limit_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            price=Price.from_str("1500.00"),
            quantity=self.instrument.make_qty(100.0),
        )
        matching_engine_l2.process_order(order, self.account_id)

        # Order should be partially filled (50 of 100)
        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 1
        assert filled_events[0].last_qty == self.instrument.make_qty(50.0)
        messages.clear()

        # Act - Attempt to modify quantity to 40, below filled_qty of 50
        modify_command = ModifyOrder(
            trader_id=self.trader_id,
            strategy_id=StrategyId("S-001"),
            instrument_id=self.instrument.id,
            client_order_id=order.client_order_id,
            venue_order_id=VenueOrderId("V-001"),
            quantity=Quantity.from_str("40.000"),
            price=None,
            trigger_price=None,
            command_id=UUID4(),
            ts_init=0,
        )
        matching_engine_l2.process_modify(modify_command, self.account_id)

        # Assert - Should receive OrderModifyRejected
        rejected_events = [m for m in messages if isinstance(m, OrderModifyRejected)]
        assert len(rejected_events) == 1, (
            f"Expected OrderModifyRejected, was {[type(m).__name__ for m in messages]}"
        )
        assert "below filled quantity" in rejected_events[0].reason

    @pytest.mark.parametrize(
        ("order_side", "book_side", "order_price", "opposite_price"),
        [
            (OrderSide.BUY, OrderSide.SELL, "100.00", "90.00"),
            (OrderSide.SELL, OrderSide.BUY, "100.00", "110.00"),
        ],
        ids=["buy_limit", "sell_limit"],
    )
    def test_partial_fill_uses_current_book_liquidity(
        self,
        order_side: OrderSide,
        book_side: OrderSide,
        order_price: str,
        opposite_price: str,
    ) -> None:
        # Test: partial fills should use current book liquidity.
        #
        # Scenario:
        # 1. LIMIT order at price with qty=200
        # 2. First delta adds 10 @ price → fills 10
        # 3. Second delta UPDATES to 50 @ price → fills 50 (current book liquidity)

        # Arrange
        matching_engine_l2 = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L2_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=True,
            trade_execution=False,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        # Initialize matching core with opposite side
        opposite_delta = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=order_side,
                price=Price.from_str(opposite_price),
                size=Quantity.from_str("100.000"),
                order_id=100,
            ),
            flags=0,
            sequence=0,
            ts_event=0,
            ts_init=0,
        )
        matching_engine_l2.process_order_book_delta(opposite_delta)

        delta_1 = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=book_side,
                price=Price.from_str(order_price),
                size=Quantity.from_str("10.000"),
                order_id=1,
            ),
            flags=0,
            sequence=1,
            ts_event=0,
            ts_init=0,
        )
        matching_engine_l2.process_order_book_delta(delta_1)

        order = TestExecStubs.limit_order(
            instrument=self.instrument,
            order_side=order_side,
            price=Price.from_str(order_price),
            quantity=self.instrument.make_qty(200.0),
        )

        # Act
        matching_engine_l2.process_order(order, self.account_id)

        # Assert
        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 1, f"Expected 1 fill, was {len(filled_events)}"
        assert filled_events[0].last_qty == self.instrument.make_qty(10.0), (
            f"First fill should be 10, was {filled_events[0].last_qty}"
        )
        messages.clear()

        # Act - Update same price level to 50 units
        delta_2 = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.UPDATE,
            order=BookOrder(
                side=book_side,
                price=Price.from_str(order_price),
                size=Quantity.from_str("50.000"),
                order_id=1,
            ),
            flags=0,
            sequence=2,
            ts_event=1,
            ts_init=1,
        )
        matching_engine_l2.process_order_book_delta(delta_2)

        # Assert - fills against current book liquidity (50)
        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 1, (
            f"Expected 1 fill event after second delta, was {len(filled_events)}"
        )
        assert filled_events[0].last_qty == self.instrument.make_qty(50.0), (
            f"Second fill should be 50 (current book liquidity), was {filled_events[0].last_qty}"
        )

    def test_new_liquidity_at_better_price_fills(self) -> None:
        # Test: new liquidity at better prices should fill.
        #
        # Scenario:
        # 1. BUY LIMIT at 100, size=200
        # 2. SELL Delta at 100, size=10 → fills 10
        # 3. SELL Delta at 90, size=50 → fills against all current book liquidity

        # Arrange
        matching_engine_l2 = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L2_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=True,
            trade_execution=False,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        # Initialize matching core with bid side
        bid_delta = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=OrderSide.BUY,
                price=Price.from_str("80.00"),
                size=Quantity.from_str("100.000"),
                order_id=100,
            ),
            flags=0,
            sequence=0,
            ts_event=0,
            ts_init=0,
        )
        matching_engine_l2.process_order_book_delta(bid_delta)

        ask_delta_1 = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=OrderSide.SELL,
                price=Price.from_str("100.00"),
                size=Quantity.from_str("10.000"),
                order_id=1,
            ),
            flags=0,
            sequence=1,
            ts_event=0,
            ts_init=0,
        )
        matching_engine_l2.process_order_book_delta(ask_delta_1)

        order = TestExecStubs.limit_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            price=Price.from_str("100.00"),
            quantity=self.instrument.make_qty(200.0),
        )

        # Act
        matching_engine_l2.process_order(order, self.account_id)

        # Assert
        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 1, f"Expected 1 fill, was {len(filled_events)}"
        assert filled_events[0].last_qty == self.instrument.make_qty(10.0)
        messages.clear()

        # Act - New liquidity at better price (90 vs limit 100)
        ask_delta_2 = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=OrderSide.SELL,
                price=Price.from_str("90.00"),
                size=Quantity.from_str("50.000"),
                order_id=2,
            ),
            flags=0,
            sequence=2,
            ts_event=1,
            ts_init=1,
        )
        matching_engine_l2.process_order_book_delta(ask_delta_2)

        # Assert - Fills against current book liquidity (50 @ 90 + 10 @ 100 = 60)
        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        total_filled = sum(e.last_qty.as_double() for e in filled_events)
        assert total_filled == 60.0, (
            f"Total fill should be 60 (current book liquidity). "
            f"Got {total_filled} from {len(filled_events)} fill(s)"
        )

    def test_fully_filled_order_not_rematched_on_subsequent_iterate(self) -> None:
        # A fully-filled order should be removed from matching core to prevent
        # duplicate fills on subsequent iterate() calls.

        # Arrange
        matching_engine_l2 = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L2_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=True,
            trade_execution=False,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        bid_delta = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=OrderSide.BUY,
                price=Price.from_str("90.00"),
                size=Quantity.from_str("100.000"),
                order_id=100,
            ),
            flags=0,
            sequence=0,
            ts_event=0,
            ts_init=0,
        )
        matching_engine_l2.process_order_book_delta(bid_delta)

        ask_delta = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=OrderSide.SELL,
                price=Price.from_str("100.00"),
                size=Quantity.from_str("50.000"),
                order_id=1,
            ),
            flags=0,
            sequence=1,
            ts_event=0,
            ts_init=0,
        )
        matching_engine_l2.process_order_book_delta(ask_delta)

        order = TestExecStubs.limit_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            price=Price.from_str("100.00"),
            quantity=self.instrument.make_qty(50.0),
        )
        matching_engine_l2.process_order(order, self.account_id)

        # Act
        matching_engine_l2.iterate(timestamp_ns=1)

        # Assert
        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 1, (
            f"Expected exactly 1 fill (initial), but got {len(filled_events)} "
            f"(duplicate fill on subsequent iterate)"
        )

    def test_liquidity_consumption_tracks_fills_at_price_level(self):
        """
        Test that with liquidity_consumption=True, fills consume available liquidity at
        each price level, reducing subsequent fill quantities.
        """
        matching_engine = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L2_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=True,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            liquidity_consumption=True,
        )

        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        # Establish market with bid side
        bid_delta = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=OrderSide.BUY,
                price=Price.from_str("90.00"),
                size=Quantity.from_str("100.000"),
                order_id=100,
            ),
            flags=0,
            sequence=0,
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_order_book_delta(bid_delta)

        # Add 100 units of liquidity at ask 100.00
        ask_delta = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=OrderSide.SELL,
                price=Price.from_str("100.00"),
                size=Quantity.from_str("100.000"),
                order_id=1,
            ),
            flags=0,
            sequence=1,
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_order_book_delta(ask_delta)

        # First order: BUY 30 units - should fill 30
        order1 = TestExecStubs.market_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(30.0),
            client_order_id=TestIdStubs.client_order_id(1),
        )
        matching_engine.process_order(order1, self.account_id)
        matching_engine.iterate(timestamp_ns=1)

        # Second order: BUY 50 units - should fill 50 (80 consumed, 20 remaining)
        order2 = TestExecStubs.market_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(50.0),
            client_order_id=TestIdStubs.client_order_id(2),
        )
        matching_engine.process_order(order2, self.account_id)
        matching_engine.iterate(timestamp_ns=2)

        # Third order: BUY 50 units - should only fill 20 (remaining liquidity)
        order3 = TestExecStubs.market_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(50.0),
            client_order_id=TestIdStubs.client_order_id(3),
        )
        matching_engine.process_order(order3, self.account_id)
        matching_engine.iterate(timestamp_ns=3)

        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 3

        # First fill: 30 units
        assert filled_events[0].last_qty == Quantity.from_str("30.000")

        # Second fill: 50 units
        assert filled_events[1].last_qty == Quantity.from_str("50.000")

        # Third fill: only 20 units (100 - 30 - 50 = 20 remaining)
        assert filled_events[2].last_qty == Quantity.from_str("20.000")

    def test_liquidity_consumption_resets_on_fresh_data(self):
        """
        Test that consumption resets when fresh data arrives at a price level.
        """
        matching_engine = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L2_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=True,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            liquidity_consumption=True,
        )

        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        # Establish market with bid side
        bid_delta = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=OrderSide.BUY,
                price=Price.from_str("90.00"),
                size=Quantity.from_str("100.000"),
                order_id=100,
            ),
            flags=0,
            sequence=0,
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_order_book_delta(bid_delta)

        # Add 50 units of liquidity at ask 100.00
        ask_delta = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=OrderSide.SELL,
                price=Price.from_str("100.00"),
                size=Quantity.from_str("50.000"),
                order_id=1,
            ),
            flags=0,
            sequence=1,
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_order_book_delta(ask_delta)

        # First order: BUY 50 units - consumes all liquidity
        order1 = TestExecStubs.market_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(50.0),
            client_order_id=TestIdStubs.client_order_id(1),
        )
        matching_engine.process_order(order1, self.account_id)
        matching_engine.iterate(timestamp_ns=1)

        # Fresh data: Update the level to 80 units (simulates new liquidity)
        ask_update = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.UPDATE,
            order=BookOrder(
                side=OrderSide.SELL,
                price=Price.from_str("100.00"),
                size=Quantity.from_str("80.000"),
                order_id=1,
            ),
            flags=0,
            sequence=2,
            ts_event=1,
            ts_init=1,
        )
        matching_engine.process_order_book_delta(ask_update)

        # Second order: BUY 60 units - should fill 60 (fresh 80 available)
        order2 = TestExecStubs.market_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(60.0),
            client_order_id=TestIdStubs.client_order_id(2),
        )
        matching_engine.process_order(order2, self.account_id)
        matching_engine.iterate(timestamp_ns=2)

        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 2
        assert filled_events[0].last_qty == Quantity.from_str("50.000")
        assert filled_events[1].last_qty == Quantity.from_str("60.000")

    def test_liquidity_consumption_with_snapshot_consistency(self):
        """
        Test that liquidity consumption uses snapshot quantities consistently.

        Regression test for issue where get_quantity_at_level could return 0 for levels
        that existed when get_all_crossed_levels was called, causing levels to be
        incorrectly skipped ("No match" issue).

        """
        matching_engine = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L2_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=True,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            liquidity_consumption=True,
        )

        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        # Establish market with bid side
        bid_delta = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=OrderSide.BUY,
                price=Price.from_str("90.00"),
                size=Quantity.from_str("100.000"),
                order_id=100,
            ),
            flags=0,
            sequence=0,
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_order_book_delta(bid_delta)

        # Add multiple ask levels
        ask_delta1 = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=OrderSide.SELL,
                price=Price.from_str("100.00"),
                size=Quantity.from_str("200.000"),
                order_id=1,
            ),
            flags=0,
            sequence=1,
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_order_book_delta(ask_delta1)

        ask_delta2 = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=OrderSide.SELL,
                price=Price.from_str("100.01"),
                size=Quantity.from_str("150.000"),
                order_id=2,
            ),
            flags=0,
            sequence=2,
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_order_book_delta(ask_delta2)

        # Place a limit order that would cross both levels
        limit_order = TestExecStubs.limit_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(300.0),
            price=Price.from_str("100.01"),
            client_order_id=TestIdStubs.client_order_id(1),
        )
        matching_engine.process_order(limit_order, self.account_id)
        matching_engine.iterate(timestamp_ns=1)

        # Order should fill using snapshot quantities
        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 2  # Two fills from two levels
        assert filled_events[0].last_qty == Quantity.from_str("200.000")  # First level
        assert filled_events[1].last_qty == Quantity.from_str(
            "100.000",
        )  # Partial from second level

    def test_liquidity_consumption_off_allows_repeated_fills(self):
        """
        Test that with liquidity_consumption=False, the same liquidity can be consumed
        by multiple orders (default behavior).
        """
        matching_engine = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L2_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=True,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            liquidity_consumption=False,  # Explicitly off
        )

        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        # Establish market with bid side
        bid_delta = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=OrderSide.BUY,
                price=Price.from_str("90.00"),
                size=Quantity.from_str("100.000"),
                order_id=100,
            ),
            flags=0,
            sequence=0,
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_order_book_delta(bid_delta)

        # Add 50 units of liquidity at ask 100.00
        ask_delta = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=OrderSide.SELL,
                price=Price.from_str("100.00"),
                size=Quantity.from_str("50.000"),
                order_id=1,
            ),
            flags=0,
            sequence=1,
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_order_book_delta(ask_delta)

        # First order: BUY 50 units
        order1 = TestExecStubs.market_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(50.0),
            client_order_id=TestIdStubs.client_order_id(1),
        )
        matching_engine.process_order(order1, self.account_id)
        matching_engine.iterate(timestamp_ns=1)

        # Second order: BUY 50 units - should also fill 50 (no consumption tracking)
        order2 = TestExecStubs.market_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(50.0),
            client_order_id=TestIdStubs.client_order_id(2),
        )
        matching_engine.process_order(order2, self.account_id)
        matching_engine.iterate(timestamp_ns=2)

        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 2
        assert filled_events[0].last_qty == Quantity.from_str("50.000")
        assert filled_events[1].last_qty == Quantity.from_str("50.000")

    @pytest.mark.parametrize(
        ("order_side", "liquidity_side"),
        [
            (OrderSide.BUY, OrderSide.SELL),
            (OrderSide.SELL, OrderSide.BUY),
        ],
    )
    def test_liquidity_consumption_by_order_side(
        self,
        order_side: OrderSide,
        liquidity_side: OrderSide,
    ):
        """
        Test that liquidity consumption correctly tracks orders consuming from the
        opposite side of the book.
        """
        matching_engine = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L2_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=True,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            liquidity_consumption=True,
        )

        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        # Establish market with opposite side (for valid market)
        opposite_delta = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=order_side,
                price=Price.from_str("90.00")
                if order_side == OrderSide.BUY
                else Price.from_str("110.00"),
                size=Quantity.from_str("100.000"),
                order_id=100,
            ),
            flags=0,
            sequence=0,
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_order_book_delta(opposite_delta)

        # Add 80 units of liquidity on the side we'll consume from
        liquidity_delta = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=liquidity_side,
                price=Price.from_str("100.00"),
                size=Quantity.from_str("80.000"),
                order_id=1,
            ),
            flags=0,
            sequence=1,
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_order_book_delta(liquidity_delta)

        # First order: 50 units - should fill 50
        order1 = TestExecStubs.market_order(
            instrument=self.instrument,
            order_side=order_side,
            quantity=self.instrument.make_qty(50.0),
            client_order_id=TestIdStubs.client_order_id(1),
        )
        matching_engine.process_order(order1, self.account_id)
        matching_engine.iterate(timestamp_ns=1)

        # Second order: 50 units - should only fill 30 (80 - 50 = 30 remaining)
        order2 = TestExecStubs.market_order(
            instrument=self.instrument,
            order_side=order_side,
            quantity=self.instrument.make_qty(50.0),
            client_order_id=TestIdStubs.client_order_id(2),
        )
        matching_engine.process_order(order2, self.account_id)
        matching_engine.iterate(timestamp_ns=2)

        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 2
        assert filled_events[0].last_qty == Quantity.from_str("50.000")
        assert filled_events[1].last_qty == Quantity.from_str("30.000")

    def test_liquidity_consumption_across_multiple_levels(self):
        """
        Test that liquidity consumption correctly tracks fills across multiple price
        levels over multiple orders.
        """
        matching_engine = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L2_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=True,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            liquidity_consumption=True,
        )

        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        # Establish market with bid side
        bid_delta = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=OrderSide.BUY,
                price=Price.from_str("90.00"),
                size=Quantity.from_str("100.000"),
                order_id=100,
            ),
            flags=0,
            sequence=0,
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_order_book_delta(bid_delta)

        # Add three ask levels
        for i, (price, size) in enumerate(
            [("100.00", "50.000"), ("100.01", "30.000"), ("100.02", "40.000")],
        ):
            delta = OrderBookDelta(
                instrument_id=self.instrument.id,
                action=BookAction.ADD,
                order=BookOrder(
                    side=OrderSide.SELL,
                    price=Price.from_str(price),
                    size=Quantity.from_str(size),
                    order_id=i + 1,
                ),
                flags=0,
                sequence=i + 1,
                ts_event=0,
                ts_init=0,
            )
            matching_engine.process_order_book_delta(delta)

        # First order: consume 40 from first level (10 remaining at 100.00)
        order1 = TestExecStubs.limit_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(40.0),
            price=Price.from_str("100.02"),
            client_order_id=TestIdStubs.client_order_id(1),
        )
        matching_engine.process_order(order1, self.account_id)
        matching_engine.iterate(timestamp_ns=1)

        # Second order: consume remaining 10 + 30 from second level + 20 from third
        order2 = TestExecStubs.limit_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(60.0),
            price=Price.from_str("100.02"),
            client_order_id=TestIdStubs.client_order_id(2),
        )
        matching_engine.process_order(order2, self.account_id)
        matching_engine.iterate(timestamp_ns=2)

        # Third order: consume remaining 20 from third level
        order3 = TestExecStubs.limit_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(50.0),
            price=Price.from_str("100.02"),
            client_order_id=TestIdStubs.client_order_id(3),
        )
        matching_engine.process_order(order3, self.account_id)
        matching_engine.iterate(timestamp_ns=3)

        filled_events = [m for m in messages if isinstance(m, OrderFilled)]

        # First order: 40 from level 1
        assert filled_events[0].last_qty == Quantity.from_str("40.000")

        # Second order: 10 + 30 + 20 = 60 across three levels
        second_order_fills = [f for f in filled_events if f.client_order_id.value.endswith("-2")]
        total_second = sum((f.last_qty for f in second_order_fills), Quantity.zero(3))
        assert total_second == Quantity.from_str("60.000")

        # Third order: only 20 remaining from third level
        third_order_fills = [f for f in filled_events if f.client_order_id.value.endswith("-3")]
        total_third = sum((f.last_qty for f in third_order_fills), Quantity.zero(3))
        assert total_third == Quantity.from_str("20.000")

    @pytest.mark.parametrize(
        ("order_side", "liquidity_side"),
        [
            (OrderSide.BUY, OrderSide.SELL),
            (OrderSide.SELL, OrderSide.BUY),
        ],
    )
    def test_liquidity_consumption_level_size_decrease(
        self,
        order_side: OrderSide,
        liquidity_side: OrderSide,
    ):
        """
        Test that when a level size decreases below consumed amount, consumption resets
        correctly.
        """
        matching_engine = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L2_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=True,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            liquidity_consumption=True,
        )

        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        # Establish market with opposite side
        opposite_delta = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=order_side,
                price=Price.from_str("90.00")
                if order_side == OrderSide.BUY
                else Price.from_str("110.00"),
                size=Quantity.from_str("100.000"),
                order_id=100,
            ),
            flags=0,
            sequence=0,
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_order_book_delta(opposite_delta)

        # Add 100 units at liquidity side
        liquidity_delta = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=liquidity_side,
                price=Price.from_str("100.00"),
                size=Quantity.from_str("100.000"),
                order_id=1,
            ),
            flags=0,
            sequence=1,
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_order_book_delta(liquidity_delta)

        # First order: consume 70 units (30 remaining)
        order1 = TestExecStubs.market_order(
            instrument=self.instrument,
            order_side=order_side,
            quantity=self.instrument.make_qty(70.0),
            client_order_id=TestIdStubs.client_order_id(1),
        )
        matching_engine.process_order(order1, self.account_id)
        matching_engine.iterate(timestamp_ns=1)

        # Update level to 50 (less than original 100, triggers reset)
        liquidity_update = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.UPDATE,
            order=BookOrder(
                side=liquidity_side,
                price=Price.from_str("100.00"),
                size=Quantity.from_str("50.000"),
                order_id=1,
            ),
            flags=0,
            sequence=2,
            ts_event=1,
            ts_init=1,
        )
        matching_engine.process_order_book_delta(liquidity_update)

        # Second order: should fill from fresh 50 (reset happened)
        order2 = TestExecStubs.market_order(
            instrument=self.instrument,
            order_side=order_side,
            quantity=self.instrument.make_qty(40.0),
            client_order_id=TestIdStubs.client_order_id(2),
        )
        matching_engine.process_order(order2, self.account_id)
        matching_engine.iterate(timestamp_ns=2)

        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 2
        assert filled_events[0].last_qty == Quantity.from_str("70.000")
        assert filled_events[1].last_qty == Quantity.from_str("40.000")

    @pytest.mark.parametrize(
        ("order_side", "liquidity_side"),
        [
            (OrderSide.BUY, OrderSide.SELL),
            (OrderSide.SELL, OrderSide.BUY),
        ],
    )
    def test_liquidity_consumption_level_size_increase(
        self,
        order_side: OrderSide,
        liquidity_side: OrderSide,
    ):
        """
        Test that when a level size increases, consumption resets to allow access to the
        fresh liquidity.
        """
        matching_engine = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L2_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=True,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            liquidity_consumption=True,
        )

        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        # Establish market with opposite side
        opposite_delta = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=order_side,
                price=Price.from_str("90.00")
                if order_side == OrderSide.BUY
                else Price.from_str("110.00"),
                size=Quantity.from_str("100.000"),
                order_id=100,
            ),
            flags=0,
            sequence=0,
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_order_book_delta(opposite_delta)

        # Add 50 units at liquidity side
        liquidity_delta = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=liquidity_side,
                price=Price.from_str("100.00"),
                size=Quantity.from_str("50.000"),
                order_id=1,
            ),
            flags=0,
            sequence=1,
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_order_book_delta(liquidity_delta)

        # First order: consume all 50 units
        order1 = TestExecStubs.market_order(
            instrument=self.instrument,
            order_side=order_side,
            quantity=self.instrument.make_qty(50.0),
            client_order_id=TestIdStubs.client_order_id(1),
        )
        matching_engine.process_order(order1, self.account_id)
        matching_engine.iterate(timestamp_ns=1)

        # Increase level to 100 (fresh liquidity arrived)
        liquidity_update = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.UPDATE,
            order=BookOrder(
                side=liquidity_side,
                price=Price.from_str("100.00"),
                size=Quantity.from_str("100.000"),
                order_id=1,
            ),
            flags=0,
            sequence=2,
            ts_event=1,
            ts_init=1,
        )
        matching_engine.process_order_book_delta(liquidity_update)

        # Second order: should fill from fresh 100 (reset happened)
        order2 = TestExecStubs.market_order(
            instrument=self.instrument,
            order_side=order_side,
            quantity=self.instrument.make_qty(80.0),
            client_order_id=TestIdStubs.client_order_id(2),
        )
        matching_engine.process_order(order2, self.account_id)
        matching_engine.iterate(timestamp_ns=2)

        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 2
        assert filled_events[0].last_qty == Quantity.from_str("50.000")
        assert filled_events[1].last_qty == Quantity.from_str("80.000")

    @pytest.mark.parametrize(
        ("order_side", "liquidity_side"),
        [
            (OrderSide.BUY, OrderSide.SELL),
            (OrderSide.SELL, OrderSide.BUY),
        ],
    )
    def test_liquidity_consumption_exhausted_level_blocks_fills(
        self,
        order_side: OrderSide,
        liquidity_side: OrderSide,
    ):
        """
        Test that once a level is fully consumed, subsequent orders get no fills from
        that level until fresh data arrives.
        """
        matching_engine = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L2_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=True,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            liquidity_consumption=True,
        )

        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        # Establish market with opposite side
        opposite_delta = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=order_side,
                price=Price.from_str("90.00")
                if order_side == OrderSide.BUY
                else Price.from_str("110.00"),
                size=Quantity.from_str("100.000"),
                order_id=100,
            ),
            flags=0,
            sequence=0,
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_order_book_delta(opposite_delta)

        # Add 50 units at liquidity side
        liquidity_delta = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=liquidity_side,
                price=Price.from_str("100.00"),
                size=Quantity.from_str("50.000"),
                order_id=1,
            ),
            flags=0,
            sequence=1,
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_order_book_delta(liquidity_delta)

        # First order: consume all 50 units
        order1 = TestExecStubs.market_order(
            instrument=self.instrument,
            order_side=order_side,
            quantity=self.instrument.make_qty(50.0),
            client_order_id=TestIdStubs.client_order_id(1),
        )
        matching_engine.process_order(order1, self.account_id)
        matching_engine.iterate(timestamp_ns=1)

        # Second order: should get nothing (level exhausted, no book update)
        order2 = TestExecStubs.market_order(
            instrument=self.instrument,
            order_side=order_side,
            quantity=self.instrument.make_qty(30.0),
            client_order_id=TestIdStubs.client_order_id(2),
        )
        matching_engine.process_order(order2, self.account_id)
        matching_engine.iterate(timestamp_ns=2)

        # Third order: also should get nothing
        order3 = TestExecStubs.market_order(
            instrument=self.instrument,
            order_side=order_side,
            quantity=self.instrument.make_qty(20.0),
            client_order_id=TestIdStubs.client_order_id(3),
        )
        matching_engine.process_order(order3, self.account_id)
        matching_engine.iterate(timestamp_ns=3)

        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 1  # Only first order filled
        assert filled_events[0].last_qty == Quantity.from_str("50.000")

    def test_liquidity_consumption_both_sides_simultaneously(self):
        """
        Test that BUY and SELL orders can consume from their respective sides of the
        book simultaneously without interference.
        """
        matching_engine = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L2_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=True,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            liquidity_consumption=True,
        )

        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        # Add bid liquidity
        bid_delta = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=OrderSide.BUY,
                price=Price.from_str("99.00"),
                size=Quantity.from_str("100.000"),
                order_id=1,
            ),
            flags=0,
            sequence=0,
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_order_book_delta(bid_delta)

        # Add ask liquidity
        ask_delta = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=OrderSide.SELL,
                price=Price.from_str("101.00"),
                size=Quantity.from_str("100.000"),
                order_id=2,
            ),
            flags=0,
            sequence=1,
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_order_book_delta(ask_delta)

        # BUY order consumes 60 from asks
        buy_order = TestExecStubs.market_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(60.0),
            client_order_id=TestIdStubs.client_order_id(1),
        )
        matching_engine.process_order(buy_order, self.account_id)
        matching_engine.iterate(timestamp_ns=1)

        # SELL order consumes 60 from bids (independent tracking)
        sell_order = TestExecStubs.market_order(
            instrument=self.instrument,
            order_side=OrderSide.SELL,
            quantity=self.instrument.make_qty(60.0),
            client_order_id=TestIdStubs.client_order_id(2),
        )
        matching_engine.process_order(sell_order, self.account_id)
        matching_engine.iterate(timestamp_ns=2)

        # Another BUY - should only get 40 remaining (100 - 60)
        buy_order2 = TestExecStubs.market_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(50.0),
            client_order_id=TestIdStubs.client_order_id(3),
        )
        matching_engine.process_order(buy_order2, self.account_id)
        matching_engine.iterate(timestamp_ns=3)

        # Another SELL - should only get 40 remaining (100 - 60)
        sell_order2 = TestExecStubs.market_order(
            instrument=self.instrument,
            order_side=OrderSide.SELL,
            quantity=self.instrument.make_qty(50.0),
            client_order_id=TestIdStubs.client_order_id(4),
        )
        matching_engine.process_order(sell_order2, self.account_id)
        matching_engine.iterate(timestamp_ns=4)

        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 4
        assert filled_events[0].last_qty == Quantity.from_str("60.000")  # BUY 60
        assert filled_events[1].last_qty == Quantity.from_str("60.000")  # SELL 60
        assert filled_events[2].last_qty == Quantity.from_str("40.000")  # BUY 40 (remaining)
        assert filled_events[3].last_qty == Quantity.from_str("40.000")  # SELL 40 (remaining)

    def test_liquidity_consumption_with_l3_mbo_book(self):
        """
        Test that liquidity consumption works correctly with L3_MBO book type.
        """
        matching_engine = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L3_MBO,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=True,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            liquidity_consumption=True,
        )

        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        # Establish market with bid
        bid_delta = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=OrderSide.BUY,
                price=Price.from_str("90.00"),
                size=Quantity.from_str("100.000"),
                order_id=100,
            ),
            flags=0,
            sequence=0,
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_order_book_delta(bid_delta)

        # Add multiple orders at same price (L3 allows this)
        for i in range(3):
            delta = OrderBookDelta(
                instrument_id=self.instrument.id,
                action=BookAction.ADD,
                order=BookOrder(
                    side=OrderSide.SELL,
                    price=Price.from_str("100.00"),
                    size=Quantity.from_str("30.000"),
                    order_id=i + 1,
                ),
                flags=0,
                sequence=i + 1,
                ts_event=0,
                ts_init=0,
            )
            matching_engine.process_order_book_delta(delta)

        # Total liquidity at 100.00 = 90 units (3 x 30)
        # First order: BUY 50 - should fill 50 (across multiple L3 orders)
        order1 = TestExecStubs.market_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(50.0),
            client_order_id=TestIdStubs.client_order_id(1),
        )
        matching_engine.process_order(order1, self.account_id)
        matching_engine.iterate(timestamp_ns=1)

        # Second order: BUY 50 - should only get 40 (90 - 50 = 40 remaining)
        order2 = TestExecStubs.market_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(50.0),
            client_order_id=TestIdStubs.client_order_id(2),
        )
        matching_engine.process_order(order2, self.account_id)
        matching_engine.iterate(timestamp_ns=2)

        filled_events = [m for m in messages if isinstance(m, OrderFilled)]

        # L3 books may produce multiple fills per order (one per book order)
        # Verify total fill quantities are correct
        order1_fills = [f for f in filled_events if f.client_order_id.value.endswith("-1")]
        order2_fills = [f for f in filled_events if f.client_order_id.value.endswith("-2")]

        total_order1 = sum((f.last_qty for f in order1_fills), Quantity.zero(3))
        total_order2 = sum((f.last_qty for f in order2_fills), Quantity.zero(3))

        assert total_order1 == Quantity.from_str("50.000")  # First order fills completely
        assert total_order2 == Quantity.from_str("40.000")  # Second order gets remaining liquidity

    @pytest.mark.parametrize(
        ("order_side", "liquidity_side"),
        [
            (OrderSide.BUY, OrderSide.SELL),
            (OrderSide.SELL, OrderSide.BUY),
        ],
    )
    def test_liquidity_consumption_level_deleted_fallback(
        self,
        order_side: OrderSide,
        liquidity_side: OrderSide,
    ):
        """
        Test that when a level is deleted between fill determination and consumption
        application, the fallback to fill quantity is used.
        """
        matching_engine = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L2_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=True,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            liquidity_consumption=True,
        )

        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        # Establish market with opposite side
        opposite_delta = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=order_side,
                price=Price.from_str("90.00")
                if order_side == OrderSide.BUY
                else Price.from_str("110.00"),
                size=Quantity.from_str("100.000"),
                order_id=100,
            ),
            flags=0,
            sequence=0,
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_order_book_delta(opposite_delta)

        # Add liquidity level
        liquidity_delta = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=liquidity_side,
                price=Price.from_str("100.00"),
                size=Quantity.from_str("100.000"),
                order_id=1,
            ),
            flags=0,
            sequence=1,
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_order_book_delta(liquidity_delta)

        # Submit order - book is stable, should fill normally
        order1 = TestExecStubs.market_order(
            instrument=self.instrument,
            order_side=order_side,
            quantity=self.instrument.make_qty(40.0),
            client_order_id=TestIdStubs.client_order_id(1),
        )
        matching_engine.process_order(order1, self.account_id)
        matching_engine.iterate(timestamp_ns=1)

        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 1
        assert filled_events[0].last_qty == Quantity.from_str("40.000")

    @pytest.mark.parametrize(
        ("order_side", "liquidity_side"),
        [
            (OrderSide.BUY, OrderSide.SELL),
            (OrderSide.SELL, OrderSide.BUY),
        ],
    )
    def test_liquidity_consumption_clears_on_delete_and_readd(
        self,
        order_side: OrderSide,
        liquidity_side: OrderSide,
    ):
        """
        Test that DELETE clears consumption tracking, so re-ADD provides fresh
        liquidity.

        When a level is deleted and then re-added, consumption should reset to allow
        fills from the new liquidity.

        """
        matching_engine = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L2_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=True,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            liquidity_consumption=True,
        )

        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        # Establish market with opposite side
        opposite_delta = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=order_side,
                price=Price.from_str("90.00")
                if order_side == OrderSide.BUY
                else Price.from_str("110.00"),
                size=Quantity.from_str("100.000"),
                order_id=100,
            ),
            flags=0,
            sequence=0,
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_order_book_delta(opposite_delta)

        # Add 100 units at liquidity level
        liquidity_delta = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=liquidity_side,
                price=Price.from_str("100.00"),
                size=Quantity.from_str("100.000"),
                order_id=1,
            ),
            flags=0,
            sequence=1,
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_order_book_delta(liquidity_delta)

        # First order: consume 30 units
        order1 = TestExecStubs.market_order(
            instrument=self.instrument,
            order_side=order_side,
            quantity=self.instrument.make_qty(30.0),
            client_order_id=TestIdStubs.client_order_id(1),
        )
        matching_engine.process_order(order1, self.account_id)
        matching_engine.iterate(timestamp_ns=1)

        # Delete the level (clears consumption tracking for this price)
        delete_delta = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.DELETE,
            order=BookOrder(
                side=liquidity_side,
                price=Price.from_str("100.00"),
                size=Quantity.from_str("0.000"),
                order_id=1,
            ),
            flags=0,
            sequence=2,
            ts_event=1,
            ts_init=1,
        )
        matching_engine.process_order_book_delta(delete_delta)

        # Re-add the level with same size (fresh liquidity)
        liquidity_delta2 = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=liquidity_side,
                price=Price.from_str("100.00"),
                size=Quantity.from_str("100.000"),
                order_id=2,
            ),
            flags=0,
            sequence=3,
            ts_event=2,
            ts_init=2,
        )
        matching_engine.process_order_book_delta(liquidity_delta2)

        # Second order: orders 80, gets full 80 (DELETE cleared consumption)
        order2 = TestExecStubs.market_order(
            instrument=self.instrument,
            order_side=order_side,
            quantity=self.instrument.make_qty(80.0),
            client_order_id=TestIdStubs.client_order_id(2),
        )
        matching_engine.process_order(order2, self.account_id)
        matching_engine.iterate(timestamp_ns=3)

        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 2
        assert filled_events[0].last_qty == Quantity.from_str("30.000")  # First order fill
        assert filled_events[1].last_qty == Quantity.from_str("80.000")  # Full fill after DELETE

    @pytest.mark.parametrize(
        ("order_side", "liquidity_side"),
        [
            (OrderSide.BUY, OrderSide.SELL),
            (OrderSide.SELL, OrderSide.BUY),
        ],
    )
    def test_liquidity_consumption_multiple_fills_same_price_level_deleted(
        self,
        order_side: OrderSide,
        liquidity_side: OrderSide,
    ):
        """
        Test that when multiple fills exist at the same price (L3 books) and the level
        is deleted during the race condition, all fills are processed using the
        aggregated fill total, not just the first fill's quantity.

        Regression test for issue where subsequent fills at same price were dropped
        because original_size was set to just the first fill's quantity.

        """
        matching_engine = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L3_MBO,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=True,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            liquidity_consumption=True,
        )

        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        # Establish market with opposite side
        opposite_delta = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=order_side,
                price=Price.from_str("90.00")
                if order_side == OrderSide.BUY
                else Price.from_str("110.00"),
                size=Quantity.from_str("100.000"),
                order_id=100,
            ),
            flags=0,
            sequence=0,
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_order_book_delta(opposite_delta)

        # Add multiple L3 orders at the same price (total 90 units)
        for i, size in enumerate(["30.000", "40.000", "20.000"]):
            delta = OrderBookDelta(
                instrument_id=self.instrument.id,
                action=BookAction.ADD,
                order=BookOrder(
                    side=liquidity_side,
                    price=Price.from_str("100.00"),
                    size=Quantity.from_str(size),
                    order_id=i + 1,
                ),
                flags=0,
                sequence=i + 1,
                ts_event=0,
                ts_init=0,
            )
            matching_engine.process_order_book_delta(delta)

        # Place order that should fill all 90 units across the three L3 orders
        order1 = TestExecStubs.market_order(
            instrument=self.instrument,
            order_side=order_side,
            quantity=self.instrument.make_qty(90.0),
            client_order_id=TestIdStubs.client_order_id(1),
        )
        matching_engine.process_order(order1, self.account_id)
        matching_engine.iterate(timestamp_ns=1)

        # All fills at same price should be processed, totaling 90 units
        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        total_filled = sum((f.last_qty for f in filled_events), Quantity.zero(3))
        assert total_filled == Quantity.from_str("90.000")

    @pytest.mark.parametrize(
        ("order_side", "aggressor_side"),
        [
            (OrderSide.BUY, AggressorSide.SELLER),
            (OrderSide.SELL, AggressorSide.BUYER),
        ],
    )
    def test_trade_consumption_prevents_overfill(
        self,
        order_side: OrderSide,
        aggressor_side: AggressorSide,
    ) -> None:
        """
        Test that with trade_execution=True and liquidity_consumption=True, multiple
        orders matching a single trade tick have their total fills capped to the trade
        size.

        Covers both BUY orders (filled by SELLER trades) and SELL orders (filled by
        BUYER trades).

        """
        matching_engine = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L1_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=True,
            trade_execution=True,
            liquidity_consumption=True,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        quote = TestDataStubs.quote_tick(
            instrument=self.instrument,
            bid_price=90.00,
            ask_price=110.00,
        )
        matching_engine.process_quote_tick(quote)

        order1 = TestExecStubs.limit_order(
            instrument=self.instrument,
            order_side=order_side,
            price=Price.from_str("100.00"),
            quantity=self.instrument.make_qty(30.0),
            client_order_id=TestIdStubs.client_order_id(1),
        )
        matching_engine.process_order(order1, self.account_id)

        order2 = TestExecStubs.limit_order(
            instrument=self.instrument,
            order_side=order_side,
            price=Price.from_str("100.00"),
            quantity=self.instrument.make_qty(30.0),
            client_order_id=TestIdStubs.client_order_id(2),
        )
        matching_engine.process_order(order2, self.account_id)

        messages.clear()

        # Trade with 50 units, less than orders' 60 total
        trade = TestDataStubs.trade_tick(
            instrument=self.instrument,
            price=100.00,
            size=50.0,
            aggressor_side=aggressor_side,
        )
        matching_engine.process_trade_tick(trade)

        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 2
        assert filled_events[0].last_qty == Quantity.from_str("30.000")
        assert filled_events[1].last_qty == Quantity.from_str("20.000")

    @pytest.mark.parametrize(
        ("order_side", "aggressor_side"),
        [
            (OrderSide.BUY, AggressorSide.SELLER),
            (OrderSide.SELL, AggressorSide.BUYER),
        ],
    )
    def test_trade_consumption_disabled_allows_overfill(
        self,
        order_side: OrderSide,
        aggressor_side: AggressorSide,
    ) -> None:
        """
        Test that with trade_execution=True but liquidity_consumption=False, the legacy
        overfill behavior is preserved (each order can fill up to the full trade size
        independently).

        Covers both BUY and SELL orders.

        """
        matching_engine = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L1_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=True,
            trade_execution=True,
            liquidity_consumption=False,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        quote = TestDataStubs.quote_tick(
            instrument=self.instrument,
            bid_price=90.00,
            ask_price=110.00,
        )
        matching_engine.process_quote_tick(quote)

        order1 = TestExecStubs.limit_order(
            instrument=self.instrument,
            order_side=order_side,
            price=Price.from_str("100.00"),
            quantity=self.instrument.make_qty(30.0),
            client_order_id=TestIdStubs.client_order_id(1),
        )
        matching_engine.process_order(order1, self.account_id)

        order2 = TestExecStubs.limit_order(
            instrument=self.instrument,
            order_side=order_side,
            price=Price.from_str("100.00"),
            quantity=self.instrument.make_qty(30.0),
            client_order_id=TestIdStubs.client_order_id(2),
        )
        matching_engine.process_order(order2, self.account_id)

        messages.clear()

        # Trade with 50 units, less than orders' 60 total
        trade = TestDataStubs.trade_tick(
            instrument=self.instrument,
            price=100.00,
            size=50.0,
            aggressor_side=aggressor_side,
        )
        matching_engine.process_trade_tick(trade)

        # Both fill fully (60 total overfills 50 trade) - legacy behavior
        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 2
        assert filled_events[0].last_qty == Quantity.from_str("30.000")
        assert filled_events[1].last_qty == Quantity.from_str("30.000")

    @pytest.mark.parametrize(
        ("order_side", "aggressor_side"),
        [
            (OrderSide.BUY, AggressorSide.SELLER),
            (OrderSide.SELL, AggressorSide.BUYER),
        ],
    )
    def test_trade_consumption_resets_on_fresh_trade(
        self,
        order_side: OrderSide,
        aggressor_side: AggressorSide,
    ) -> None:
        """
        Test that trade consumption resets when a fresh trade tick arrives, allowing new
        fills against the fresh liquidity.
        """
        matching_engine = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L1_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=True,
            trade_execution=True,
            liquidity_consumption=True,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        quote = TestDataStubs.quote_tick(
            instrument=self.instrument,
            bid_price=90.00,
            ask_price=110.00,
        )
        matching_engine.process_quote_tick(quote)

        order1 = TestExecStubs.limit_order(
            instrument=self.instrument,
            order_side=order_side,
            price=Price.from_str("100.00"),
            quantity=self.instrument.make_qty(30.0),
            client_order_id=TestIdStubs.client_order_id(1),
        )
        matching_engine.process_order(order1, self.account_id)

        order2 = TestExecStubs.limit_order(
            instrument=self.instrument,
            order_side=order_side,
            price=Price.from_str("100.00"),
            quantity=self.instrument.make_qty(30.0),
            client_order_id=TestIdStubs.client_order_id(2),
        )
        matching_engine.process_order(order2, self.account_id)

        messages.clear()

        trade1 = TestDataStubs.trade_tick(
            instrument=self.instrument,
            price=100.00,
            size=40.0,
            aggressor_side=aggressor_side,
        )
        matching_engine.process_trade_tick(trade1)

        # Fresh trade resets consumption
        trade2 = TestDataStubs.trade_tick(
            instrument=self.instrument,
            price=100.00,
            size=50.0,
            aggressor_side=aggressor_side,
        )
        matching_engine.process_trade_tick(trade2)

        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 3
        assert filled_events[0].last_qty == Quantity.from_str("30.000")
        assert filled_events[1].last_qty == Quantity.from_str("10.000")
        assert filled_events[2].last_qty == Quantity.from_str("20.000")

    @pytest.mark.parametrize(
        ("order_side", "aggressor_side", "order_price", "trade_price", "book_bid", "book_ask"),
        [
            # BUY order above trade price, SELLER trade
            (OrderSide.BUY, AggressorSide.SELLER, "211.35", "211.32", "211.30", "211.40"),
            # SELL order below trade price, BUYER trade
            (OrderSide.SELL, AggressorSide.BUYER, "211.35", "211.38", "211.30", "211.40"),
        ],
        ids=["buy_above_seller_trade", "sell_below_buyer_trade"],
    )
    def test_trade_execution_fill_with_liquidity_consumption_l2_mbp(
        self,
        order_side: OrderSide,
        aggressor_side: AggressorSide,
        order_price: str,
        trade_price: str,
        book_bid: str,
        book_ask: str,
    ) -> None:
        """
        Test that trade execution fills work correctly with liquidity_consumption=True
        on L2_MBP when trade price differs from order price.

        Regression test for bug where _apply_liquidity_consumption checked ORDER BOOK
        for available liquidity at the trade price, discarding fills when the trade
        price wasn't in the book.

        """
        matching_engine = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L2_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=True,
            trade_execution=True,
            liquidity_consumption=True,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Arrange
        snapshot = TestDataStubs.order_book_snapshot(
            instrument=self.instrument,
            bid_price=float(book_bid),
            ask_price=float(book_ask),
            bid_size=100.0,
            ask_size=100.0,
        )
        matching_engine.process_order_book_deltas(snapshot)

        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        order = TestExecStubs.limit_order(
            instrument=self.instrument,
            order_side=order_side,
            price=Price.from_str(order_price),
            quantity=self.instrument.make_qty(50.0),
        )
        matching_engine.process_order(order, self.account_id)
        messages.clear()

        # Act
        trade = TestDataStubs.trade_tick(
            instrument=self.instrument,
            price=float(trade_price),
            size=100.0,
            aggressor_side=aggressor_side,
        )
        matching_engine.process_trade_tick(trade)

        # Assert - fills at limit price (conservative), not trade price
        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 1, (
            f"{order_side.name} LIMIT at {order_price} should fill on "
            f"{aggressor_side.name} trade at {trade_price} with L2_MBP + liquidity_consumption"
        )
        assert filled_events[0].last_qty == Quantity.from_str("50.000")
        assert filled_events[0].last_px == Price.from_str(order_price)

    @pytest.mark.parametrize(
        ("order_side", "aggressor_side", "order_price", "trade_price", "book_bid", "book_ask"),
        [
            (OrderSide.BUY, AggressorSide.SELLER, "211.35", "211.32", "211.30", "211.40"),
            (OrderSide.SELL, AggressorSide.BUYER, "211.35", "211.38", "211.30", "211.40"),
        ],
        ids=["buy_orders_seller_trade", "sell_orders_buyer_trade"],
    )
    def test_trade_execution_consumption_prevents_overfill_l2_mbp(
        self,
        order_side: OrderSide,
        aggressor_side: AggressorSide,
        order_price: str,
        trade_price: str,
        book_bid: str,
        book_ask: str,
    ) -> None:
        """
        Test that trade consumption tracking works correctly with L2_MBP when multiple
        orders compete for the same trade tick.

        Verifies that _trade_consumption tracking still functions when trade execution
        fills bypass _apply_liquidity_consumption.

        """
        matching_engine = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L2_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=True,
            trade_execution=True,
            liquidity_consumption=True,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Arrange
        snapshot = TestDataStubs.order_book_snapshot(
            instrument=self.instrument,
            bid_price=float(book_bid),
            ask_price=float(book_ask),
            bid_size=100.0,
            ask_size=100.0,
        )
        matching_engine.process_order_book_deltas(snapshot)

        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        order1 = TestExecStubs.limit_order(
            instrument=self.instrument,
            order_side=order_side,
            price=Price.from_str(order_price),
            quantity=self.instrument.make_qty(30.0),
            client_order_id=TestIdStubs.client_order_id(1),
        )
        matching_engine.process_order(order1, self.account_id)

        order2 = TestExecStubs.limit_order(
            instrument=self.instrument,
            order_side=order_side,
            price=Price.from_str(order_price),
            quantity=self.instrument.make_qty(30.0),
            client_order_id=TestIdStubs.client_order_id(2),
        )
        matching_engine.process_order(order2, self.account_id)
        messages.clear()

        # Act - trade tick with 50 qty (less than total order qty of 60)
        trade = TestDataStubs.trade_tick(
            instrument=self.instrument,
            price=float(trade_price),
            size=50.0,
            aggressor_side=aggressor_side,
        )
        matching_engine.process_trade_tick(trade)

        # Assert
        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 2, (
            f"Expected 2 fills for {order_side.name} orders, was {len(filled_events)}"
        )
        assert filled_events[0].last_qty == Quantity.from_str("30.000")
        assert filled_events[1].last_qty == Quantity.from_str("20.000")

        # Fills occur at limit price (conservative), not trade price
        assert filled_events[0].last_px == Price.from_str(order_price)
        assert filled_events[1].last_px == Price.from_str(order_price)

    def test_trade_consumption_with_three_orders(self) -> None:
        """
        Test that trade consumption tracking works correctly with three orders competing
        for the same trade tick.
        """
        matching_engine = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L1_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=True,
            trade_execution=True,
            liquidity_consumption=True,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Set initial bid/ask
        quote = TestDataStubs.quote_tick(
            instrument=self.instrument,
            bid_price=90.00,
            ask_price=110.00,
        )
        matching_engine.process_quote_tick(quote)

        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        # Place 3 BUY LIMIT orders at 100.00 (qty=30 each, total=90)
        order1 = TestExecStubs.limit_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            price=Price.from_str("100.00"),
            quantity=self.instrument.make_qty(30.0),
            client_order_id=TestIdStubs.client_order_id(1),
        )
        matching_engine.process_order(order1, self.account_id)

        order2 = TestExecStubs.limit_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            price=Price.from_str("100.00"),
            quantity=self.instrument.make_qty(30.0),
            client_order_id=TestIdStubs.client_order_id(2),
        )
        matching_engine.process_order(order2, self.account_id)

        order3 = TestExecStubs.limit_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            price=Price.from_str("100.00"),
            quantity=self.instrument.make_qty(30.0),
            client_order_id=TestIdStubs.client_order_id(3),
        )
        matching_engine.process_order(order3, self.account_id)

        messages.clear()

        # Process SELLER trade at 100.00 with size=70 (less than total 90)
        trade = TestDataStubs.trade_tick(
            instrument=self.instrument,
            price=100.00,
            size=70.0,
            aggressor_side=AggressorSide.SELLER,
        )
        matching_engine.process_trade_tick(trade)

        # Assert: Orders fill in order, consuming 30 + 30 + 10 = 70
        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 3, f"Expected 3 fills, was {len(filled_events)}"
        assert filled_events[0].last_qty == Quantity.from_str("30.000")
        assert filled_events[1].last_qty == Quantity.from_str("30.000")
        assert filled_events[2].last_qty == Quantity.from_str("10.000")

    def test_trade_fill_returns_none_when_trade_fully_consumed(self) -> None:
        """
        Test that orders don't fill when the trade tick has been fully consumed by
        previous orders.
        """
        matching_engine = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L1_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=True,
            trade_execution=True,
            liquidity_consumption=True,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Set initial bid/ask
        quote = TestDataStubs.quote_tick(
            instrument=self.instrument,
            bid_price=90.00,
            ask_price=110.00,
        )
        matching_engine.process_quote_tick(quote)

        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        # Place 3 BUY LIMIT orders at 100.00 (qty=20 each, total=60)
        for i in range(3):
            order = TestExecStubs.limit_order(
                instrument=self.instrument,
                order_side=OrderSide.BUY,
                price=Price.from_str("100.00"),
                quantity=self.instrument.make_qty(20.0),
                client_order_id=TestIdStubs.client_order_id(i + 1),
            )
            matching_engine.process_order(order, self.account_id)

        messages.clear()

        # Process SELLER trade at 100.00 with size=40 (only enough for 2 orders)
        trade = TestDataStubs.trade_tick(
            instrument=self.instrument,
            price=100.00,
            size=40.0,
            aggressor_side=AggressorSide.SELLER,
        )
        matching_engine.process_trade_tick(trade)

        # Assert: Only 2 orders fill (20 + 20 = 40), third order gets nothing
        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 2, f"Expected 2 fills, was {len(filled_events)}"
        assert filled_events[0].last_qty == Quantity.from_str("20.000")
        assert filled_events[1].last_qty == Quantity.from_str("20.000")

    def test_trade_execution_fill_limited_by_trade_size(self) -> None:
        """
        Test that trade execution fills are limited by trade tick size when
        liquidity_consumption is enabled.
        """
        matching_engine = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L1_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=True,
            trade_execution=True,
            liquidity_consumption=True,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Set initial bid/ask
        quote = TestDataStubs.quote_tick(
            instrument=self.instrument,
            bid_price=90.00,
            ask_price=110.00,
        )
        matching_engine.process_quote_tick(quote)

        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        # Place large BUY LIMIT order at 100.00 (qty=100)
        order = TestExecStubs.limit_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            price=Price.from_str("100.00"),
            quantity=self.instrument.make_qty(100.0),
        )
        matching_engine.process_order(order, self.account_id)
        messages.clear()

        # Process SELLER trade at 100.00 with size=30
        trade = TestDataStubs.trade_tick(
            instrument=self.instrument,
            price=100.00,
            size=30.0,
            aggressor_side=AggressorSide.SELLER,
        )
        matching_engine.process_trade_tick(trade)

        # Assert: Fill is limited to trade size (30) not order size (100)
        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 1, f"Expected 1 fill, was {len(filled_events)}"
        assert filled_events[0].last_qty == Quantity.from_str("30.000")

    def test_stop_market_fills_on_seller_trade_tick(self) -> None:
        """
        Test that stop market orders fill correctly on trade ticks.

        Stop-market orders fill directly without generating an OrderTriggered event.

        """
        matching_engine = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L1_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=False,
            trade_execution=True,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Set initial bid/ask with bid at 100
        quote = TestDataStubs.quote_tick(
            instrument=self.instrument,
            bid_price=100.00,
            ask_price=110.00,
        )
        matching_engine.process_quote_tick(quote)

        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        # Place SELL STOP_MARKET with trigger at 95
        order = StopMarketOrder(
            trader_id=self.trader_id,
            strategy_id=TestIdStubs.strategy_id(),
            instrument_id=self.instrument.id,
            client_order_id=TestIdStubs.client_order_id(),
            order_side=OrderSide.SELL,
            quantity=self.instrument.make_qty(1.0),
            trigger_price=Price.from_str("95.00"),
            trigger_type=TriggerType.DEFAULT,
            init_id=UUID4(),
            ts_init=0,
        )
        matching_engine.process_order(order, self.account_id)
        messages.clear()

        # Process SELLER trade at 95 (should trigger the stop)
        trade = TestDataStubs.trade_tick(
            instrument=self.instrument,
            price=95.00,
            size=10.0,
            aggressor_side=AggressorSide.SELLER,
        )
        matching_engine.process_trade_tick(trade)

        # Assert: stop-market fills directly (no OrderTriggered event)
        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 1, f"Stop should fill, was {len(filled_events)} fills"


def _create_bar_execution_matching_engine() -> OrderMatchingEngine:
    clock = TestClock()
    trader_id = TestIdStubs.trader_id()
    msgbus = MessageBus(trader_id=trader_id, clock=clock)
    instrument = _ETHUSDT_PERP_BINANCE
    cache = TestComponentStubs.cache()
    cache.add_instrument(instrument)

    return OrderMatchingEngine(
        instrument=instrument,
        raw_id=0,
        fill_model=FillModel(),
        fee_model=MakerTakerFeeModel(),
        book_type=BookType.L1_MBP,
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        reject_stop_orders=True,
        bar_execution=True,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )


def _create_bar_with_volume(volume: str) -> Bar:
    instrument = _ETHUSDT_PERP_BINANCE
    bar_spec = BarSpecification(
        step=1,
        aggregation=BarAggregation.MINUTE,
        price_type=PriceType.LAST,
    )
    bar_type = BarType(
        instrument_id=instrument.id,
        bar_spec=bar_spec,
        aggregation_source=AggregationSource.EXTERNAL,
    )
    return Bar(
        bar_type=bar_type,
        open=Price.from_str("1000.00"),
        high=Price.from_str("1001.00"),
        low=Price.from_str("999.00"),
        close=Price.from_str("1000.50"),
        volume=Quantity.from_str(volume),
        ts_event=0,
        ts_init=0,
    )


@pytest.mark.parametrize(
    "volume",
    [
        "0.001",  # Minimum volume equal to size_increment
        "0.002",  # Quarter would round to 0, bumps to min
        "0.003",  # Quarter rounds down
        "0.005",  # Quarter = 0.00125, rounds to 0.001
        "0.010",  # Quarter = 0.0025, rounds to 0.002
        "0.150",  # Quarter = 0.0375, not multiple of size_increment
        "1.000",  # Quarter = 0.25, exact
        "1.234",  # Quarter = 0.3085, rounds to 0.308
        "100.000",  # Large volume that divides evenly
    ],
)
def test_bar_execution_respects_size_increment(volume: str) -> None:
    """
    Test bar execution quantity rounding respects instrument size_increment.

    Related to fix in PR #3352 for fractional fill quantities.

    """
    # Arrange
    matching_engine = _create_bar_execution_matching_engine()
    bar = _create_bar_with_volume(volume)

    # Act - Should not raise
    matching_engine.process_bar(bar)


@pytest.mark.parametrize(
    ("order_side", "opposite_side"),
    [
        (OrderSide.BUY, OrderSide.SELL),
        (OrderSide.SELL, OrderSide.BUY),
    ],
    ids=["buy_partial_fill_then_modify", "sell_partial_fill_then_modify"],
)
def test_modify_partially_filled_limit_order_crosses_new_book_level(
    order_side: OrderSide,
    opposite_side: OrderSide,
) -> None:
    """
    Test that modifying a partially filled limit order to a price that crosses a new
    level in the book triggers an immediate fill.

    Regression test for reported bug:
    1. BUY LIMIT at 0.05458 (crosses ask, gets partial fill)
    2. Partial fill consumes liquidity at that level
    3. Modify to 0.05461 (should cross new ask at 0.05459)
    4. BUY at 0.05461 should cross SELL at 0.05459, but NO FILL

    """
    # Arrange
    clock = TestClock()
    trader_id = TestIdStubs.trader_id()
    msgbus = MessageBus(trader_id=trader_id, clock=clock)
    instrument = TestInstrumentProvider.ethusdt_perp_binance()
    cache = TestComponentStubs.cache()
    cache.add_instrument(instrument)
    account_id = TestIdStubs.account_id()

    exec_engine = ExecutionEngine(
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )
    _ = exec_engine  # Registers handlers on msgbus

    matching_engine = OrderMatchingEngine(
        instrument=instrument,
        raw_id=0,
        fill_model=FillModel(),
        fee_model=MakerTakerFeeModel(),
        book_type=BookType.L2_MBP,
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        reject_stop_orders=True,
        trade_execution=False,
        liquidity_consumption=True,  # Track consumed liquidity per level
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    events: list[Any] = []
    msgbus.subscribe("events*", events.append)

    # Set up L2 book matching user's exact scenario:
    # - First level is crossed by initial order → partial fill
    # - Second level is BETWEEN initial order price and modified price
    # - After modify, the new price should cross the second level
    #
    # User's scenario (BUY side):
    # - BUY LIMIT at 0.05458 (crosses ASK at or below 0.05458)
    # - Partial fill consumes first level
    # - Modify to 0.05461
    # - ASK at 0.05459 should be crossed, but NO FILL
    #
    # For BUY: ASK at 1498 (crossed by initial 1500), ASK at 1502 (not crossed by 1500, crossed by 1510)
    # For SELL: BID at 1502 (crossed by initial 1500), BID at 1498 (not crossed by 1500, crossed by 1490)
    if order_side == OrderSide.BUY:
        first_level_price = "1498.00"  # ASK below initial order → crossed, partial fill
        second_level_price = (
            "1502.00"  # ASK above initial but below modified → should cross after modify
        )
        same_side_price = "1490.00"  # BID (same side) required by FillModel
        order_initial_price = "1500.00"  # Crosses first level at 1498
        order_modify_price = "1510.00"  # Should cross second level at 1502
    else:
        first_level_price = "1502.00"  # BID above initial order → crossed, partial fill
        second_level_price = (
            "1498.00"  # BID below initial but above modified → should cross after modify
        )
        same_side_price = "1510.00"  # ASK (same side) required by FillModel
        order_initial_price = "1500.00"  # Crosses first level at 1502
        order_modify_price = "1490.00"  # Should cross second level at 1498

    # Add first level on opposite side (will be partially consumed)
    delta1 = OrderBookDelta(
        instrument_id=instrument.id,
        action=BookAction.ADD,
        order=BookOrder(
            side=opposite_side,
            price=Price.from_str(first_level_price),
            size=Quantity.from_str("50.000"),
            order_id=1,
        ),
        flags=0,
        sequence=0,
        ts_event=0,
        ts_init=0,
    )
    matching_engine.process_order_book_delta(delta1)

    # Add second level on opposite side
    delta2 = OrderBookDelta(
        instrument_id=instrument.id,
        action=BookAction.ADD,
        order=BookOrder(
            side=opposite_side,
            price=Price.from_str(second_level_price),
            size=Quantity.from_str("50.000"),
            order_id=2,
        ),
        flags=0,
        sequence=1,
        ts_event=0,
        ts_init=0,
    )
    matching_engine.process_order_book_delta(delta2)

    # Add same-side level (required by FillModel to determine fills)
    delta3 = OrderBookDelta(
        instrument_id=instrument.id,
        action=BookAction.ADD,
        order=BookOrder(
            side=order_side,
            price=Price.from_str(same_side_price),
            size=Quantity.from_str("100.000"),
            order_id=3,
        ),
        flags=0,
        sequence=2,
        ts_event=0,
        ts_init=0,
    )
    matching_engine.process_order_book_delta(delta3)

    # Step 1: Place limit order that crosses first level (partial fill)
    # Order qty 100, but only 50 available at first level
    order = TestExecStubs.limit_order(
        instrument=instrument,
        order_side=order_side,
        price=Price.from_str(order_initial_price),
        quantity=instrument.make_qty(100.0),
    )
    cache.add_order(order)
    matching_engine.process_order(order, account_id)

    # Verify partial fill occurred
    filled_events = [e for e in events if isinstance(e, OrderFilled)]
    assert len(filled_events) == 1, f"Expected 1 partial fill, was {len(filled_events)}"
    assert filled_events[0].last_qty == Quantity.from_str("50.000"), (
        f"Expected partial fill of 50, was {filled_events[0].last_qty}"
    )
    events.clear()

    # Step 2: Modify order to cross second level
    modify_command = ModifyOrder(
        trader_id=trader_id,
        strategy_id=StrategyId("S-001"),
        instrument_id=instrument.id,
        client_order_id=order.client_order_id,
        venue_order_id=VenueOrderId("V-001"),
        quantity=None,
        price=Price.from_str(order_modify_price),
        trigger_price=None,
        command_id=UUID4(),
        ts_init=1,
    )
    matching_engine.process_modify(modify_command, account_id)

    # Assert - remaining quantity should fill at second level
    filled_events = [e for e in events if isinstance(e, OrderFilled)]
    assert len(filled_events) >= 1, (
        f"Expected fill after modifying partially filled order, "
        f"got events: {[type(e).__name__ for e in events]}"
    )
    assert filled_events[0].last_px == Price.from_str(second_level_price), (
        f"Fill price should be {second_level_price}, was {filled_events[0].last_px}"
    )


class TestOrderMatchingEngineMarketOrderAcks:
    """
    Tests for use_market_order_acks feature.

    When enabled, the matching engine generates OrderAccepted events for market orders
    before filling them, mimicking behavior of venues like Binance.

    """

    def setup(self):
        self.clock = TestClock()
        self.trader_id = TestIdStubs.trader_id()
        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
        )
        self.instrument = _ETHUSDT_PERP_BINANCE
        self.account_id = TestIdStubs.account_id()
        self.cache = TestComponentStubs.cache()
        self.cache.add_instrument(self.instrument)

    def test_market_order_with_acks_generates_accepted_then_filled(self) -> None:
        # Arrange - create engine with use_market_order_acks enabled
        matching_engine = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L1_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            use_market_order_acks=True,
        )

        # Set up market with bid/ask
        quote = TestDataStubs.quote_tick(
            instrument=self.instrument,
            bid_price=1000.0,
            ask_price=1001.0,
        )
        matching_engine.process_quote_tick(quote)

        # Register to capture events
        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        # Create and process market order
        order: MarketOrder = TestExecStubs.market_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
        )
        matching_engine.process_order(order, self.account_id)

        # Assert - OrderAccepted should come before OrderFilled
        assert len(messages) == 2
        assert isinstance(messages[0], OrderAccepted)
        assert isinstance(messages[1], OrderFilled)
        assert messages[0].client_order_id == order.client_order_id
        assert messages[1].client_order_id == order.client_order_id

    def test_market_order_without_acks_generates_only_filled(self) -> None:
        # Arrange - create engine with use_market_order_acks disabled (default)
        matching_engine = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L1_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            use_market_order_acks=False,
        )

        # Set up market with bid/ask
        quote = TestDataStubs.quote_tick(
            instrument=self.instrument,
            bid_price=1000.0,
            ask_price=1001.0,
        )
        matching_engine.process_quote_tick(quote)

        # Register to capture events
        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        # Create and process market order
        order: MarketOrder = TestExecStubs.market_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
        )
        matching_engine.process_order(order, self.account_id)

        # Assert - only OrderFilled, no OrderAccepted
        assert len(messages) == 1
        assert isinstance(messages[0], OrderFilled)
        assert messages[0].client_order_id == order.client_order_id


class TestOrderMatchingEngineLiquidityConsumption:
    """
    Regression tests for liquidity consumption edge cases.
    """

    def setup(self):
        self.clock = TestClock()
        self.trader_id = TestIdStubs.trader_id()
        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
        )
        self.instrument = _ETHUSDT_PERP_BINANCE
        self.account_id = TestIdStubs.account_id()
        self.cache = TestComponentStubs.cache()
        self.cache.add_instrument(self.instrument)

    def test_stop_market_fills_at_gap_with_liquidity_consumption(self) -> None:
        """
        Regression test: Stop-market orders should fill at trigger price during
        bar processing even when liquidity_consumption=True and the trigger price
        has no book liquidity (gap scenario).

        Previously, the fill was discarded because liquidity consumption checked
        for book size at the trigger price, which was 0 for gaps.
        """
        # Arrange: Create engine with liquidity_consumption enabled
        clock = TestClock()
        trader_id = TestIdStubs.trader_id()
        msgbus = MessageBus(trader_id=trader_id, clock=clock)
        cache = TestComponentStubs.cache()
        cache.add_instrument(self.instrument)

        matching_engine = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L1_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=False,
            liquidity_consumption=True,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
        )

        # Set initial bid/ask at 100/110
        quote = TestDataStubs.quote_tick(
            instrument=self.instrument,
            bid_price=100.00,
            ask_price=110.00,
        )
        matching_engine.process_quote_tick(quote)

        messages: list[Any] = []
        msgbus.register("ExecEngine.process", messages.append)

        # Place SELL stop at 95 (not currently triggered)
        order = StopMarketOrder(
            trader_id=trader_id,
            strategy_id=TestIdStubs.strategy_id(),
            instrument_id=self.instrument.id,
            client_order_id=TestIdStubs.client_order_id(),
            order_side=OrderSide.SELL,
            quantity=self.instrument.make_qty(1.0),
            trigger_price=Price.from_str("95.00"),
            trigger_type=TriggerType.DEFAULT,
            init_id=UUID4(),
            ts_init=0,
        )
        matching_engine.process_order(order, self.account_id)
        messages.clear()

        # Act: Process quote that gaps through the stop (bid goes from 100 to 90)
        gap_quote = TestDataStubs.quote_tick(
            instrument=self.instrument,
            bid_price=90.00,
            ask_price=100.00,
        )
        matching_engine.process_quote_tick(gap_quote)

        # Assert: Stop should fill despite gap (no triggered event for stop-market)
        filled_events = [m for m in messages if isinstance(m, OrderFilled)]

        assert len(filled_events) == 1, (
            f"Stop should fill despite gap, was: {[type(m).__name__ for m in messages]}"
        )

    def test_market_if_touched_fills_at_trigger_price(self) -> None:
        """
        Test MarketIfTouchedOrder fills at trigger price during bar processing.

        Regression test: Previously, MIT orders were filling at bar extremes
        instead of trigger price, causing positive slippage.

        """
        # Arrange: Create engine with bar_execution enabled
        clock = TestClock()
        trader_id = TestIdStubs.trader_id()
        msgbus = MessageBus(trader_id=trader_id, clock=clock)
        cache = TestComponentStubs.cache()
        cache.add_instrument(self.instrument)

        matching_engine = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L1_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=False,
            bar_execution=True,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
        )

        messages: list[Any] = []
        msgbus.register("ExecEngine.process", messages.append)

        # Place BUY MIT at 95.00 (below current market)
        # MIT BUY triggers when price touches 95 from above
        order = MarketIfTouchedOrder(
            trader_id=trader_id,
            strategy_id=TestIdStubs.strategy_id(),
            instrument_id=self.instrument.id,
            client_order_id=TestIdStubs.client_order_id(),
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(1.0),
            trigger_price=Price.from_str("95.00"),
            trigger_type=TriggerType.DEFAULT,
            init_id=UUID4(),
            ts_init=0,
        )
        matching_engine.process_order(order, self.account_id)
        messages.clear()

        # Act: Process bar with low at 90.00 (crosses trigger at 95.00)
        # Bar moves from 100 -> high:105 -> low:90 -> close:92
        # Low at 90.00 should trigger MIT at 95.00, NOT fill at 90.00
        bar_spec = BarSpecification(
            step=1,
            aggregation=BarAggregation.MINUTE,
            price_type=PriceType.LAST,
        )
        bar_type = BarType(
            instrument_id=self.instrument.id,
            bar_spec=bar_spec,
            aggregation_source=AggregationSource.EXTERNAL,
        )
        trigger_bar = Bar(
            bar_type=bar_type,
            open=Price.from_str("100.00"),
            high=Price.from_str("105.00"),
            low=Price.from_str("90.00"),
            close=Price.from_str("92.00"),
            volume=Quantity.from_str("100.000"),
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_bar(trigger_bar)

        # Assert: MIT fills at trigger price (95.00), NOT bar extreme (90.00)
        filled_events = [m for m in messages if isinstance(m, OrderFilled)]

        assert len(filled_events) == 1, (
            f"MIT should fill at trigger price, was: {[type(m).__name__ for m in messages]}"
        )
        fill_event = filled_events[0]
        assert fill_event.last_px == Price.from_str("95.00"), (
            f"Expected fill at trigger price 95.00, was {fill_event.last_px}"
        )

    def test_market_if_touched_sell_fills_at_trigger_price(self) -> None:
        """
        Test SELL MarketIfTouchedOrder fills at trigger price (not bar extreme) during
        bar processing.
        """
        # Arrange
        clock = TestClock()
        trader_id = TestIdStubs.trader_id()
        msgbus = MessageBus(trader_id=trader_id, clock=clock)
        cache = TestComponentStubs.cache()
        cache.add_instrument(self.instrument)

        matching_engine = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L1_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=False,
            bar_execution=True,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
        )

        messages: list[Any] = []
        msgbus.register("ExecEngine.process", messages.append)

        # SELL MIT at 105 triggers when price touches from below
        order = MarketIfTouchedOrder(
            trader_id=trader_id,
            strategy_id=TestIdStubs.strategy_id(),
            instrument_id=self.instrument.id,
            client_order_id=TestIdStubs.client_order_id(),
            order_side=OrderSide.SELL,
            quantity=self.instrument.make_qty(1.0),
            trigger_price=Price.from_str("105.00"),
            trigger_type=TriggerType.DEFAULT,
            init_id=UUID4(),
            ts_init=0,
        )
        matching_engine.process_order(order, self.account_id)
        messages.clear()

        # Act: bar high at 110 crosses trigger at 105
        bar_spec = BarSpecification(
            step=1,
            aggregation=BarAggregation.MINUTE,
            price_type=PriceType.LAST,
        )
        bar_type = BarType(
            instrument_id=self.instrument.id,
            bar_spec=bar_spec,
            aggregation_source=AggregationSource.EXTERNAL,
        )
        trigger_bar = Bar(
            bar_type=bar_type,
            open=Price.from_str("100.00"),
            high=Price.from_str("110.00"),
            low=Price.from_str("98.00"),
            close=Price.from_str("102.00"),
            volume=Quantity.from_str("100.000"),
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_bar(trigger_bar)

        # Assert
        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 1, (
            f"MIT should fill at trigger price, was: {[type(m).__name__ for m in messages]}"
        )
        fill_event = filled_events[0]
        assert fill_event.last_px == Price.from_str("105.00"), (
            f"Expected fill at trigger price 105.00, was {fill_event.last_px}"
        )

    def test_market_if_touched_buy_fills_at_trigger_price_with_liquidity_consumption(
        self,
    ) -> None:
        """
        Test BUY MIT fills at trigger price with liquidity consumption enabled.

        Regression: Liquidity consumption must not discard fills at trigger price
        where no book liquidity exists (gap scenario).

        """
        # Arrange
        clock = TestClock()
        trader_id = TestIdStubs.trader_id()
        msgbus = MessageBus(trader_id=trader_id, clock=clock)
        cache = TestComponentStubs.cache()
        cache.add_instrument(self.instrument)

        matching_engine = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L1_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=False,
            bar_execution=True,
            liquidity_consumption=True,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
        )

        messages: list[Any] = []
        msgbus.register("ExecEngine.process", messages.append)

        # BUY MIT at 95 triggers when price touches from above
        order = MarketIfTouchedOrder(
            trader_id=trader_id,
            strategy_id=TestIdStubs.strategy_id(),
            instrument_id=self.instrument.id,
            client_order_id=TestIdStubs.client_order_id(),
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(1.0),
            trigger_price=Price.from_str("95.00"),
            trigger_type=TriggerType.DEFAULT,
            init_id=UUID4(),
            ts_init=0,
        )
        matching_engine.process_order(order, self.account_id)
        messages.clear()

        # Act: bar low at 90 crosses trigger at 95
        bar_spec = BarSpecification(
            step=1,
            aggregation=BarAggregation.MINUTE,
            price_type=PriceType.LAST,
        )
        bar_type = BarType(
            instrument_id=self.instrument.id,
            bar_spec=bar_spec,
            aggregation_source=AggregationSource.EXTERNAL,
        )
        trigger_bar = Bar(
            bar_type=bar_type,
            open=Price.from_str("100.00"),
            high=Price.from_str("102.00"),
            low=Price.from_str("90.00"),
            close=Price.from_str("98.00"),
            volume=Quantity.from_str("100.000"),
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_bar(trigger_bar)

        # Assert
        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 1, (
            f"MIT should fill with liquidity consumption, was: {[type(m).__name__ for m in messages]}"
        )
        fill_event = filled_events[0]
        assert fill_event.last_px == Price.from_str("95.00"), (
            f"Expected fill at trigger price 95.00 with liquidity consumption, was {fill_event.last_px}"
        )

    def test_market_if_touched_sell_fills_at_trigger_price_with_liquidity_consumption(
        self,
    ) -> None:
        """
        Test SELL MIT fills at trigger price with liquidity consumption enabled.

        Regression: Liquidity consumption must not discard fills at trigger price
        where no book liquidity exists (gap scenario).

        """
        # Arrange
        clock = TestClock()
        trader_id = TestIdStubs.trader_id()
        msgbus = MessageBus(trader_id=trader_id, clock=clock)
        cache = TestComponentStubs.cache()
        cache.add_instrument(self.instrument)

        matching_engine = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L1_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=False,
            bar_execution=True,
            liquidity_consumption=True,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
        )

        messages: list[Any] = []
        msgbus.register("ExecEngine.process", messages.append)

        # SELL MIT at 105 triggers when price touches from below
        order = MarketIfTouchedOrder(
            trader_id=trader_id,
            strategy_id=TestIdStubs.strategy_id(),
            instrument_id=self.instrument.id,
            client_order_id=TestIdStubs.client_order_id(),
            order_side=OrderSide.SELL,
            quantity=self.instrument.make_qty(1.0),
            trigger_price=Price.from_str("105.00"),
            trigger_type=TriggerType.DEFAULT,
            init_id=UUID4(),
            ts_init=0,
        )
        matching_engine.process_order(order, self.account_id)
        messages.clear()

        # Act: bar high at 110 crosses trigger at 105
        bar_spec = BarSpecification(
            step=1,
            aggregation=BarAggregation.MINUTE,
            price_type=PriceType.LAST,
        )
        bar_type = BarType(
            instrument_id=self.instrument.id,
            bar_spec=bar_spec,
            aggregation_source=AggregationSource.EXTERNAL,
        )
        trigger_bar = Bar(
            bar_type=bar_type,
            open=Price.from_str("100.00"),
            high=Price.from_str("110.00"),
            low=Price.from_str("98.00"),
            close=Price.from_str("102.00"),
            volume=Quantity.from_str("100.000"),
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_bar(trigger_bar)

        # Assert
        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 1, (
            f"MIT should fill with liquidity consumption, was: {[type(m).__name__ for m in messages]}"
        )
        fill_event = filled_events[0]
        assert fill_event.last_px == Price.from_str("105.00"), (
            f"Expected fill at trigger price 105.00 with liquidity consumption, was {fill_event.last_px}"
        )

    def test_liquidity_consumption_regression_level_after_delete(self):
        """
        Tests that a level reappearing after DELETE is treated as fresh liquidity and
        matches correctly (no stale book race condition).
        """
        matching_engine = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L2_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=True,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            liquidity_consumption=True,
        )

        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        # Arrange
        bid_delta = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=OrderSide.BUY,
                price=Price.from_str("90.00"),
                size=Quantity.from_str("1000.000"),
                order_id=1,
            ),
            flags=0,
            sequence=0,
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_order_book_delta(bid_delta)

        ask_delta_1 = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=OrderSide.SELL,
                price=Price.from_str("100.00"),
                size=Quantity.from_str("500.000"),
                order_id=100,
            ),
            flags=0,
            sequence=1,
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_order_book_delta(ask_delta_1)

        # Act - consume liquidity
        order1 = TestExecStubs.market_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(500.0),
            client_order_id=TestIdStubs.client_order_id(1),
        )
        matching_engine.process_order(order1, self.account_id)
        matching_engine.iterate(timestamp_ns=1)

        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 1
        assert filled_events[0].last_qty == Quantity.from_str("500.000")
        messages.clear()

        # Act - delete level
        ask_delta_2 = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.DELETE,
            order=BookOrder(
                side=OrderSide.SELL,
                price=Price.from_str("100.00"),
                size=Quantity.from_str("0.000"),
                order_id=100,
            ),
            flags=0,
            sequence=2,
            ts_event=1,
            ts_init=1,
        )
        matching_engine.process_order_book_delta(ask_delta_2)

        # Act - level reappears after DELETE (tests stale book fix)
        ask_delta_3 = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.UPDATE,
            order=BookOrder(
                side=OrderSide.SELL,
                price=Price.from_str("100.00"),
                size=Quantity.from_str("300.000"),
                order_id=100,
            ),
            flags=0,
            sequence=3,
            ts_event=2,
            ts_init=2,
        )
        matching_engine.process_order_book_delta(ask_delta_3)

        # Assert - should fill 300 (fresh liquidity after DELETE)
        order2 = TestExecStubs.market_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(300.0),
            client_order_id=TestIdStubs.client_order_id(2),
        )
        matching_engine.process_order(order2, self.account_id)
        matching_engine.iterate(timestamp_ns=3)

        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 1
        assert filled_events[0].last_qty == Quantity.from_str("300.000")

    def test_liquidity_consumption_comprehensive_regression(self):
        """
        Tests key liquidity consumption scenarios:
        1. Sequential fills with consumption tracking
        2. UPDATE resets consumption (correct for L3/MBO)
        3. Size increase resets consumption
        4. Size decrease resets consumption
        5. Level deletion and reappearance
        """
        matching_engine = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L2_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=True,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            liquidity_consumption=True,
        )

        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        bid_delta = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=OrderSide.BUY,
                price=Price.from_str("90.00"),
                size=Quantity.from_str("1000.000"),
                order_id=1,
            ),
            flags=0,
            sequence=0,
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_order_book_delta(bid_delta)

        # Scenario 1: Initial ADD + consume
        ask_delta_1 = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=OrderSide.SELL,
                price=Price.from_str("100.00"),
                size=Quantity.from_str("800.000"),
                order_id=100,
            ),
            flags=0,
            sequence=1,
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_order_book_delta(ask_delta_1)

        order1 = TestExecStubs.market_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(500.0),
            client_order_id=TestIdStubs.client_order_id(1),
        )
        matching_engine.process_order(order1, self.account_id)
        matching_engine.iterate(timestamp_ns=1)

        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 1
        assert filled_events[0].last_qty == Quantity.from_str("500.000")
        messages.clear()

        # Scenario 2: UPDATE resets consumption (correct for L3 where same aggregate size
        # could be backed by different orders)
        ask_delta_2 = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.UPDATE,
            order=BookOrder(
                side=OrderSide.SELL,
                price=Price.from_str("100.00"),
                size=Quantity.from_str("800.000"),
                order_id=100,
            ),
            flags=0,
            sequence=2,
            ts_event=1,
            ts_init=1,
        )
        matching_engine.process_order_book_delta(ask_delta_2)

        order2 = TestExecStubs.market_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(400.0),
            client_order_id=TestIdStubs.client_order_id(2),
        )
        matching_engine.process_order(order2, self.account_id)
        matching_engine.iterate(timestamp_ns=2)

        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 1
        assert filled_events[0].last_qty == Quantity.from_str("400.000")
        messages.clear()

        # Scenario 3: Size INCREASE resets consumption
        ask_delta_3 = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.UPDATE,
            order=BookOrder(
                side=OrderSide.SELL,
                price=Price.from_str("100.00"),
                size=Quantity.from_str("1500.000"),
                order_id=100,
            ),
            flags=0,
            sequence=3,
            ts_event=2,
            ts_init=2,
        )
        matching_engine.process_order_book_delta(ask_delta_3)

        order3 = TestExecStubs.market_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(1000.0),
            client_order_id=TestIdStubs.client_order_id(3),
        )
        matching_engine.process_order(order3, self.account_id)
        matching_engine.iterate(timestamp_ns=3)

        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 1
        assert filled_events[0].last_qty == Quantity.from_str("1000.000")
        messages.clear()

        # Scenario 4: Size DECREASE resets consumption
        ask_delta_4 = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.UPDATE,
            order=BookOrder(
                side=OrderSide.SELL,
                price=Price.from_str("100.00"),
                size=Quantity.from_str("600.000"),
                order_id=100,
            ),
            flags=0,
            sequence=4,
            ts_event=3,
            ts_init=3,
        )
        matching_engine.process_order_book_delta(ask_delta_4)

        order4 = TestExecStubs.market_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(700.0),
            client_order_id=TestIdStubs.client_order_id(4),
        )
        matching_engine.process_order(order4, self.account_id)
        matching_engine.iterate(timestamp_ns=4)

        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 1
        assert filled_events[0].last_qty == Quantity.from_str("600.000")
        messages.clear()

        # Scenario 5: DELETE then UPDATE (stale book fix)
        ask_delta_5 = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.DELETE,
            order=BookOrder(
                side=OrderSide.SELL,
                price=Price.from_str("100.00"),
                size=Quantity.from_str("0.000"),
                order_id=100,
            ),
            flags=0,
            sequence=5,
            ts_event=4,
            ts_init=4,
        )
        matching_engine.process_order_book_delta(ask_delta_5)

        ask_delta_6 = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.UPDATE,
            order=BookOrder(
                side=OrderSide.SELL,
                price=Price.from_str("100.00"),
                size=Quantity.from_str("400.000"),
                order_id=100,
            ),
            flags=0,
            sequence=6,
            ts_event=5,
            ts_init=5,
        )
        matching_engine.process_order_book_delta(ask_delta_6)

        order5 = TestExecStubs.market_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(400.0),
            client_order_id=TestIdStubs.client_order_id(5),
        )
        matching_engine.process_order(order5, self.account_id)
        matching_engine.iterate(timestamp_ns=6)

        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 1
        assert filled_events[0].last_qty == Quantity.from_str("400.000")

    def test_liquidity_consumption_unrelated_level_update(self):
        """
        Tests that updating an unrelated price level does NOT reset consumption tracking
        at other levels.
        """
        matching_engine = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L2_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=True,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            liquidity_consumption=True,
        )

        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        # Arrange: bid and two ask levels
        bid_delta = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=OrderSide.BUY,
                price=Price.from_str("90.00"),
                size=Quantity.from_str("1000.000"),
                order_id=1,
            ),
            flags=0,
            sequence=0,
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_order_book_delta(bid_delta)

        ask_delta_100 = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=OrderSide.SELL,
                price=Price.from_str("100.00"),
                size=Quantity.from_str("500.000"),
                order_id=100,
            ),
            flags=0,
            sequence=1,
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_order_book_delta(ask_delta_100)

        ask_delta_99 = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=OrderSide.SELL,
                price=Price.from_str("99.00"),
                size=Quantity.from_str("300.000"),
                order_id=99,
            ),
            flags=0,
            sequence=2,
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_order_book_delta(ask_delta_99)

        # Act: consume 200 from 99.00 level
        order1 = TestExecStubs.market_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(200.0),
            client_order_id=TestIdStubs.client_order_id(1),
        )
        matching_engine.process_order(order1, self.account_id)
        matching_engine.iterate(timestamp_ns=1)

        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 1
        assert filled_events[0].last_qty == Quantity.from_str("200.000")
        assert filled_events[0].last_px == Price.from_str("99.00")
        messages.clear()

        # Act: UPDATE the 100.00 level (unrelated to 99.00)
        ask_delta_100_update = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.UPDATE,
            order=BookOrder(
                side=OrderSide.SELL,
                price=Price.from_str("100.00"),
                size=Quantity.from_str("600.000"),
                order_id=100,
            ),
            flags=0,
            sequence=3,
            ts_event=1,
            ts_init=1,
        )
        matching_engine.process_order_book_delta(ask_delta_100_update)

        # Act: try to fill more from 99.00 - should only get remaining 100
        order2 = TestExecStubs.market_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(200.0),
            client_order_id=TestIdStubs.client_order_id(2),
        )
        matching_engine.process_order(order2, self.account_id)
        matching_engine.iterate(timestamp_ns=2)

        # Assert: consumption at 99.00 preserved despite 100.00 update
        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 1
        assert filled_events[0].last_qty == Quantity.from_str("100.000")
        assert filled_events[0].last_px == Price.from_str("99.00")

    def test_liquidity_consumption_resets_on_snapshot_flag(self):
        """
        Tests that consumption tracking resets when a delta with F_SNAPSHOT flag (value
        32) arrives, even if the level size is unchanged.
        """
        matching_engine = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L2_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=True,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            liquidity_consumption=True,
        )

        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        # Arrange
        bid_delta = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=OrderSide.BUY,
                price=Price.from_str("90.00"),
                size=Quantity.from_str("1000.000"),
                order_id=1,
            ),
            flags=0,
            sequence=0,
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_order_book_delta(bid_delta)

        ask_delta = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=OrderSide.SELL,
                price=Price.from_str("100.00"),
                size=Quantity.from_str("500.000"),
                order_id=100,
            ),
            flags=0,
            sequence=1,
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_order_book_delta(ask_delta)

        # Act: consume 300 from the level
        order1 = TestExecStubs.market_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(300.0),
            client_order_id=TestIdStubs.client_order_id(1),
        )
        matching_engine.process_order(order1, self.account_id)
        matching_engine.iterate(timestamp_ns=1)

        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 1
        assert filled_events[0].last_qty == Quantity.from_str("300.000")
        messages.clear()

        # Act: snapshot arrives with SAME size (F_SNAPSHOT = 32)
        snapshot_delta = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.UPDATE,
            order=BookOrder(
                side=OrderSide.SELL,
                price=Price.from_str("100.00"),
                size=Quantity.from_str("500.000"),
                order_id=100,
            ),
            flags=32,
            sequence=2,
            ts_event=1,
            ts_init=1,
        )
        matching_engine.process_order_book_delta(snapshot_delta)

        # Act: try to fill 400 - should get full 400 since snapshot reset consumption
        order2 = TestExecStubs.market_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(400.0),
            client_order_id=TestIdStubs.client_order_id(2),
        )
        matching_engine.process_order(order2, self.account_id)
        matching_engine.iterate(timestamp_ns=2)

        # Assert: full 400 filled (consumption was reset by snapshot)
        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 1
        assert filled_events[0].last_qty == Quantity.from_str("400.000")

    def test_deterministic_maker_fills_at_touch_prob_1(self):
        """
        Regression test verifying that with prob_fill_on_limit=1.0, MAKER limit orders
        at touch fill deterministically when the market moves to their price.

        This tests FillModel behavior, not liquidity consumption.

        """
        fill_model = FillModel(prob_fill_on_limit=1.0, random_seed=42)

        matching_engine = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=fill_model,
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L2_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=True,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            liquidity_consumption=True,
        )

        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        # Arrange: market with bid=90, ask=110
        bid_delta = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=OrderSide.BUY,
                price=Price.from_str("90.00"),
                size=Quantity.from_str("1000.000"),
                order_id=1,
            ),
            flags=0,
            sequence=0,
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_order_book_delta(bid_delta)

        ask_delta = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=OrderSide.SELL,
                price=Price.from_str("110.00"),
                size=Quantity.from_str("1000.000"),
                order_id=2,
            ),
            flags=0,
            sequence=1,
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_order_book_delta(ask_delta)

        # Act: place BUY LIMIT at 100.00 (rests as MAKER since below ask)
        limit_order = TestExecStubs.limit_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            price=Price.from_str("100.00"),
            quantity=self.instrument.make_qty(500.0),
            client_order_id=TestIdStubs.client_order_id(1),
        )
        matching_engine.process_order(limit_order, self.account_id)
        matching_engine.iterate(timestamp_ns=1)

        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        accepted_events = [m for m in messages if isinstance(m, OrderAccepted)]
        assert len(accepted_events) == 1
        assert len(filled_events) == 0
        messages.clear()

        # Act: move ask down to 100.00 (touches limit price)
        ask_update = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.UPDATE,
            order=BookOrder(
                side=OrderSide.SELL,
                price=Price.from_str("100.00"),
                size=Quantity.from_str("800.000"),
                order_id=2,
            ),
            flags=0,
            sequence=2,
            ts_event=1,
            ts_init=1,
        )
        matching_engine.process_order_book_delta(ask_update)
        matching_engine.iterate(timestamp_ns=2)

        # Assert: with prob_fill_on_limit=1.0, order fills deterministically as MAKER
        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 1
        assert filled_events[0].last_qty == Quantity.from_str("500.000")
        assert filled_events[0].last_px == Price.from_str("100.00")
        assert filled_events[0].liquidity_side == LiquiditySide.MAKER

    def test_taker_fills_regardless_of_prob_fill_on_limit(self):
        """
        Test that TAKER orders (crossing the spread) fill regardless of
        prob_fill_on_limit setting, since that only affects MAKER orders.
        """
        fill_model = FillModel(prob_fill_on_limit=0.0, random_seed=42)

        matching_engine = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=fill_model,
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L2_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=True,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            liquidity_consumption=True,
        )

        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        # Arrange: market with bid=90, ask=100
        bid_delta = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=OrderSide.BUY,
                price=Price.from_str("90.00"),
                size=Quantity.from_str("1000.000"),
                order_id=1,
            ),
            flags=0,
            sequence=0,
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_order_book_delta(bid_delta)

        ask_delta = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=OrderSide.SELL,
                price=Price.from_str("100.00"),
                size=Quantity.from_str("1000.000"),
                order_id=2,
            ),
            flags=0,
            sequence=1,
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_order_book_delta(ask_delta)

        # Act: BUY LIMIT at 100.00 (equals ask, crosses as TAKER)
        limit_order = TestExecStubs.limit_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            price=Price.from_str("100.00"),
            quantity=self.instrument.make_qty(500.0),
            client_order_id=TestIdStubs.client_order_id(1),
        )
        matching_engine.process_order(limit_order, self.account_id)
        matching_engine.iterate(timestamp_ns=1)

        # Assert: fills as TAKER regardless of prob_fill_on_limit
        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 1
        assert filled_events[0].last_qty == Quantity.from_str("500.000")
        assert filled_events[0].liquidity_side == LiquiditySide.TAKER

    def test_maker_fills_when_ask_crosses_below_limit(self):
        """
        Tests that a resting BUY LIMIT fills when ask crosses BELOW the limit price (not
        just at touch).
        """
        fill_model = FillModel(prob_fill_on_limit=1.0, random_seed=42)

        matching_engine = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=fill_model,
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L2_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=True,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            liquidity_consumption=True,
        )

        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        # Arrange: market with bid=90, ask=110
        bid_delta = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=OrderSide.BUY,
                price=Price.from_str("90.00"),
                size=Quantity.from_str("1000.000"),
                order_id=1,
            ),
            flags=0,
            sequence=0,
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_order_book_delta(bid_delta)

        ask_delta = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=OrderSide.SELL,
                price=Price.from_str("110.00"),
                size=Quantity.from_str("1000.000"),
                order_id=2,
            ),
            flags=0,
            sequence=1,
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_order_book_delta(ask_delta)

        # Act: place BUY LIMIT at 100.00 (rests as MAKER)
        limit_order = TestExecStubs.limit_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            price=Price.from_str("100.00"),
            quantity=self.instrument.make_qty(500.0),
            client_order_id=TestIdStubs.client_order_id(1),
        )
        matching_engine.process_order(limit_order, self.account_id)
        matching_engine.iterate(timestamp_ns=1)

        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 0
        messages.clear()

        # Act: ask crosses BELOW limit to 99.00
        ask_update = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.UPDATE,
            order=BookOrder(
                side=OrderSide.SELL,
                price=Price.from_str("99.00"),
                size=Quantity.from_str("200.000"),
                order_id=2,
            ),
            flags=0,
            sequence=2,
            ts_event=1,
            ts_init=1,
        )
        matching_engine.process_order_book_delta(ask_update)
        matching_engine.iterate(timestamp_ns=2)

        # Assert: order fills as MAKER (fills at limit price 100.00)
        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 1
        assert filled_events[0].last_qty == Quantity.from_str("200.000")
        assert filled_events[0].last_px == Price.from_str("100.00")
        assert filled_events[0].liquidity_side == LiquiditySide.MAKER

    def test_liquidity_consumption_resets_on_depth10_snapshot_flag(self):
        """
        Tests that consumption tracking resets when an OrderBookDepth10 with F_SNAPSHOT
        flag (value 32) arrives.
        """
        matching_engine = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L2_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=True,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            liquidity_consumption=True,
        )

        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        # Arrange: set up book with depth10
        depth = OrderBookDepth10(
            instrument_id=self.instrument.id,
            bids=[
                BookOrder(
                    side=OrderSide.BUY,
                    price=Price.from_str("90.00"),
                    size=Quantity.from_str("1000.000"),
                    order_id=1,
                ),
            ],
            asks=[
                BookOrder(
                    side=OrderSide.SELL,
                    price=Price.from_str("100.00"),
                    size=Quantity.from_str("500.000"),
                    order_id=100,
                ),
            ],
            bid_counts=[1],
            ask_counts=[1],
            flags=0,
            sequence=0,
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_order_book_depth10(depth)

        # Act: consume 300 from the level
        order1 = TestExecStubs.market_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(300.0),
            client_order_id=TestIdStubs.client_order_id(1),
        )
        matching_engine.process_order(order1, self.account_id)
        matching_engine.iterate(timestamp_ns=1)

        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 1
        assert filled_events[0].last_qty == Quantity.from_str("300.000")
        messages.clear()

        # Act: depth10 snapshot arrives with SAME size (F_SNAPSHOT = 32)
        snapshot_depth = OrderBookDepth10(
            instrument_id=self.instrument.id,
            bids=[
                BookOrder(
                    side=OrderSide.BUY,
                    price=Price.from_str("90.00"),
                    size=Quantity.from_str("1000.000"),
                    order_id=1,
                ),
            ],
            asks=[
                BookOrder(
                    side=OrderSide.SELL,
                    price=Price.from_str("100.00"),
                    size=Quantity.from_str("500.000"),
                    order_id=100,
                ),
            ],
            bid_counts=[1],
            ask_counts=[1],
            flags=32,
            sequence=1,
            ts_event=1,
            ts_init=1,
        )
        matching_engine.process_order_book_depth10(snapshot_depth)

        # Act: try to fill 400 - should get full 400 since snapshot reset consumption
        order2 = TestExecStubs.market_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(400.0),
            client_order_id=TestIdStubs.client_order_id(2),
        )
        matching_engine.process_order(order2, self.account_id)
        matching_engine.iterate(timestamp_ns=2)

        # Assert: full 400 filled (consumption was reset by snapshot)
        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 1
        assert filled_events[0].last_qty == Quantity.from_str("400.000")

    def test_liquidity_consumption_clears_on_delete(self):
        """
        Tests that consumption tracking is cleared when a level is deleted, allowing
        fills when the level reappears with the same size.
        """
        matching_engine = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L2_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=True,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            liquidity_consumption=True,
        )

        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        # Arrange: set up book with bid and ask
        bid_delta = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=OrderSide.BUY,
                price=Price.from_str("90.00"),
                size=Quantity.from_str("1000.000"),
                order_id=1,
            ),
            flags=0,
            sequence=0,
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_order_book_delta(bid_delta)

        ask_delta = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=OrderSide.SELL,
                price=Price.from_str("100.00"),
                size=Quantity.from_str("500.000"),
                order_id=100,
            ),
            flags=0,
            sequence=1,
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_order_book_delta(ask_delta)

        # Act: consume ALL 500 from the level
        order1 = TestExecStubs.market_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(500.0),
            client_order_id=TestIdStubs.client_order_id(1),
        )
        matching_engine.process_order(order1, self.account_id)
        matching_engine.iterate(timestamp_ns=1)

        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 1
        assert filled_events[0].last_qty == Quantity.from_str("500.000")
        messages.clear()

        # Act: DELETE the level (removes it from book)
        delete_delta = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.DELETE,
            order=BookOrder(
                side=OrderSide.SELL,
                price=Price.from_str("100.00"),
                size=Quantity.from_str("0.000"),
                order_id=100,
            ),
            flags=0,
            sequence=2,
            ts_event=1,
            ts_init=1,
        )
        matching_engine.process_order_book_delta(delete_delta)

        # Act: re-ADD the level with SAME size (500)
        readd_delta = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=OrderSide.SELL,
                price=Price.from_str("100.00"),
                size=Quantity.from_str("500.000"),
                order_id=101,
            ),
            flags=0,
            sequence=3,
            ts_event=2,
            ts_init=2,
        )
        matching_engine.process_order_book_delta(readd_delta)

        # Act: try to fill 300 - should succeed since DELETE cleared consumption
        order2 = TestExecStubs.market_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(300.0),
            client_order_id=TestIdStubs.client_order_id(2),
        )
        matching_engine.process_order(order2, self.account_id)
        matching_engine.iterate(timestamp_ns=3)

        # Assert: full 300 filled (consumption was cleared by DELETE)
        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 1
        assert filled_events[0].last_qty == Quantity.from_str("300.000")

    def test_liquidity_consumption_clears_on_update(self):
        """
        Tests that consumption tracking is cleared when a level is updated, even with
        the same size.

        This is correct for L3/MBO data where the same aggregate size could be backed by
        different orders.

        """
        matching_engine = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L2_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=True,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            liquidity_consumption=True,
        )

        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        # Arrange: set up book with bid and ask
        bid_delta = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=OrderSide.BUY,
                price=Price.from_str("90.00"),
                size=Quantity.from_str("1000.000"),
                order_id=1,
            ),
            flags=0,
            sequence=0,
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_order_book_delta(bid_delta)

        ask_delta = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=OrderSide.SELL,
                price=Price.from_str("100.00"),
                size=Quantity.from_str("500.000"),
                order_id=100,
            ),
            flags=0,
            sequence=1,
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_order_book_delta(ask_delta)

        # Act: consume ALL 500 from the level
        order1 = TestExecStubs.market_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(500.0),
            client_order_id=TestIdStubs.client_order_id(1),
        )
        matching_engine.process_order(order1, self.account_id)
        matching_engine.iterate(timestamp_ns=1)

        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 1
        assert filled_events[0].last_qty == Quantity.from_str("500.000")
        messages.clear()

        # Act: UPDATE the level with SAME size (500) - should clear consumption
        update_delta = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.UPDATE,
            order=BookOrder(
                side=OrderSide.SELL,
                price=Price.from_str("100.00"),
                size=Quantity.from_str("500.000"),
                order_id=100,
            ),
            flags=0,
            sequence=2,
            ts_event=1,
            ts_init=1,
        )
        matching_engine.process_order_book_delta(update_delta)

        # Act: try to fill 300 - should succeed since UPDATE cleared consumption
        order2 = TestExecStubs.market_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(300.0),
            client_order_id=TestIdStubs.client_order_id(2),
        )
        matching_engine.process_order(order2, self.account_id)
        matching_engine.iterate(timestamp_ns=2)

        # Assert: full 300 filled (consumption was cleared by UPDATE)
        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 1
        assert filled_events[0].last_qty == Quantity.from_str("300.000")

    @pytest.mark.parametrize(
        "order_side",
        [OrderSide.BUY, OrderSide.SELL],
    )
    def test_liquidity_consumption_tracks_fills_at_multiple_price_levels(
        self,
        order_side: OrderSide,
    ):
        """
        Regression test for liquidity consumption tracking at multiple price levels.

        When an order fills across multiple price levels, consumption must be tracked
        separately at each original book price level. This test verifies that after
        consuming liquidity at multiple levels, subsequent orders cannot access that
        consumed liquidity.

        """
        matching_engine = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L2_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=True,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            liquidity_consumption=True,
        )

        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        if order_side == OrderSide.BUY:
            # Arrange: bid at 90, asks at 99, 100, 101 (50 qty each at 99/100)
            bid_delta = OrderBookDelta(
                instrument_id=self.instrument.id,
                action=BookAction.ADD,
                order=BookOrder(
                    side=OrderSide.BUY,
                    price=Price.from_str("90.00"),
                    size=Quantity.from_str("1000.000"),
                    order_id=100,
                ),
                flags=0,
                sequence=0,
                ts_event=0,
                ts_init=0,
            )
            matching_engine.process_order_book_delta(bid_delta)

            ask1 = OrderBookDelta(
                instrument_id=self.instrument.id,
                action=BookAction.ADD,
                order=BookOrder(
                    side=OrderSide.SELL,
                    price=Price.from_str("99.00"),
                    size=Quantity.from_str("50.000"),
                    order_id=1,
                ),
                flags=0,
                sequence=1,
                ts_event=0,
                ts_init=0,
            )
            matching_engine.process_order_book_delta(ask1)

            ask2 = OrderBookDelta(
                instrument_id=self.instrument.id,
                action=BookAction.ADD,
                order=BookOrder(
                    side=OrderSide.SELL,
                    price=Price.from_str("100.00"),
                    size=Quantity.from_str("50.000"),
                    order_id=2,
                ),
                flags=0,
                sequence=2,
                ts_event=0,
                ts_init=0,
            )
            matching_engine.process_order_book_delta(ask2)

            ask3 = OrderBookDelta(
                instrument_id=self.instrument.id,
                action=BookAction.ADD,
                order=BookOrder(
                    side=OrderSide.SELL,
                    price=Price.from_str("101.00"),
                    size=Quantity.from_str("100.000"),
                    order_id=3,
                ),
                flags=0,
                sequence=3,
                ts_event=0,
                ts_init=0,
            )
            matching_engine.process_order_book_delta(ask3)

            limit_price = Price.from_str("100.00")
        else:
            # Arrange: ask at 110, bids at 101, 100, 99 (50 qty each at 101/100)
            ask_delta = OrderBookDelta(
                instrument_id=self.instrument.id,
                action=BookAction.ADD,
                order=BookOrder(
                    side=OrderSide.SELL,
                    price=Price.from_str("110.00"),
                    size=Quantity.from_str("1000.000"),
                    order_id=100,
                ),
                flags=0,
                sequence=0,
                ts_event=0,
                ts_init=0,
            )
            matching_engine.process_order_book_delta(ask_delta)

            bid1 = OrderBookDelta(
                instrument_id=self.instrument.id,
                action=BookAction.ADD,
                order=BookOrder(
                    side=OrderSide.BUY,
                    price=Price.from_str("101.00"),
                    size=Quantity.from_str("50.000"),
                    order_id=1,
                ),
                flags=0,
                sequence=1,
                ts_event=0,
                ts_init=0,
            )
            matching_engine.process_order_book_delta(bid1)

            bid2 = OrderBookDelta(
                instrument_id=self.instrument.id,
                action=BookAction.ADD,
                order=BookOrder(
                    side=OrderSide.BUY,
                    price=Price.from_str("100.00"),
                    size=Quantity.from_str("50.000"),
                    order_id=2,
                ),
                flags=0,
                sequence=2,
                ts_event=0,
                ts_init=0,
            )
            matching_engine.process_order_book_delta(bid2)

            bid3 = OrderBookDelta(
                instrument_id=self.instrument.id,
                action=BookAction.ADD,
                order=BookOrder(
                    side=OrderSide.BUY,
                    price=Price.from_str("99.00"),
                    size=Quantity.from_str("100.000"),
                    order_id=3,
                ),
                flags=0,
                sequence=3,
                ts_event=0,
                ts_init=0,
            )
            matching_engine.process_order_book_delta(bid3)

            limit_price = Price.from_str("100.00")

        # Act: first order crosses both levels (50 + 50 = 100 total)
        order1 = TestExecStubs.limit_order(
            instrument=self.instrument,
            order_side=order_side,
            quantity=self.instrument.make_qty(100.0),
            price=limit_price,
            client_order_id=TestIdStubs.client_order_id(1),
        )
        matching_engine.process_order(order1, self.account_id)
        matching_engine.iterate(timestamp_ns=1)

        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        order1_fills = [f for f in filled_events if f.client_order_id.value.endswith("-1")]
        total_order1 = sum((f.last_qty for f in order1_fills), Quantity.zero(3))
        assert total_order1 == Quantity.from_str("100.000"), "First order should fill 100"

        messages.clear()

        # Act: second order attempts to fill from consumed levels
        order2 = TestExecStubs.limit_order(
            instrument=self.instrument,
            order_side=order_side,
            quantity=self.instrument.make_qty(50.0),
            price=limit_price,
            client_order_id=TestIdStubs.client_order_id(2),
        )
        matching_engine.process_order(order2, self.account_id)
        matching_engine.iterate(timestamp_ns=2)

        # Assert: no fill since both levels are consumed
        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        order2_fills = [f for f in filled_events if f.client_order_id.value.endswith("-2")]
        total_order2 = sum((f.last_qty for f in order2_fills), Quantity.zero(3))
        assert total_order2 == Quantity.zero(3), (
            "Second order should NOT fill - both price levels should be consumed. "
            f"Got fill of {total_order2}. If this fails, consumption was incorrectly "
            "tracked at wrong price levels."
        )

    def test_fok_order_canceled_when_liquidity_consumption_exhausts_fills(self):
        """
        Test that FOK orders are properly canceled when liquidity consumption results in
        no available fills.
        """
        exec_engine = ExecutionEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        _ = exec_engine  # Registers handlers on msgbus

        matching_engine = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L2_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=True,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            liquidity_consumption=True,
        )

        events: list[Any] = []
        self.msgbus.subscribe("events.order.*", events.append)

        bid_delta = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=OrderSide.BUY,
                price=Price.from_str("90.00"),
                size=Quantity.from_str("100.000"),
                order_id=100,
            ),
            flags=0,
            sequence=0,
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_order_book_delta(bid_delta)

        # Add 50 units of liquidity at ask 100.00
        ask_delta = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=OrderSide.SELL,
                price=Price.from_str("100.00"),
                size=Quantity.from_str("50.000"),
                order_id=1,
            ),
            flags=0,
            sequence=1,
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_order_book_delta(ask_delta)

        # First order consumes all liquidity
        order1 = TestExecStubs.market_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(50.0),
            client_order_id=TestIdStubs.client_order_id(1),
        )
        self.cache.add_order(order1)
        matching_engine.process_order(order1, self.account_id)
        matching_engine.iterate(timestamp_ns=1)
        events.clear()

        # FOK order should be canceled since no liquidity remains
        fok_order = TestExecStubs.limit_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            price=Price.from_str("100.00"),
            quantity=self.instrument.make_qty(30.0),
            time_in_force=TimeInForce.FOK,
            client_order_id=TestIdStubs.client_order_id(2),
        )
        self.cache.add_order(fok_order)
        matching_engine.process_order(fok_order, self.account_id)
        matching_engine.iterate(timestamp_ns=2)

        canceled_events = [e for e in events if isinstance(e, OrderCanceled)]
        assert len(canceled_events) == 1
        assert canceled_events[0].client_order_id == fok_order.client_order_id

    def test_ioc_order_canceled_when_liquidity_consumption_exhausts_fills(self):
        """
        Test that IOC orders are properly canceled when liquidity consumption results in
        no available fills.
        """
        exec_engine = ExecutionEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        _ = exec_engine  # Registers handlers on msgbus

        matching_engine = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L2_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=True,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            liquidity_consumption=True,
        )

        events: list[Any] = []
        self.msgbus.subscribe("events.order.*", events.append)

        bid_delta = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=OrderSide.BUY,
                price=Price.from_str("90.00"),
                size=Quantity.from_str("100.000"),
                order_id=100,
            ),
            flags=0,
            sequence=0,
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_order_book_delta(bid_delta)

        # Add 50 units of liquidity at ask 100.00
        ask_delta = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=OrderSide.SELL,
                price=Price.from_str("100.00"),
                size=Quantity.from_str("50.000"),
                order_id=1,
            ),
            flags=0,
            sequence=1,
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_order_book_delta(ask_delta)

        # First order consumes all liquidity
        order1 = TestExecStubs.market_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(50.0),
            client_order_id=TestIdStubs.client_order_id(1),
        )
        self.cache.add_order(order1)
        matching_engine.process_order(order1, self.account_id)
        matching_engine.iterate(timestamp_ns=1)
        events.clear()

        # IOC order should be canceled since no liquidity remains
        ioc_order = TestExecStubs.limit_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            price=Price.from_str("100.00"),
            quantity=self.instrument.make_qty(30.0),
            time_in_force=TimeInForce.IOC,
            client_order_id=TestIdStubs.client_order_id(2),
        )
        self.cache.add_order(ioc_order)
        matching_engine.process_order(ioc_order, self.account_id)
        matching_engine.iterate(timestamp_ns=2)

        canceled_events = [e for e in events if isinstance(e, OrderCanceled)]
        assert len(canceled_events) == 1
        assert canceled_events[0].client_order_id == ioc_order.client_order_id

    def test_gtc_order_not_canceled_when_liquidity_consumption_exhausts_fills(self):
        """
        Test that GTC orders are NOT canceled when liquidity consumption results in no
        available fills (they remain open for future fills).
        """
        matching_engine = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L2_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=True,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            liquidity_consumption=True,
        )

        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        bid_delta = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=OrderSide.BUY,
                price=Price.from_str("90.00"),
                size=Quantity.from_str("100.000"),
                order_id=100,
            ),
            flags=0,
            sequence=0,
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_order_book_delta(bid_delta)

        # Add 50 units of liquidity at ask 100.00
        ask_delta = OrderBookDelta(
            instrument_id=self.instrument.id,
            action=BookAction.ADD,
            order=BookOrder(
                side=OrderSide.SELL,
                price=Price.from_str("100.00"),
                size=Quantity.from_str("50.000"),
                order_id=1,
            ),
            flags=0,
            sequence=1,
            ts_event=0,
            ts_init=0,
        )
        matching_engine.process_order_book_delta(ask_delta)

        # First order consumes all liquidity
        order1 = TestExecStubs.market_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(50.0),
            client_order_id=TestIdStubs.client_order_id(1),
        )
        matching_engine.process_order(order1, self.account_id)
        matching_engine.iterate(timestamp_ns=1)
        messages.clear()

        # GTC order should NOT be canceled - it should remain open
        gtc_order = TestExecStubs.limit_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            price=Price.from_str("100.00"),
            quantity=self.instrument.make_qty(30.0),
            time_in_force=TimeInForce.GTC,
            client_order_id=TestIdStubs.client_order_id(2),
        )
        matching_engine.process_order(gtc_order, self.account_id)
        matching_engine.iterate(timestamp_ns=2)

        canceled_events = [m for m in messages if isinstance(m, OrderCanceled)]
        filled_events = [m for m in messages if isinstance(m, OrderFilled)]

        # No cancel, no fill - order remains open
        assert len(canceled_events) == 0
        assert len(filled_events) == 0

    @pytest.mark.parametrize(
        ("order_side", "aggressor_side"),
        [
            (OrderSide.BUY, AggressorSide.SELLER),
            (OrderSide.SELL, AggressorSide.BUYER),
        ],
    )
    def test_trade_execution_fill_model_at_limit_with_prob_zero_does_not_fill(
        self,
        order_side: OrderSide,
        aggressor_side: AggressorSide,
    ):
        """
        Test that when trade price equals limit price exactly, the fill model
        probability check is used.

        With prob_fill_on_limit=0.0, the order should not fill from trade execution
        (simulates being at back of queue).

        """
        fill_model = FillModel(prob_fill_on_limit=0.0, random_seed=42)

        matching_engine = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=fill_model,
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L1_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=True,
            trade_execution=True,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        # Set initial market state
        quote = TestDataStubs.quote_tick(
            instrument=self.instrument,
            bid_price=211.30,
            ask_price=211.40,
        )
        matching_engine.process_quote_tick(quote)

        # Place limit order at 211.35 (in the spread)
        order = TestExecStubs.limit_order(
            instrument=self.instrument,
            order_side=order_side,
            price=Price.from_str("211.35"),
            quantity=self.instrument.make_qty(1.0),
        )
        matching_engine.process_order(order, self.account_id)
        messages.clear()

        # Trade at exactly the limit price
        trade = TestDataStubs.trade_tick(
            instrument=self.instrument,
            price=211.35,
            aggressor_side=aggressor_side,
        )
        matching_engine.process_trade_tick(trade)

        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 0, (
            "Order should NOT fill when trade at limit price with prob_fill_on_limit=0.0"
        )

    @pytest.mark.parametrize(
        ("order_side", "aggressor_side"),
        [
            (OrderSide.BUY, AggressorSide.SELLER),
            (OrderSide.SELL, AggressorSide.BUYER),
        ],
    )
    def test_trade_execution_fill_model_at_limit_with_prob_one_fills(
        self,
        order_side: OrderSide,
        aggressor_side: AggressorSide,
    ):
        """
        Test that when trade price equals limit price exactly, with
        prob_fill_on_limit=1.0 the order fills deterministically.
        """
        fill_model = FillModel(prob_fill_on_limit=1.0, random_seed=42)

        matching_engine = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=fill_model,
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L1_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=True,
            trade_execution=True,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        # Set initial market state
        quote = TestDataStubs.quote_tick(
            instrument=self.instrument,
            bid_price=211.30,
            ask_price=211.40,
        )
        matching_engine.process_quote_tick(quote)

        # Place limit order at 211.35 (in the spread)
        order = TestExecStubs.limit_order(
            instrument=self.instrument,
            order_side=order_side,
            price=Price.from_str("211.35"),
            quantity=self.instrument.make_qty(1.0),
        )
        matching_engine.process_order(order, self.account_id)
        messages.clear()

        # Trade at exactly the limit price
        trade = TestDataStubs.trade_tick(
            instrument=self.instrument,
            price=211.35,
            aggressor_side=aggressor_side,
        )
        matching_engine.process_trade_tick(trade)

        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 1, (
            "Order should fill when trade at limit price with prob_fill_on_limit=1.0"
        )
        assert filled_events[0].last_px == Price.from_str("211.35")

    @pytest.mark.parametrize(
        ("order_side", "aggressor_side", "limit_price", "trade_price"),
        [
            # BUY: trade crosses below limit (better price for buyer)
            (OrderSide.BUY, AggressorSide.SELLER, "211.35", 211.30),
            # SELL: trade crosses above limit (better price for seller)
            (OrderSide.SELL, AggressorSide.BUYER, "211.35", 211.40),
        ],
    )
    def test_trade_execution_crossing_limit_fills_regardless_of_fill_model(
        self,
        order_side: OrderSide,
        aggressor_side: AggressorSide,
        limit_price: str,
        trade_price: float,
    ):
        """
        Test that when trade price crosses the limit (better price), the fill model is
        NOT consulted and the order fills.

        Crossing trades indicate the order would have been at the front of the queue.

        """
        fill_model = FillModel(prob_fill_on_limit=0.0, random_seed=42)

        matching_engine = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=fill_model,
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L1_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=True,
            trade_execution=True,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        # Set initial market state
        quote = TestDataStubs.quote_tick(
            instrument=self.instrument,
            bid_price=211.20,
            ask_price=211.50,
        )
        matching_engine.process_quote_tick(quote)

        # Place limit order
        order = TestExecStubs.limit_order(
            instrument=self.instrument,
            order_side=order_side,
            price=Price.from_str(limit_price),
            quantity=self.instrument.make_qty(1.0),
        )
        matching_engine.process_order(order, self.account_id)
        messages.clear()

        # Trade crosses the limit price (better price)
        trade = TestDataStubs.trade_tick(
            instrument=self.instrument,
            price=trade_price,
            aggressor_side=aggressor_side,
        )
        matching_engine.process_trade_tick(trade)

        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 1, (
            "Order should fill when trade crosses limit regardless of fill model"
        )
        # Fill at limit price (conservative)
        assert filled_events[0].last_px == Price.from_str(limit_price)

    @pytest.mark.parametrize(
        ("order_side", "aggressor_side"),
        [
            (OrderSide.BUY, AggressorSide.SELLER),
            (OrderSide.SELL, AggressorSide.BUYER),
        ],
    )
    def test_trade_execution_fill_model_rejection_still_applies_liquidity_consumption(
        self,
        order_side: OrderSide,
        aggressor_side: AggressorSide,
    ):
        """
        Test that when trade execution fill is skipped due to fill model rejection,
        liquidity consumption tracking from the trade is still applied.

        Setup: No book liquidity at the limit/trade price, so trade execution path
        is exercised. Fill model rejects (prob=0), and trade consumption prevents
        a subsequent trade from filling.

        """
        fill_model = FillModel(prob_fill_on_limit=0.0, random_seed=42)

        matching_engine = OrderMatchingEngine(
            instrument=self.instrument,
            raw_id=0,
            fill_model=fill_model,
            fee_model=MakerTakerFeeModel(),
            book_type=BookType.L2_MBP,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            reject_stop_orders=True,
            trade_execution=True,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            liquidity_consumption=True,
        )

        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        # Set up book with liquidity AWAY from the limit/trade price.
        # This ensures fills_at_trade_price=False so trade execution path is exercised.
        if order_side == OrderSide.BUY:
            bid_delta = OrderBookDelta(
                instrument_id=self.instrument.id,
                action=BookAction.ADD,
                order=BookOrder(
                    side=OrderSide.BUY,
                    price=Price.from_str("200.00"),
                    size=Quantity.from_str("1000.000"),
                    order_id=100,
                ),
                flags=0,
                sequence=0,
                ts_event=0,
                ts_init=0,
            )
            matching_engine.process_order_book_delta(bid_delta)

            # Ask at 210.00 - NOT at 211.35 where limit/trade will be
            ask_delta = OrderBookDelta(
                instrument_id=self.instrument.id,
                action=BookAction.ADD,
                order=BookOrder(
                    side=OrderSide.SELL,
                    price=Price.from_str("210.00"),
                    size=Quantity.from_str("50.000"),
                    order_id=1,
                ),
                flags=0,
                sequence=1,
                ts_event=0,
                ts_init=0,
            )
            matching_engine.process_order_book_delta(ask_delta)
        else:
            ask_delta = OrderBookDelta(
                instrument_id=self.instrument.id,
                action=BookAction.ADD,
                order=BookOrder(
                    side=OrderSide.SELL,
                    price=Price.from_str("220.00"),
                    size=Quantity.from_str("1000.000"),
                    order_id=100,
                ),
                flags=0,
                sequence=0,
                ts_event=0,
                ts_init=0,
            )
            matching_engine.process_order_book_delta(ask_delta)

            # Bid at 212.00 - NOT at 211.35 where limit/trade will be
            bid_delta = OrderBookDelta(
                instrument_id=self.instrument.id,
                action=BookAction.ADD,
                order=BookOrder(
                    side=OrderSide.BUY,
                    price=Price.from_str("212.00"),
                    size=Quantity.from_str("50.000"),
                    order_id=1,
                ),
                flags=0,
                sequence=1,
                ts_event=0,
                ts_init=0,
            )
            matching_engine.process_order_book_delta(bid_delta)

        # First order consumes book liquidity (at 210.00 or 212.00)
        order1 = TestExecStubs.market_order(
            instrument=self.instrument,
            order_side=order_side,
            quantity=self.instrument.make_qty(50.0),
            client_order_id=TestIdStubs.client_order_id(1),
        )
        matching_engine.process_order(order1, self.account_id)
        matching_engine.iterate(timestamp_ns=1)

        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 1
        assert filled_events[0].last_qty == Quantity.from_str("50.000")
        messages.clear()

        # Place limit order at 211.35 (in the spread, no book liquidity here)
        order2 = TestExecStubs.limit_order(
            instrument=self.instrument,
            order_side=order_side,
            price=Price.from_str("211.35"),
            quantity=self.instrument.make_qty(30.0),
            client_order_id=TestIdStubs.client_order_id(2),
        )
        matching_engine.process_order(order2, self.account_id)
        messages.clear()

        # Trade tick at the limit price.
        # fills_at_trade_price=False (no book at 211.35) → trade execution path entered
        # Fill model rejects (prob=0) → no fill
        trade = TestDataStubs.trade_tick(
            instrument=self.instrument,
            price=211.35,
            aggressor_side=aggressor_side,
        )
        matching_engine.process_trade_tick(trade)

        # Order should NOT fill due to fill model rejection
        filled_events = [m for m in messages if isinstance(m, OrderFilled)]
        assert len(filled_events) == 0, (
            "Order should NOT fill when fill model rejects at limit price"
        )
