#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="stubs.py" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

from decimal import Decimal
from datetime import datetime, timedelta, timezone

from inv_trader.model.enums import Venue, Resolution, QuoteType, CurrencyCode, SecurityType
from inv_trader.model.objects import Symbol, BarType, Instrument
# Unix epoch is the UTC time at 00:00:00 on 1/1/1970
UNIX_EPOCH = datetime(1970, 1, 1, 0, 0, 0, 0, timezone.utc)
AUDUSD_FXCM = Symbol('AUDUSD', Venue.FXCM)
GBPUSD_FXCM = Symbol('GBPUSD', Venue.FXCM)
USDJPY_FXCM = Symbol('USDJPY', Venue.FXCM)


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
    def instrument_gbpusd():
        return Instrument(Symbol('GBPUSD', Venue.FXCM),
                          'GBP/USD',
                          CurrencyCode.USD,
                          SecurityType.FOREX,
                          tick_precision=5,
                          tick_size=Decimal('0.00001'),
                          tick_value=Decimal('0.01'),
                          target_direct_spread=Decimal('0.00001'),
                          round_lot_size=1000,
                          contract_size=1,
                          min_stop_distance_entry=0,
                          min_limit_distance_entry=0,
                          min_stop_distance=0,
                          min_limit_distance=0,
                          min_trade_size=1,
                          max_trade_size=50000000,
                          margin_requirement=Decimal(),
                          rollover_interest_buy=Decimal(),
                          rollover_interest_sell=Decimal(),
                          timestamp=UNIX_EPOCH)

    @staticmethod
    def instrument_usdjpy():
        return Instrument(Symbol('USDJPY', Venue.FXCM),
                          'USD/JPY',
                          CurrencyCode.JPY,
                          SecurityType.FOREX,
                          tick_precision=3,
                          tick_size=Decimal('0.001'),
                          tick_value=Decimal('0.01'),
                          target_direct_spread=Decimal('0.001'),
                          round_lot_size=1000,
                          contract_size=1,
                          min_stop_distance_entry=Decimal(),
                          min_limit_distance_entry=Decimal(),
                          min_stop_distance=Decimal(),
                          min_limit_distance=Decimal(),
                          min_trade_size=1,
                          max_trade_size=50000000,
                          margin_requirement=Decimal(),
                          rollover_interest_buy=Decimal(),
                          rollover_interest_sell=Decimal(),
                          timestamp=UNIX_EPOCH)

    @staticmethod
    def bartype_audusd_1min_bid():
        return BarType(AUDUSD_FXCM, 1, Resolution.MINUTE, QuoteType.BID)

    @staticmethod
    def bartype_audusd_1min_ask():
        return BarType(AUDUSD_FXCM, 1, Resolution.MINUTE, QuoteType.ASK)

    @staticmethod
    def bartype_gbpusd_1min_bid():
        return BarType(GBPUSD_FXCM, 1, Resolution.MINUTE, QuoteType.BID)

    @staticmethod
    def bartype_gbpusd_1min_ask():
        return BarType(GBPUSD_FXCM, 1, Resolution.MINUTE, QuoteType.ASK)

    @staticmethod
    def bartype_gbpusd_1sec_mid():
        return BarType(GBPUSD_FXCM, 1, Resolution.SECOND, QuoteType.MID)

    @staticmethod
    def bartype_usdjpy_1min_bid():
        return BarType(USDJPY_FXCM, 1, Resolution.MINUTE, QuoteType.BID)

    @staticmethod
    def bartype_usdjpy_1min_ask():
        return BarType(USDJPY_FXCM, 1, Resolution.MINUTE, QuoteType.ASK)
