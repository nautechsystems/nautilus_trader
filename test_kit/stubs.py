# -------------------------------------------------------------------------------------------------
# <copyright file="stubs.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import uuid

from decimal import Decimal
from datetime import datetime, timedelta, timezone

from nautilus_trader.model.enums import Resolution, QuoteType, Currency, SecurityType
from nautilus_trader.model.objects import (
    Quantity,
    Venue,
    Symbol,
    Price,
    BarSpecification,
    BarType,
    Bar,
    Instrument)
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.core.correctness import ConditionFailed
from nautilus_trader.core.types import GUID, ValidString
from nautilus_trader.common.clock import TestClock
from nautilus_trader.model.enums import OrderSide, OrderType, OrderStatus, TimeInForce
from nautilus_trader.model.objects import Quantity, Venue, Symbol, Price
from nautilus_trader.model.identifiers import Label, IdTag, OrderId, ExecutionId, ExecutionTicket, PositionIdGenerator
from nautilus_trader.model.order import Order, OrderFactory
from nautilus_trader.model.position import Position
from nautilus_trader.model.events import OrderInitialized, OrderSubmitted, OrderAccepted, OrderRejected
from nautilus_trader.model.events import OrderWorking, OrderExpired, OrderModified, OrderCancelled
from nautilus_trader.model.events import OrderCancelReject, OrderPartiallyFilled, OrderFilled

# Unix epoch is the UTC time at 00:00:00 on 1/1/1970
UNIX_EPOCH = datetime(1970, 1, 1, 0, 0, 0, 0, timezone.utc)
AUDUSD_FXCM = Symbol('AUDUSD', Venue('FXCM'))
GBPUSD_FXCM = Symbol('GBPUSD', Venue('FXCM'))
USDJPY_FXCM = Symbol('USDJPY', Venue('FXCM'))
ONE_MINUTE_BID = BarSpecification(1, Resolution.MINUTE, QuoteType.BID)
ONE_MINUTE_ASK = BarSpecification(1, Resolution.MINUTE, QuoteType.ASK)
ONE_MINUTE_MID = BarSpecification(1, Resolution.MINUTE, QuoteType.MID)
ONE_SECOND_MID = BarSpecification(1, Resolution.SECOND, QuoteType.MID)


class TestStubs:

    @staticmethod
    def unix_epoch(offset_mins: int=0) -> datetime:
        """
        Generate a stub datetime based on the given offset from Unix epoch time.

        Unix time (also known as POSIX time or epoch time) is a system for
        describing instants in time, defined as the number of seconds that have
        elapsed since 00:00:00 Coordinated Universal Time (UTC), on Thursday,
        1 January 1970, minus the number of leap seconds which have taken place
        since then.

        :return: The unix epoch datetime plus any offset.
        """
        return UNIX_EPOCH + timedelta(minutes=offset_mins)

    @staticmethod
    def symbol_audusd_fxcm():
        return Symbol('AUDUSD', Venue('FXCM'))

    @staticmethod
    def symbol_gbpusd_fxcm():
        return Symbol('GBPUSD', Venue('FXCM'))

    @staticmethod
    def symbol_usdjpy_fxcm():
        return Symbol('USDJPY', Venue('FXCM'))

    @staticmethod
    def instrument_gbpusd():
        return Instrument(
            InstrumentId('GBPUSD.FXCM'),
            Symbol('GBPUSD', Venue('FXCM')),
            'GBP/USD',
            Currency.USD,
            SecurityType.FOREX,
            tick_precision=5,
            tick_size=Decimal('0.00001'),
            round_lot_size=Quantity(1000),
            min_stop_distance_entry=0,
            min_limit_distance_entry=0,
            min_stop_distance=0,
            min_limit_distance=0,
            min_trade_size=Quantity(1),
            max_trade_size=Quantity(50000000),
            rollover_interest_buy=Decimal(),
            rollover_interest_sell=Decimal(),
            timestamp=UNIX_EPOCH)

    @staticmethod
    def instrument_usdjpy():
        return Instrument(
            InstrumentId('USDJPY.FXCM'),
            Symbol('USDJPY', Venue('FXCM')),
            'USD/JPY',
            Currency.JPY,
            SecurityType.FOREX,
            tick_precision=3,
            tick_size=Decimal('0.001'),
            round_lot_size=Quantity(1000),
            min_stop_distance_entry=Decimal(),
            min_limit_distance_entry=Decimal(),
            min_stop_distance=Decimal(),
            min_limit_distance=Decimal(),
            min_trade_size=Quantity(1),
            max_trade_size=Quantity(50000000),
            rollover_interest_buy=Decimal(),
            rollover_interest_sell=Decimal(),
            timestamp=UNIX_EPOCH)

    @staticmethod
    def bartype_audusd_1min_bid():
        return BarType(AUDUSD_FXCM, ONE_MINUTE_BID)

    @staticmethod
    def bartype_audusd_1min_ask():
        return BarType(AUDUSD_FXCM, ONE_MINUTE_ASK)

    @staticmethod
    def bartype_gbpusd_1min_bid():
        return BarType(GBPUSD_FXCM, ONE_MINUTE_BID)

    @staticmethod
    def bartype_gbpusd_1min_ask():
        return BarType(GBPUSD_FXCM, ONE_MINUTE_ASK)

    @staticmethod
    def bartype_gbpusd_1sec_mid():
        return BarType(GBPUSD_FXCM, ONE_SECOND_MID)

    @staticmethod
    def bartype_usdjpy_1min_bid():
        return BarType(USDJPY_FXCM, ONE_MINUTE_BID)

    @staticmethod
    def bartype_usdjpy_1min_ask():
        return BarType(USDJPY_FXCM, ONE_MINUTE_ASK)

    @staticmethod
    def bar_5decimal():
        return Bar(Price('1.00002'),
                   Price('1.00004'),
                   Price('1.00001'),
                   Price('1.00003'),
                   100000,
                   UNIX_EPOCH)

    @staticmethod
    def bar_3decimal():
        return Bar(Price('90.002'),
                   Price('90.004'),
                   Price('90.001'),
                   Price('90.003'),
                   100000,
                   UNIX_EPOCH)

    @staticmethod
    def position(number=1, price=Price('1.00000')):
        clock = TestClock()

        generator = PositionIdGenerator(
            id_tag_trader=IdTag('001'),
            id_tag_strategy=IdTag('001'),
            clock=clock)

        for i in range(number - 1):
            generator.generate()

        order_factory = OrderFactory(
            id_tag_trader=IdTag('001'),
            id_tag_strategy=IdTag('001'),
            clock=clock)

        order = order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        order_filled = OrderFilled(
            order.id,
            ExecutionId('E123456'),
            ExecutionTicket('T123456'),
            order.symbol,
            order.side,
            order.quantity,
            price,
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        position_id = generator.generate()
        position = Position(position_id=position_id, fill_event=order_filled)

        return position

    @staticmethod
    def position_which_is_closed(number=1, close_price=Price('1.00010')):
        clock = TestClock()

        position = TestStubs.position(number=number)

        order_factory = OrderFactory(
            id_tag_trader=IdTag('001'),
            id_tag_strategy=IdTag('001'),
            clock=clock)

        order = order_factory.market(
            AUDUSD_FXCM,
            OrderSide.SELL,
            Quantity(100000))

        order_filled = OrderFilled(
            order.id,
            ExecutionId('E123456'),
            ExecutionTicket('T123456'),
            order.symbol,
            order.side,
            order.quantity,
            close_price,
            UNIX_EPOCH + timedelta(minutes=5),
            GUID(uuid.uuid4()),
            UNIX_EPOCH + timedelta(minutes=5))

        position.apply(order_filled)

        return position
