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
from nautilus_trader.model.events import OrderWorking
from nautilus_trader.model.events import PositionClosed
from nautilus_trader.model.events import PositionModified
from nautilus_trader.model.events import PositionOpened
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ExecutionId
from nautilus_trader.model.identifiers import OrderId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
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
    def symbol_btcusd_bitmex() -> Symbol:
        return Symbol("BTC/USD", Venue("BITMEX"))

    @staticmethod
    def symbol_ethusd_bitmex() -> Symbol:
        return Symbol("ETH/USD", Venue("BITMEX"))

    @staticmethod
    def symbol_btcusdt_binance() -> Symbol:
        return Symbol("BTC/USDT", Venue("BINANCE"))

    @staticmethod
    def symbol_ethusdt_binance() -> Symbol:
        return Symbol("ETH/USDT", Venue("BINANCE"))

    @staticmethod
    def symbol_audusd_fxcm() -> Symbol:
        return Symbol("AUD/USD", Venue("SIM"))

    @staticmethod
    def symbol_gbpusd_fxcm() -> Symbol:
        return Symbol("GBP/USD", Venue("SIM"))

    @staticmethod
    def symbol_usdjpy_fxcm() -> Symbol:
        return Symbol("USD/JPY", Venue("SIM"))

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
        return BarType(TestStubs.symbol_audusd_fxcm(), TestStubs.bar_spec_1min_bid())

    @staticmethod
    def bartype_audusd_1min_ask() -> BarType:
        return BarType(TestStubs.symbol_audusd_fxcm(), TestStubs.bar_spec_1min_ask())

    @staticmethod
    def bartype_gbpusd_1min_bid() -> BarType:
        return BarType(TestStubs.symbol_gbpusd_fxcm(), TestStubs.bar_spec_1min_bid())

    @staticmethod
    def bartype_gbpusd_1min_ask() -> BarType:
        return BarType(TestStubs.symbol_gbpusd_fxcm(), TestStubs.bar_spec_1min_ask())

    @staticmethod
    def bartype_gbpusd_1sec_mid() -> BarType:
        return BarType(TestStubs.symbol_gbpusd_fxcm(), TestStubs.bar_spec_1sec_mid())

    @staticmethod
    def bartype_usdjpy_1min_bid() -> BarType:
        return BarType(TestStubs.symbol_usdjpy_fxcm(), TestStubs.bar_spec_1min_bid())

    @staticmethod
    def bartype_usdjpy_1min_ask() -> BarType:
        return BarType(TestStubs.symbol_usdjpy_fxcm(), TestStubs.bar_spec_1min_ask())

    @staticmethod
    def bartype_btcusdt_binance_1min_bid() -> BarType:
        return BarType(TestStubs.symbol_btcusdt_binance(), TestStubs.bar_spec_1min_bid())

    @staticmethod
    def bartype_btcusdt_binance_100tick_last() -> BarType:
        return BarType(TestStubs.symbol_btcusdt_binance(), TestStubs.bar_spec_100tick_last())

    @staticmethod
    def bar_5decimal() -> Bar:
        return Bar(
            Price("1.00002"),
            Price("1.00004"),
            Price("1.00001"),
            Price("1.00003"),
            Quantity(100000),
            UNIX_EPOCH,
        )

    @staticmethod
    def bar_3decimal() -> Bar:
        return Bar(
            Price("90.002"),
            Price("90.004"),
            Price("90.001"),
            Price("90.003"),
            Quantity(100000),
            UNIX_EPOCH,
        )

    @staticmethod
    def quote_tick_3decimal(symbol=None) -> QuoteTick:
        return QuoteTick(
            symbol if symbol is not None else TestStubs.symbol_usdjpy_fxcm(),
            Price("90.002"),
            Price("90.003"),
            Quantity(1),
            Quantity(1),
            UNIX_EPOCH,
        )

    @staticmethod
    def quote_tick_5decimal(symbol=None) -> QuoteTick:
        return QuoteTick(
            symbol if symbol is not None else TestStubs.symbol_audusd_fxcm(),
            Price("1.00001"),
            Price("1.00003"),
            Quantity(1),
            Quantity(1),
            UNIX_EPOCH,
        )

    @staticmethod
    def trade_tick_5decimal(symbol=None) -> TradeTick:
        return TradeTick(
            symbol if symbol is not None else TestStubs.symbol_audusd_fxcm(),
            Price("1.00001"),
            Quantity(100000),
            OrderSide.BUY,
            TradeMatchId("123456"),
            UNIX_EPOCH,
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
            [Money(1_000_000, USD)],
            {"default_currency": "USD"},
            uuid4(),
            UNIX_EPOCH,
        )

    @staticmethod
    def event_order_submitted(order) -> OrderSubmitted:
        return OrderSubmitted(
            TestStubs.account_id(),
            order.cl_ord_id,
            UNIX_EPOCH,
            uuid4(),
            UNIX_EPOCH,
        )

    @staticmethod
    def event_order_accepted(order) -> OrderAccepted:
        return OrderAccepted(
            TestStubs.account_id(),
            order.cl_ord_id,
            OrderId("1"),
            UNIX_EPOCH,
            uuid4(),
            UNIX_EPOCH,
        )

    @staticmethod
    def event_order_rejected(order) -> OrderRejected:
        return OrderRejected(
            TestStubs.account_id(),
            order.cl_ord_id,
            UNIX_EPOCH,
            "ORDER_REJECTED",
            uuid4(),
            UNIX_EPOCH,
        )

    @staticmethod
    def event_order_filled(
            order,
            instrument,
            position_id=None,
            strategy_id=None,
            fill_price=None,
            fill_qty=None,
            liquidity_side=LiquiditySide.TAKER,
    ) -> OrderFilled:
        if position_id is None:
            position_id = PositionId(order.cl_ord_id.value.replace("P", "T"))
        if strategy_id is None:
            strategy_id = StrategyId.null()
        if fill_price is None:
            fill_price = Price("1.00000")
        if fill_qty is None:
            fill_qty = order.quantity

        commission = instrument.calculate_commission(
            quantity=order.quantity,
            avg_price=fill_price,
            liquidity_side=liquidity_side,
        )

        return OrderFilled(
            account_id=TestStubs.account_id(),
            cl_ord_id=order.cl_ord_id,
            order_id=OrderId("1"),
            execution_id=ExecutionId(order.cl_ord_id.value.replace("O", "E")),
            position_id=position_id,
            strategy_id=strategy_id,
            symbol=order.symbol,
            order_side=order.side,
            fill_qty=fill_qty,
            cum_qty=Quantity(order.filled_qty + fill_qty),
            leaves_qty=Quantity(max(0, order.quantity - order.filled_qty - fill_qty)),
            fill_price=order.price if fill_price is None else fill_price,
            currency=instrument.quote_currency,
            is_inverse=instrument.is_inverse,
            commission=commission,
            liquidity_side=liquidity_side,
            execution_time=UNIX_EPOCH,
            event_id=uuid4(),
            event_timestamp=UNIX_EPOCH,
        )

    @staticmethod
    def event_order_working(order, working_price=None) -> OrderWorking:
        if working_price is None:
            working_price = Price("1.00000")

        return OrderWorking(
            TestStubs.account_id(),
            order.cl_ord_id,
            OrderId("1"),
            order.symbol,
            order.side,
            order.type,
            order.quantity,
            order.price if working_price is None else working_price,
            order.time_in_force,
            order.expire_time,
            UNIX_EPOCH,
            uuid4(),
            UNIX_EPOCH,
        )

    @staticmethod
    def event_order_cancelled(order) -> OrderCancelled:
        return OrderCancelled(
            TestStubs.account_id(),
            order.cl_ord_id,
            OrderId("1"),
            UNIX_EPOCH,
            uuid4(),
            UNIX_EPOCH,
        )

    @staticmethod
    def event_order_expired(order) -> OrderExpired:
        return OrderExpired(
            TestStubs.account_id(),
            order.cl_ord_id,
            OrderId("1"),
            UNIX_EPOCH,
            uuid4(),
            UNIX_EPOCH,
        )

    @staticmethod
    def event_position_opened(position) -> PositionOpened:
        return PositionOpened(
            position,
            position.last_event,
            uuid4(),
            UNIX_EPOCH,
        )

    @staticmethod
    def event_position_modified(position) -> PositionModified:
        return PositionModified(
            position,
            position.last_event,
            uuid4(),
            UNIX_EPOCH,
        )

    @staticmethod
    def event_position_closed(position) -> PositionClosed:
        return PositionClosed(
            position,
            position.last_event,
            uuid4(),
            UNIX_EPOCH,
        )
