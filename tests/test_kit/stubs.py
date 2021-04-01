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

from datetime import datetime

import pytz

from nautilus_trader.core.uuid import uuid4
from nautilus_trader.model.bar import Bar
from nautilus_trader.model.bar import BarSpecification
from nautilus_trader.model.bar import BarType
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.events import OrderAccepted
from nautilus_trader.model.events import OrderCancelled
from nautilus_trader.model.events import OrderExpired
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.events import OrderRejected
from nautilus_trader.model.events import OrderSubmitted
from nautilus_trader.model.events import OrderTriggered
from nautilus_trader.model.events import PositionChanged
from nautilus_trader.model.events import PositionClosed
from nautilus_trader.model.events import PositionOpened
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ExecutionId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import OrderId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TradeMatchId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.tick import QuoteTick
from nautilus_trader.model.tick import TradeTick


# Unix epoch is the UTC time at 00:00:00 on 1/1/1970
UNIX_EPOCH = datetime(1970, 1, 1, 0, 0, 0, 0, tzinfo=pytz.utc)


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
            TestStubs.bartype_audusd_1min_bid(),
            Price("1.00002"),
            Price("1.00004"),
            Price("1.00001"),
            Price("1.00003"),
            Quantity(100000),
            0,
        )

    @staticmethod
    def bar_3decimal() -> Bar:
        return Bar(
            TestStubs.bartype_usdjpy_1min_bid(),
            Price("90.002"),
            Price("90.004"),
            Price("90.001"),
            Price("90.003"),
            Quantity(100000),
            0,
        )

    @staticmethod
    def quote_tick_3decimal(instrument_id=None, bid=None, ask=None) -> QuoteTick:
        return QuoteTick(
            instrument_id if instrument_id is not None else TestStubs.usdjpy_id(),
            bid if bid is not None else Price("90.002"),
            ask if ask is not None else Price("90.005"),
            Quantity(1),
            Quantity(1),
            0,
        )

    @staticmethod
    def quote_tick_5decimal(instrument_id=None, bid=None, ask=None) -> QuoteTick:
        return QuoteTick(
            instrument_id if instrument_id is not None else TestStubs.audusd_id(),
            bid if bid is not None else Price("1.00001"),
            ask if ask is not None else Price("1.00003"),
            Quantity(1),
            Quantity(1),
            0,
        )

    @staticmethod
    def trade_tick_5decimal(instrument_id=None, price=None) -> TradeTick:
        return TradeTick(
            instrument_id if instrument_id is not None else TestStubs.audusd_id(),
            price if price is not None else Price("1.00001"),
            Quantity(100000),
            OrderSide.BUY,
            TradeMatchId("123456"),
            0,
        )

    @staticmethod
    def trader_id() -> TraderId:
        return TraderId("TESTER", "000")

    @staticmethod
    def account_id() -> AccountId:
        return AccountId("SIM", "000")

    @staticmethod
    def event_account_state(account_id=None) -> AccountState:
        if account_id is None:
            account_id = TestStubs.account_id()

        return AccountState(
            account_id,
            [Money(1_000_000, USD)],
            [Money(1_000_000, USD)],
            [Money(0, USD)],
            {"default_currency": "USD"},
            uuid4(),
            0,
        )

    @staticmethod
    def event_order_submitted(order) -> OrderSubmitted:
        return OrderSubmitted(
            TestStubs.account_id(),
            order.cl_ord_id,
            0,
            uuid4(),
            0,
        )

    @staticmethod
    def event_order_accepted(order, order_id=None) -> OrderAccepted:
        if order_id is None:
            order_id = OrderId("1")
        return OrderAccepted(
            TestStubs.account_id(),
            order.cl_ord_id,
            order_id,
            0,
            uuid4(),
            0,
        )

    @staticmethod
    def event_order_rejected(order) -> OrderRejected:
        return OrderRejected(
            TestStubs.account_id(),
            order.cl_ord_id,
            0,
            "ORDER_REJECTED",
            uuid4(),
            0,
        )

    @staticmethod
    def event_order_filled(
        order,
        instrument,
        order_id=None,
        execution_id=None,
        position_id=None,
        strategy_id=None,
        last_qty=None,
        last_px=None,
        liquidity_side=LiquiditySide.TAKER,
        execution_ns=0,
    ) -> OrderFilled:
        if order_id is None:
            order_id = OrderId("1")
        if execution_id is None:
            execution_id = ExecutionId(order.cl_ord_id.value.replace("O", "E"))
        if position_id is None:
            position_id = order.position_id
        if strategy_id is None:
            strategy_id = order.strategy_id
        if last_px is None:
            last_px = Price("1.00000")
        if last_qty is None:
            last_qty = order.quantity

        commission = instrument.calculate_commission(
            last_qty=order.quantity,
            last_px=last_px,
            liquidity_side=liquidity_side,
        )

        return OrderFilled(
            account_id=TestStubs.account_id(),
            cl_ord_id=order.cl_ord_id,
            order_id=order_id,
            execution_id=execution_id,
            position_id=position_id,
            strategy_id=strategy_id,
            instrument_id=order.instrument_id,
            order_side=order.side,
            last_qty=last_qty,
            last_px=order.price if last_px is None else last_px,
            cum_qty=Quantity(order.filled_qty + last_qty),
            leaves_qty=Quantity(max(0, order.quantity - order.filled_qty - last_qty)),
            currency=instrument.quote_currency,
            is_inverse=instrument.is_inverse,
            commission=commission,
            liquidity_side=liquidity_side,
            execution_ns=execution_ns,
            event_id=uuid4(),
            timestamp_ns=0,
        )

    @staticmethod
    def event_order_cancelled(order) -> OrderCancelled:
        return OrderCancelled(
            TestStubs.account_id(),
            order.cl_ord_id,
            order.id,
            0,
            uuid4(),
            0,
        )

    @staticmethod
    def event_order_expired(order) -> OrderExpired:
        return OrderExpired(
            TestStubs.account_id(),
            order.cl_ord_id,
            order.id,
            0,
            uuid4(),
            0,
        )

    @staticmethod
    def event_order_triggered(order) -> OrderTriggered:
        return OrderTriggered(
            TestStubs.account_id(),
            order.cl_ord_id,
            order.id,
            0,
            uuid4(),
            0,
        )

    @staticmethod
    def event_position_opened(position) -> PositionOpened:
        return PositionOpened(
            position,
            position.last_event,
            uuid4(),
            0,
        )

    @staticmethod
    def event_position_changed(position) -> PositionChanged:
        return PositionChanged(
            position,
            position.last_event,
            uuid4(),
            0,
        )

    @staticmethod
    def event_position_closed(position) -> PositionClosed:
        return PositionClosed(
            position,
            position.last_event,
            uuid4(),
            0,
        )
