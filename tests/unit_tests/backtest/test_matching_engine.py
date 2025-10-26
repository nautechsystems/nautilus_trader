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

from typing import Any

import pytest

from nautilus_trader.backtest.engine import OrderMatchingEngine
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.backtest.models import MakerTakerFeeModel
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.component import TestClock
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import InstrumentCloseType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import MarketStatusAction
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.objects import Price
from nautilus_trader.model.orders import MarketOrder
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

    @pytest.mark.skip(reason="WIP to introduce flags")
    def test_process_auction_book(self) -> None:
        # Arrange
        snapshot = TestDataStubs.order_book_snapshot(
            instrument=self.instrument,
            bid_price=100,
            ask_price=105,
        )
        self.matching_engine.process_order_book(snapshot)

        client_order: MarketOrder = TestExecStubs.market_order(
            instrument=self.instrument,
            order_side=OrderSide.BUY,
            time_in_force=TimeInForce.AT_THE_CLOSE,
        )
        self.cache.add_order(client_order)
        self.matching_engine.process_order(client_order, self.account_id)
        self.matching_engine.process_status(MarketStatusAction.PRE_OPEN)

        messages: list[Any] = []
        self.msgbus.register("ExecEngine.process", messages.append)

        # Act
        self.matching_engine.process_status(MarketStatusAction.PAUSE)

        # Assert
        assert self.matching_engine.msgbus.sent_count == 1
        assert isinstance(messages[0], OrderFilled)

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
        depth = TestDataStubs.order_book_depth10()
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
        assert self.matching_engine.best_ask_price() == Price.from_str("1000.0")

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
        assert self.matching_engine.best_bid_price() == Price.from_str("1000.0")

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
        assert self.matching_engine.best_bid_price() == Price.from_str("1000.0")
        assert self.matching_engine.best_ask_price() == Price.from_str("1000.0")

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
        assert matching_engine.best_bid_price() == Price.from_str("1000.0")
        assert matching_engine.best_ask_price() == Price.from_str("1000.0")

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
