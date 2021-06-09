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

import asyncio
from datetime import datetime
from typing import List

import pytz

from nautilus_trader.cache.cache import Cache
from nautilus_trader.cache.database import BypassCacheDatabase
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import LiveLogger
from nautilus_trader.core.uuid import uuid4
from nautilus_trader.model.bar import Bar
from nautilus_trader.model.bar import BarSpecification
from nautilus_trader.model.bar import BarType
from nautilus_trader.model.c_enums.account_type import AccountType
from nautilus_trader.model.c_enums.orderbook_level import OrderBookLevel
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.data import Data
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.events import OrderAccepted
from nautilus_trader.model.events import OrderCanceled
from nautilus_trader.model.events import OrderExpired
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.events import OrderPendingCancel
from nautilus_trader.model.events import OrderPendingReplace
from nautilus_trader.model.events import OrderRejected
from nautilus_trader.model.events import OrderSubmitted
from nautilus_trader.model.events import OrderTriggered
from nautilus_trader.model.events import PositionChanged
from nautilus_trader.model.events import PositionClosed
from nautilus_trader.model.events import PositionOpened
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ExecutionId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TradeMatchId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orderbook.book import OrderBook
from nautilus_trader.model.orderbook.book import OrderBookSnapshot
from nautilus_trader.model.orderbook.ladder import Ladder
from nautilus_trader.model.orderbook.order import Order
from nautilus_trader.model.tick import QuoteTick
from nautilus_trader.model.tick import TradeTick
from nautilus_trader.trading.portfolio import Portfolio
from tests.test_kit.mocks import MockLiveDataEngine
from tests.test_kit.mocks import MockLiveExecutionEngine
from tests.test_kit.mocks import MockLiveRiskEngine
from tests.test_kit.providers import TestInstrumentProvider


# UNIX epoch is the UTC time at 00:00:00 on 1/1/1970
# https://en.wikipedia.org/wiki/Unix_time
UNIX_EPOCH = datetime(1970, 1, 1, 0, 0, 0, 0, tzinfo=pytz.utc)


class MyData(Data):
    """
    Represents an example user defined data class.
    """

    def __init__(
        self,
        value,
        ts_event_ns=0,
        timestamp_ns=0,
    ):
        super().__init__(ts_event_ns, timestamp_ns)
        self.value = value


class TestStubs:
    @staticmethod
    def btcusd_bitmex_id() -> InstrumentId:
        return InstrumentId(Symbol("BTC/USD"), Venue("BITMEX"))

    @staticmethod
    def ethusd_bitmex_id() -> InstrumentId:
        return InstrumentId(Symbol("ETH/USD"), Venue("BITMEX"))

    @staticmethod
    def btcusdt_binance_id() -> InstrumentId:
        return InstrumentId(Symbol("BTC/USDT"), Venue("BINANCE"))

    @staticmethod
    def ethusdt_binance_id() -> InstrumentId:
        return InstrumentId(Symbol("ETH/USDT"), Venue("BINANCE"))

    @staticmethod
    def audusd_id() -> InstrumentId:
        return InstrumentId(Symbol("AUD/USD"), Venue("SIM"))

    @staticmethod
    def gbpusd_id() -> InstrumentId:
        return InstrumentId(Symbol("GBP/USD"), Venue("SIM"))

    @staticmethod
    def usdjpy_id() -> InstrumentId:
        return InstrumentId(Symbol("USD/JPY"), Venue("SIM"))

    @staticmethod
    def bar_spec_1min_bid() -> BarSpecification:
        return BarSpecification(1, BarAggregation.MINUTE, PriceType.BID)

    @staticmethod
    def bar_spec_1min_ask() -> BarSpecification:
        return BarSpecification(1, BarAggregation.MINUTE, PriceType.ASK)

    @staticmethod
    def bar_spec_1min_mid() -> BarSpecification:
        return BarSpecification(1, BarAggregation.MINUTE, PriceType.MID)

    @staticmethod
    def bar_spec_1sec_mid() -> BarSpecification:
        return BarSpecification(1, BarAggregation.SECOND, PriceType.MID)

    @staticmethod
    def bar_spec_100tick_last() -> BarSpecification:
        return BarSpecification(100, BarAggregation.TICK, PriceType.LAST)

    @staticmethod
    def bartype_audusd_1min_bid() -> BarType:
        return BarType(TestStubs.audusd_id(), TestStubs.bar_spec_1min_bid())

    @staticmethod
    def bartype_audusd_1min_ask() -> BarType:
        return BarType(TestStubs.audusd_id(), TestStubs.bar_spec_1min_ask())

    @staticmethod
    def bartype_gbpusd_1min_bid() -> BarType:
        return BarType(TestStubs.gbpusd_id(), TestStubs.bar_spec_1min_bid())

    @staticmethod
    def bartype_gbpusd_1min_ask() -> BarType:
        return BarType(TestStubs.gbpusd_id(), TestStubs.bar_spec_1min_ask())

    @staticmethod
    def bartype_gbpusd_1sec_mid() -> BarType:
        return BarType(TestStubs.gbpusd_id(), TestStubs.bar_spec_1sec_mid())

    @staticmethod
    def bartype_usdjpy_1min_bid() -> BarType:
        return BarType(TestStubs.usdjpy_id(), TestStubs.bar_spec_1min_bid())

    @staticmethod
    def bartype_usdjpy_1min_ask() -> BarType:
        return BarType(TestStubs.usdjpy_id(), TestStubs.bar_spec_1min_ask())

    @staticmethod
    def bartype_btcusdt_binance_100tick_last() -> BarType:
        return BarType(
            TestStubs.btcusdt_binance_id(), TestStubs.bar_spec_100tick_last()
        )

    @staticmethod
    def bar_5decimal() -> Bar:
        return Bar(
            bar_type=TestStubs.bartype_audusd_1min_bid(),
            open_price=Price.from_str("1.00002"),
            high_price=Price.from_str("1.00004"),
            low_price=Price.from_str("1.00001"),
            close_price=Price.from_str("1.00003"),
            volume=Quantity.from_int(1_000_000),
            ts_event_ns=0,
            ts_recv_ns=0,
        )

    @staticmethod
    def bar_3decimal() -> Bar:
        return Bar(
            bar_type=TestStubs.bartype_usdjpy_1min_bid(),
            open_price=Price.from_str("90.002"),
            high_price=Price.from_str("90.004"),
            low_price=Price.from_str("90.001"),
            close_price=Price.from_str("90.003"),
            volume=Quantity.from_int(1_000_000),
            ts_event_ns=0,
            ts_recv_ns=0,
        )

    @staticmethod
    def quote_tick_3decimal(
        instrument_id=None, bid=None, ask=None, bid_volume=None, ask_volume=None
    ) -> QuoteTick:
        return QuoteTick(
            instrument_id=instrument_id
            if instrument_id is not None
            else TestStubs.usdjpy_id(),
            bid=bid or Price.from_str("90.002"),
            ask=ask or Price.from_str("90.005"),
            bid_size=bid_volume or Quantity.from_int(1_000_000),
            ask_size=ask_volume or Quantity.from_int(1_000_000),
            ts_event_ns=0,
            ts_recv_ns=0,
        )

    @staticmethod
    def quote_tick_5decimal(instrument_id=None, bid=None, ask=None) -> QuoteTick:
        return QuoteTick(
            instrument_id=instrument_id
            if instrument_id is not None
            else TestStubs.audusd_id(),
            bid=bid or Price.from_str("1.00001"),
            ask=ask or Price.from_str("1.00003"),
            bid_size=Quantity.from_int(1_000_000),
            ask_size=Quantity.from_int(1_000_000),
            ts_event_ns=0,
            ts_recv_ns=0,
        )

    @staticmethod
    def trade_tick_5decimal(
        instrument_id=None, price=None, aggressor_side=None, quantity=None
    ) -> TradeTick:
        return TradeTick(
            instrument_id=instrument_id or TestStubs.audusd_id(),
            price=price or Price.from_str("1.00001"),
            size=quantity or Quantity.from_int(100000),
            aggressor_side=aggressor_side or AggressorSide.BUY,
            match_id=TradeMatchId("123456"),
            ts_event_ns=0,
            ts_recv_ns=0,
        )

    @staticmethod
    def order(price: float, side: OrderSide, size=10):
        return Order(price=price, side=side, volume=size)

    @staticmethod
    def ladder(reverse: bool, orders: List[Order]):
        ladder = Ladder(reverse=reverse, price_precision=2, size_precision=2)
        for order in orders:
            ladder.add(order)
        return ladder

    @staticmethod
    def order_book(
        instrument=None,
        level=OrderBookLevel.L2,
        bid_price=10,
        ask_price=15,
        bid_levels=3,
        ask_levels=3,
        bid_volume=10,
        ask_volume=10,
    ) -> OrderBook:
        instrument = instrument or TestInstrumentProvider.default_fx_ccy("AUD/USD")
        order_book = OrderBook.create(
            instrument=instrument,
            level=level,
        )
        snapshot = TestStubs.order_book_snapshot(
            instrument_id=instrument.id,
            bid_price=bid_price,
            ask_price=ask_price,
            bid_levels=bid_levels,
            ask_levels=ask_levels,
            bid_volume=bid_volume,
            ask_volume=ask_volume,
        )
        order_book.apply_snapshot(snapshot)
        return order_book

    @staticmethod
    def order_book_snapshot(
        instrument_id=None,
        bid_price=10,
        ask_price=15,
        bid_levels=3,
        ask_levels=3,
        bid_volume=10,
        ask_volume=10,
        level=OrderBookLevel.L2,
    ) -> OrderBookSnapshot:
        err = "Too many levels generated; orders will be in cross. Increase bid/ask spread or reduce number of levels"
        assert bid_price < ask_price, err

        return OrderBookSnapshot(
            instrument_id=instrument_id or TestStubs.audusd_id(),
            level=level,
            bids=[(bid_price - i, bid_volume * (1 + i)) for i in range(bid_levels)],
            asks=[(ask_price + i, ask_volume * (1 + i)) for i in range(ask_levels)],
            ts_event_ns=0,
            ts_recv_ns=0,
        )

    @staticmethod
    def trader_id() -> TraderId:
        return TraderId("TESTER-000")

    @staticmethod
    def account_id() -> AccountId:
        return AccountId("SIM", "000")

    @staticmethod
    def strategy_id() -> StrategyId:
        return StrategyId("Test-1")

    @staticmethod
    def event_account_state(account_id=None) -> AccountState:
        if account_id is None:
            account_id = TestStubs.account_id()

        return AccountState(
            account_id=account_id,
            account_type=AccountType.CASH,
            base_currency=USD,
            reported=True,  # reported
            balances=[
                AccountBalance(
                    USD,
                    Money(1_000_000, USD),
                    Money(0, USD),
                    Money(1_000_000, USD),
                )
            ],
            info={},
            event_id=uuid4(),
            ts_updated_ns=0,
            timestamp_ns=0,
        )

    @staticmethod
    def event_order_submitted(order) -> OrderSubmitted:
        return OrderSubmitted(
            account_id=TestStubs.account_id(),
            client_order_id=order.client_order_id,
            ts_submitted_ns=0,
            event_id=uuid4(),
            timestamp_ns=0,
        )

    @staticmethod
    def event_order_accepted(order, venue_order_id=None) -> OrderAccepted:
        if venue_order_id is None:
            venue_order_id = VenueOrderId("1")
        return OrderAccepted(
            account_id=TestStubs.account_id(),
            client_order_id=order.client_order_id,
            venue_order_id=venue_order_id,
            ts_accepted_ns=0,
            event_id=uuid4(),
            timestamp_ns=0,
        )

    @staticmethod
    def event_order_rejected(order) -> OrderRejected:
        return OrderRejected(
            account_id=TestStubs.account_id(),
            client_order_id=order.client_order_id,
            reason="ORDER_REJECTED",
            ts_rejected_ns=0,
            event_id=uuid4(),
            timestamp_ns=0,
        )

    @staticmethod
    def event_order_pending_replace(order) -> OrderPendingReplace:
        return OrderPendingReplace(
            account_id=TestStubs.account_id(),
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            ts_pending_ns=0,
            event_id=uuid4(),
            timestamp_ns=0,
        )

    @staticmethod
    def event_order_pending_cancel(order) -> OrderPendingCancel:
        return OrderPendingCancel(
            account_id=TestStubs.account_id(),
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            ts_pending_ns=0,
            event_id=uuid4(),
            timestamp_ns=0,
        )

    @staticmethod
    def event_order_filled(
        order,
        instrument,
        venue_order_id=None,
        execution_id=None,
        position_id=None,
        strategy_id=None,
        last_qty=None,
        last_px=None,
        liquidity_side=LiquiditySide.TAKER,
        ts_filled_ns=0,
    ) -> OrderFilled:
        if venue_order_id is None:
            venue_order_id = VenueOrderId("1")
        if execution_id is None:
            execution_id = ExecutionId(order.client_order_id.value.replace("O", "E"))
        if position_id is None:
            position_id = order.position_id
        if strategy_id is None:
            strategy_id = order.strategy_id
        if last_px is None:
            last_px = Price.from_str(f"{1:.{instrument.price_precision}f}")
        if last_qty is None:
            last_qty = order.quantity

        commission = instrument.calculate_commission(
            last_qty=order.quantity,
            last_px=last_px,
            liquidity_side=liquidity_side,
        )

        return OrderFilled(
            account_id=TestStubs.account_id(),
            client_order_id=order.client_order_id,
            venue_order_id=venue_order_id,
            execution_id=execution_id,
            position_id=position_id,
            strategy_id=strategy_id,
            instrument_id=order.instrument_id,
            order_side=order.side,
            last_qty=last_qty,
            last_px=order.price if last_px is None else last_px,
            currency=instrument.quote_currency,
            commission=commission,
            liquidity_side=liquidity_side,
            ts_filled_ns=ts_filled_ns,
            event_id=uuid4(),
            timestamp_ns=0,
        )

    @staticmethod
    def event_order_canceled(order) -> OrderCanceled:
        return OrderCanceled(
            account_id=TestStubs.account_id(),
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            ts_canceled_ns=0,
            event_id=uuid4(),
            timestamp_ns=0,
        )

    @staticmethod
    def event_order_expired(order) -> OrderExpired:
        return OrderExpired(
            account_id=TestStubs.account_id(),
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            ts_expired_ns=0,
            event_id=uuid4(),
            timestamp_ns=0,
        )

    @staticmethod
    def event_order_triggered(order) -> OrderTriggered:
        return OrderTriggered(
            account_id=TestStubs.account_id(),
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            ts_triggered_ns=0,
            event_id=uuid4(),
            timestamp_ns=0,
        )

    @staticmethod
    def event_position_opened(position) -> PositionOpened:
        return PositionOpened(
            position=position,
            order_fill=position.last_event,
            event_id=uuid4(),
            timestamp_ns=0,
        )

    @staticmethod
    def event_position_changed(position) -> PositionChanged:
        return PositionChanged(
            position=position,
            order_fill=position.last_event,
            event_id=uuid4(),
            timestamp_ns=0,
        )

    @staticmethod
    def event_position_closed(position) -> PositionClosed:
        return PositionClosed(
            position=position,
            order_fill=position.last_event,
            event_id=uuid4(),
            timestamp_ns=0,
        )

    @staticmethod
    def clock() -> LiveClock:
        return LiveClock()

    @staticmethod
    def logger():
        return LiveLogger(loop=asyncio.get_event_loop(), clock=TestStubs.clock())

    @staticmethod
    def cache_db():
        return BypassCacheDatabase(
            trader_id=TestStubs.trader_id(),
            logger=TestStubs.logger(),
        )

    @staticmethod
    def cache():
        return Cache(
            database=TestStubs.cache_db(),
            logger=TestStubs.logger(),
        )

    @staticmethod
    def portfolio():
        return Portfolio(
            clock=TestStubs.clock(),
            cache=TestStubs.cache(),
            logger=TestStubs.logger(),
        )

    @staticmethod
    def mock_live_data_engine():
        return MockLiveDataEngine(
            loop=asyncio.get_event_loop(),
            portfolio=TestStubs.portfolio(),
            cache=TestStubs.cache(),
            clock=TestStubs.clock(),
            logger=TestStubs.logger(),
        )

    @staticmethod
    def mock_live_exec_engine():
        return MockLiveExecutionEngine(
            loop=asyncio.get_event_loop(),
            portfolio=TestStubs.portfolio(),
            cache=TestStubs.cache(),
            clock=TestStubs.clock(),
            logger=TestStubs.logger(),
        )

    @staticmethod
    def mock_live_risk_engine():
        return MockLiveRiskEngine(
            loop=asyncio.get_event_loop(),
            exec_engine=TestStubs.mock_live_exec_engine(),
            portfolio=TestStubs.portfolio(),
            cache=TestStubs.cache(),
            clock=TestStubs.clock(),
            logger=TestStubs.logger(),
        )
