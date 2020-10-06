# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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
from datetime import timedelta

import pytz

from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.common.generators import PositionIdGenerator
from nautilus_trader.core.uuid import uuid4
from nautilus_trader.model.bar import Bar
from nautilus_trader.model.bar import BarSpecification
from nautilus_trader.model.bar import BarType
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import Maker
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
from nautilus_trader.model.identifiers import IdTag
from nautilus_trader.model.identifiers import MatchId
from nautilus_trader.model.identifiers import OrderId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instrument import ForexInstrument
from nautilus_trader.model.objects import Decimal
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.position import Position
from nautilus_trader.model.tick import QuoteTick
from nautilus_trader.model.tick import TradeTick

# Unix epoch is the UTC time at 00:00:00 on 1/1/1970
UNIX_EPOCH = datetime(1970, 1, 1, 0, 0, 0, 0, tzinfo=pytz.utc)


class TestStubs:

    @staticmethod
    def symbol_audusd_fxcm() -> Symbol:
        return Symbol("AUD/USD", Venue('FXCM'))

    @staticmethod
    def symbol_gbpusd_fxcm() -> Symbol:
        return Symbol("GBP/USD", Venue('FXCM'))

    @staticmethod
    def symbol_usdjpy_fxcm() -> Symbol:
        return Symbol("USD/JPY", Venue('FXCM'))

    @staticmethod
    def instrument_gbpusd() -> ForexInstrument:
        return ForexInstrument(
            Symbol("GBP/USD", Venue('FXCM')),
            price_precision=5,
            size_precision=0,
            min_stop_distance_entry=0,
            min_limit_distance_entry=0,
            min_stop_distance=0,
            min_limit_distance=0,
            tick_size=Decimal("0.00001"),
            lot_size=Quantity(1000),
            min_trade_size=Quantity(1),
            max_trade_size=Quantity(50000000),
            rollover_interest_buy=Decimal(),
            rollover_interest_sell=Decimal(),
            timestamp=UNIX_EPOCH,
        )

    @staticmethod
    def instrument_usdjpy() -> ForexInstrument:
        return ForexInstrument(
            Symbol("USD/JPY", Venue('FXCM')),
            price_precision=3,
            size_precision=0,
            min_stop_distance_entry=0,
            min_limit_distance_entry=0,
            min_stop_distance=0,
            min_limit_distance=0,
            tick_size=Decimal("0.001"),
            lot_size=Quantity(1000),
            min_trade_size=Quantity(1),
            max_trade_size=Quantity(50000000),
            rollover_interest_buy=Decimal(),
            rollover_interest_sell=Decimal(),
            timestamp=UNIX_EPOCH,
        )

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
    def quote_tick_3decimal(symbol) -> QuoteTick:
        return QuoteTick(
            symbol,
            Price("90.002"),
            Price("90.003"),
            Quantity(1),
            Quantity(1),
            UNIX_EPOCH,
        )

    @staticmethod
    def quote_tick_5decimal(symbol) -> QuoteTick:
        return QuoteTick(
            symbol,
            Price("1.00001"),
            Price("1.00003"),
            Quantity(1),
            Quantity(1),
            UNIX_EPOCH,
        )

    @staticmethod
    def trade_tick_5decimal(symbol) -> TradeTick:
        return TradeTick(
            symbol,
            Price("1.00001"),
            Quantity(100000),
            Maker.BUYER,
            MatchId("123456"),
            UNIX_EPOCH,
        )

    @staticmethod
    def trader_id() -> TraderId:
        return TraderId("TESTER", "000")

    @staticmethod
    def account_id() -> AccountId:
        return AccountId("0", "0", AccountType.SIMULATED)

    @staticmethod
    def account_event(account_id=None) -> AccountState:
        if account_id is None:
            account_id = TestStubs.account_id()

        return AccountState(
            account_id,
            Currency.USD(),
            Money(1000000.00, Currency.USD()),
            Money(1000000.00, Currency.USD()),
            Money(1000000.00, Currency.USD()),
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
            position_id=None,
            strategy_id=None,
            fill_price=None,
            filled_qty=None,
            leaves_qty=None,
            commission=0,
    ) -> OrderFilled:
        if position_id is None:
            position_id = PositionId(order.cl_ord_id.value.replace("P", "T"))
        if strategy_id is None:
            strategy_id = StrategyId("S", "NULL")
        if fill_price is None:
            fill_price = Price("1.00000")
        if filled_qty is None:
            filled_qty = order.quantity
        if leaves_qty is None:
            leaves_qty = Quantity()

        return OrderFilled(
            TestStubs.account_id(),
            order.cl_ord_id,
            OrderId("1"),
            ExecutionId(order.cl_ord_id.value.replace("O", "E")),
            position_id,
            strategy_id,
            order.symbol,
            order.side,
            filled_qty,
            leaves_qty,
            order.price if fill_price is None else fill_price,
            Money(commission, Currency.USD()),
            LiquiditySide.TAKER,
            Currency.USD(),  # Stub event
            Currency.USD(),  # Stub event
            UNIX_EPOCH,
            uuid4(),
            UNIX_EPOCH,
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
            position.last_event(),
            uuid4(),
            UNIX_EPOCH,
        )

    @staticmethod
    def event_position_modified(position) -> PositionModified:
        return PositionModified(
            position,
            position.last_event(),
            uuid4(),
            UNIX_EPOCH,
        )

    @staticmethod
    def event_position_closed(position) -> PositionClosed:
        return PositionClosed(
            position,
            position.last_event(),
            uuid4(),
            UNIX_EPOCH,
        )

    @staticmethod
    def position(number=1, entry_price=None) -> Position:
        if entry_price is None:
            entry_price = Price("1.00000")

        generator = PositionIdGenerator(id_tag_trader=IdTag("001"))

        for _i in range(number):
            generator.generate(TestStubs.symbol_audusd_fxcm())

        order_factory = OrderFactory(
            strategy_id=StrategyId("S", "001"),
            id_tag_trader=IdTag("001"),
            id_tag_strategy=IdTag("001"),
            clock=LiveClock(),
        )

        order = order_factory.market(
            TestStubs.symbol_audusd_fxcm(),
            OrderSide.BUY,
            Quantity(100000),
        )

        position_id = PositionId(TestStubs.symbol_audusd_fxcm().value)
        order_filled = TestStubs.event_order_filled(
            order,
            position_id=position_id,
            fill_price=entry_price,
        )

        position = Position(event=order_filled)

        return position

    @staticmethod
    def position_which_is_closed(position_id, close_price=None) -> Position:

        if close_price is None:
            close_price = Price("1.0001")

        order_factory = OrderFactory(
            strategy_id=StrategyId("S", "001"),
            id_tag_trader=IdTag("001"),
            id_tag_strategy=IdTag("001"),
        )

        order = order_factory.market(
            TestStubs.symbol_audusd_fxcm(),
            OrderSide.SELL,
            Quantity(100000),
        )

        filled1 = OrderFilled(
            TestStubs.account_id(),
            order.cl_ord_id,
            OrderId("1"),
            ExecutionId(order.cl_ord_id.value.replace('O', 'E')),
            position_id,
            StrategyId("S", "1"),
            order.symbol,
            order.side,
            order.quantity,
            Quantity(),
            close_price,
            Money(0, Currency.USD()),
            LiquiditySide.TAKER,
            Currency.USD(),  # Stub event
            Currency.USD(),  # Stub event
            UNIX_EPOCH + timedelta(minutes=5),
            uuid4(),
            UNIX_EPOCH + timedelta(minutes=5),
        )

        filled2 = OrderFilled(
            TestStubs.account_id(),
            order.cl_ord_id,
            OrderId("2"),
            ExecutionId(order.cl_ord_id.value.replace('O', 'E')),
            position_id,
            StrategyId("S", "1"),
            order.symbol,
            OrderSide.BUY,
            order.quantity,
            Quantity(),
            close_price,
            Money(0, Currency.USD()),
            LiquiditySide.TAKER,
            Currency.USD(),  # Stub event
            Currency.USD(),  # Stub event
            UNIX_EPOCH + timedelta(minutes=5),
            uuid4(),
            UNIX_EPOCH + timedelta(minutes=5),
        )

        position = Position(filled1)
        position.apply(filled2)

        return position
